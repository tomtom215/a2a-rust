#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Interop matrix: {a2a-rust, a2a-rs} client  x  {a2a-rust, a2a-rs} server  x  {JSONRPC, HTTP+JSON, GRPC}.
# Usage: run_matrix.sh <echo|llm> <outfile.jsonl>
set -u
MODE=$1; OUT=$2; BIN=${BIN:-$(cd "$(dirname "$0")" && pwd)/target/release}
: > "$OUT"
pkill -f "$BIN/agent-" 2>/dev/null; sleep 0.5
AGENT_MODE=$MODE "$BIN/agent-rust" 2>/tmp/agent-rust.$MODE.log &
AGENT_MODE=$MODE "$BIN/agent-rs"   2>/tmp/agent-rs.$MODE.log &
for u in http://127.0.0.1:7101 http://127.0.0.1:7201; do
  for i in $(seq 1 50); do curl -sf "$u/.well-known/agent-card.json" >/dev/null && break; sleep 0.2; done
done
LLM=""; [ "$MODE" = llm ] && LLM=--llm
for server in "a2a-rust http://127.0.0.1:7101" "a2a-rs http://127.0.0.1:7201"; do
  set -- $server; sname=$1; url=$2
  for driver in driver-rust driver-rs; do
    for b in JSONRPC HTTP+JSON GRPC; do
      timeout 900 "$BIN/$driver" "$url" "$b" $LLM 2>>/tmp/driver.$MODE.err \
        | python3 -c "import sys,json
for l in sys.stdin:
    d=json.loads(l); d['server']='$sname'; print(json.dumps(d))" >> "$OUT"
      echo "server=$sname driver=$driver binding=$b exit=${PIPESTATUS[0]}" >&2
    done
  done
done
pkill -f "$BIN/agent-"
