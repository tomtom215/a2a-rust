#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Wire capture: each client x each server over JSON-RPC and HTTP+JSON, through
# capture_proxy.py. One log per (client, server). gRPC is not proxied.
set -u
OUTDIR=$1; H=$(cd "$(dirname "$0")" && pwd); BIN=${BIN:-$H/target/release}
mkdir -p "$OUTDIR"; pkill -f "$BIN/agent-" 2>/dev/null; sleep 0.5
AGENT_MODE=echo "$BIN/agent-rust" >/dev/null 2>&1 &
AGENT_MODE=echo "$BIN/agent-rs" >/dev/null 2>&1 &
sleep 1.5
for client in rust rs; do
  for server in rust rs; do
    LOG="$OUTDIR/client-$client--server-$server.jsonl"; : > "$LOG"
    if [ $server = rust ]; then
      python3 "$H/capture_proxy.py" 8101 127.0.0.1:7101 "$LOG" 7101=8101 7102=8102 & P1=$!
      python3 "$H/capture_proxy.py" 8102 127.0.0.1:7102 "$LOG" 7101=8101 7102=8102 & P2=$!
      BASE=http://127.0.0.1:8101
    else
      python3 "$H/capture_proxy.py" 8201 127.0.0.1:7201 "$LOG" 7201=8201 & P1=$!; P2=
      BASE=http://127.0.0.1:8201
    fi
    sleep 0.7
    for b in JSONRPC HTTP+JSON; do
      "$BIN/driver-$client" "$BASE" "$b" > "$OUTDIR/checks-$client-$server-$b.jsonl" 2>/dev/null
      "$BIN/deep_$client" "$BASE" "$b" > "$OUTDIR/deep-$client-$server-$b.jsonl" 2>/dev/null
    done
    kill $P1 $P2 2>/dev/null; sleep 0.3
  done
done
pkill -f "$BIN/agent-"
