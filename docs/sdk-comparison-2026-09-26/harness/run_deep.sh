#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Tier-2 interop matrix: deep_{rust,rs} clients x agent-{rust,rs} servers x 3 bindings.
# Usage: run_deep.sh <outfile.jsonl>
set -u
OUT=$1; BIN=${BIN:-$(cd "$(dirname "$0")" && pwd)/target/release}
: > "$OUT"
pkill -f "$BIN/agent-" 2>/dev/null; sleep 0.5
AGENT_MODE=echo "$BIN/agent-rust" >/dev/null 2>&1 &
AGENT_MODE=echo "$BIN/agent-rs" >/dev/null 2>&1 &
for u in http://127.0.0.1:7101 http://127.0.0.1:7201; do
  for i in $(seq 1 50); do curl -sf "$u/.well-known/agent-card.json" >/dev/null && break; sleep 0.2; done
done
for server in "a2a-rust http://127.0.0.1:7101" "a2a-rs http://127.0.0.1:7201"; do
  set -- $server; sname=$1; url=$2
  for driver in deep_rust deep_rs; do
    for b in JSONRPC HTTP+JSON GRPC; do
      timeout 300 "$BIN/$driver" "$url" "$b" 2>>"$OUT.err" \
        | python3 -c "import sys,json
for l in sys.stdin:
    d=json.loads(l); d['server']='$sname'; print(json.dumps(d))" >> "$OUT"
      echo "server=$sname driver=$driver binding=$b exit=${PIPESTATUS[0]}" >&2
    done
  done
done
pkill -f "$BIN/agent-"
