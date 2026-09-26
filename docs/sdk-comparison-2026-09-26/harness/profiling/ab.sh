#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Alternating A/B: baseline (crates.io 0.14.0) vs boxing patch. Same profile, pinned CPUs.
set -u
OHA=/root/.cargo/bin/oha; OUT=/opt/bench/results/deep/prof/ab.jsonl; : > $OUT
B='{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"hello"}]}}}'
S='{"jsonrpc":"2.0","id":1,"method":"SendStreamingMessage","params":{"message":{"messageId":"m","role":"ROLE_USER","parts":[{"text":"hello"}]}}}'
for rep in 1 2 3; do for c in 1 16 64; do for wl in send stream; do for v in base patched; do
  bin=/opt/bench/prof-target/release/agent-rust; [ $v = patched ] && bin=/opt/bench/prof-target/release/agent-rust-patched
  AGENT_MODE=echo taskset -c 0,1 $bin >/dev/null 2>&1 & SP=$!
  for i in $(seq 1 50); do curl -sf http://127.0.0.1:7101/.well-known/agent-card.json >/dev/null && break; sleep 0.1; done
  body=$B; extra=(); [ $wl = stream ] && { body=$S; extra=(-H 'Accept: text/event-stream'); }
  taskset -c 2,3 $OHA -z 3s -c $c --no-tui -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' "${extra[@]}" -d "$body" http://127.0.0.1:7101/ >/dev/null 2>&1
  t0=$(awk '{print $14+$15}' /proc/$SP/stat)
  taskset -c 2,3 $OHA -z 15s -c $c --no-tui --output-format json -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' "${extra[@]}" -d "$body" http://127.0.0.1:7101/ > /tmp/ab.json 2>/dev/null
  t1=$(awk '{print $14+$15}' /proc/$SP/stat)
  python3 -c "
import json;d=json.load(open('/tmp/ab.json'));n=sum(int(v) for v in d['statusCodeDistribution'].values())
print(json.dumps({'v':'$v','wl':'$wl','c':$c,'rep':$rep,'rps':d['summary']['requestsPerSec'],'p50':d['latencyPercentiles']['p50']*1000,'p99':d['latencyPercentiles']['p99']*1000,'cpu_us':($t1-$t0)*10000/n,'status':d['statusCodeDistribution']}))" >> $OUT
  kill $SP; wait $SP 2>/dev/null
done; done; done; done
