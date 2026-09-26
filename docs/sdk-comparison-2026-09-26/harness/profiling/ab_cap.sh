#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
set -u
OHA=/root/.cargo/bin/oha; OUT=/opt/bench/results/deep/prof/ab_cap.jsonl; : > $OUT
BIN=/opt/bench/harness/target/release/agent-rust
B='{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"hello"}]}}}'
for rep in 1 2 3; do for c in 1 16 64; do for v in default 32; do
  if [ $v = default ]; then AGENT_MODE=echo taskset -c 0,1 $BIN >/dev/null 2>&1 & else QUEUE_CAP=32 AGENT_MODE=echo taskset -c 0,1 $BIN >/dev/null 2>&1 & fi; SP=$!
  for i in $(seq 1 50); do curl -sf http://127.0.0.1:7101/.well-known/agent-card.json >/dev/null && break; sleep 0.1; done
  taskset -c 2,3 $OHA -z 3s -c $c --no-tui -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' -d "$B" http://127.0.0.1:7101/ >/dev/null 2>&1
  t0=$(awk '{print $14+$15}' /proc/$SP/stat)
  taskset -c 2,3 $OHA -z 15s -c $c --no-tui --output-format json -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' -d "$B" http://127.0.0.1:7101/ > /tmp/abc.json 2>/dev/null
  t1=$(awk '{print $14+$15}' /proc/$SP/stat)
  python3 -c "
import json;d=json.load(open('/tmp/abc.json'));n=sum(int(v) for v in d['statusCodeDistribution'].values())
print(json.dumps({'v':'$v','c':$c,'rep':$rep,'rps':d['summary']['requestsPerSec'],'cpu_us':($t1-$t0)*10000/n,'status':d['statusCodeDistribution']}))" >> $OUT
  kill $SP; wait $SP 2>/dev/null
done; done; done
