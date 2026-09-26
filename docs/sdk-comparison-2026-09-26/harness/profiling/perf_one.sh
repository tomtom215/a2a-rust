#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# perf_one.sh <name> <binary> <url>
set -u
cd "$(dirname "$0")"; P=/usr/lib/linux-tools-6.8.0-31/perf
NAME=$1; BINARY=$2; URL=$3
B='{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"hello"}]}}}'
AGENT_MODE=echo taskset -c 0,1 "$BINARY" >/dev/null 2>&1 & SP=$!
for i in $(seq 1 50); do curl -sf "${URL%/jsonrpc}/.well-known/agent-card.json" >/dev/null 2>&1 && break; curl -sf "$URL.well-known/agent-card.json" >/dev/null 2>&1 && break; sleep 0.1; done
taskset -c 2,3 /root/.cargo/bin/oha -z 3s -c 16 --no-tui -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' -d "$B" "$URL" >/dev/null 2>&1
$P record -F 1999 -p $SP -o perf.$NAME.data -- sleep 15 >/dev/null 2>&1 &
RP=$!
taskset -c 2,3 /root/.cargo/bin/oha -z 15s -c 16 --no-tui -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' -d "$B" "$URL" > oha.$NAME.txt 2>&1
wait $RP; kill $SP; wait $SP 2>/dev/null
$P report -i perf.$NAME.data --no-children --sort sym --stdio 2>/dev/null > perf.$NAME.txt
grep -m1 "Requests/sec" oha.$NAME.txt
