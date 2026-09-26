#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# SDK overhead benchmark. Run ONLY on an otherwise idle machine.
# Servers pinned to CPUs 0-1, load generators to CPUs 2-3. Echo mode (no model),
# so the measurement is the SDK's request path, not the executor.
# Output: $RESULTS/perf/<ts>/*.json + summary.jsonl
set -u
BENCH=${BENCH:-/opt/bench}
BIN=${BIN:-$(cd "$(dirname "$0")" && pwd)/target/release}
OHA=${OHA:-/root/.cargo/bin/oha}
GHZ=${GHZ:-$BENCH/tools/ghz}
PROTO=$BENCH/a2a-rust-acts/proto/a2a_v1
OUT=${RESULTS:-$BENCH/results}/perf/$(date -u +%Y%m%dT%H%M%SZ); mkdir -p "$OUT"
DUR=${DUR:-20s}; REPS=${REPS:-3}; CONCS=${CONCS:-"1 16 64"}
SUM="$OUT/summary.jsonl"

cpu_ticks() { awk '{print $14+$15}' /proc/$1/stat; }
rss_kib() { awk '/VmRSS/{print $2}' /proc/$1/status; }

start_server() { # $1 = rust|rs
  pkill -f "$BIN/agent-" 2>/dev/null; sleep 0.5
  AGENT_MODE=echo taskset -c 0,1 "$BIN/agent-$1" 2>/dev/null &
  SPID=$!
  for i in $(seq 1 50); do curl -sf "$CARD/.well-known/agent-card.json" >/dev/null && break; sleep 0.1; done
}

jsonrpc_body() { echo "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":{\"message\":{\"messageId\":\"m-$RANDOM$RANDOM\",\"role\":\"ROLE_USER\",\"parts\":[{\"text\":\"hello\"}]}}}"; }

run_http() { # sdk workload url body extra_header...
  local sdk=$1 wl=$2 url=$3 body=$4; shift 4
  for c in $CONCS; do
    for r in $(seq 1 $REPS); do
      start_server "$sdk_bin"
      # warm-up
      taskset -c 2,3 $OHA -z 3s -c "$c" --no-tui -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' "$@" -d "$body" "$url" >/dev/null 2>&1
      local t0=$(cpu_ticks $SPID)
      taskset -c 2,3 $OHA -z "$DUR" -c "$c" --no-tui --output-format json -m POST -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' "$@" -d "$body" "$url" > "$OUT/$sdk-$wl-c$c-r$r.json" 2>/dev/null
      local t1=$(cpu_ticks $SPID) rss=$(rss_kib $SPID)
      python3 - "$OUT/$sdk-$wl-c$c-r$r.json" "$sdk" "$wl" "$c" "$r" "$((t1-t0))" "$rss" >> "$SUM" <<'EOF'
import json,sys
f,sdk,wl,c,r,ticks,rss=sys.argv[1:]
d=json.load(open(f)); s=d['summary']; p=d['latencyPercentiles']
codes=d.get('statusCodeDistribution',{})
n=sum(int(v) for v in codes.values())
ok=int(codes.get('200',0))
print(json.dumps({'sdk':sdk,'workload':wl,'conc':int(c),'rep':int(r),'rps':s['requestsPerSec'],
 'p50_ms':p['p50']*1000,'p99_ms':p['p99']*1000,'success_rate':s['successRate'],'status':codes,
 'errors':d.get('errorDistribution',{}),'server_cpu_us_per_req':(int(ticks)*10000/ n) if n else None,
 'rss_kib_end':int(rss)}))
EOF
    done
  done
}

run_grpc() { # sdk port
  local sdk=$1 port=$2
  for c in $CONCS; do
    for r in $(seq 1 $REPS); do
      start_server "$sdk_bin"
      taskset -c 2,3 $GHZ --insecure --proto $PROTO/a2a.proto -i $PROTO,/usr/include --call lf.a2a.v1.A2AService/SendMessage \
        -m '{"a2a-version":"1.0"}' -d '{"message":{"message_id":"{{.UUID}}","role":"ROLE_USER","parts":[{"text":"hello"}]}}' \
        -z 3s -c "$c" 127.0.0.1:$port >/dev/null 2>&1
      local t0=$(cpu_ticks $SPID)
      taskset -c 2,3 $GHZ --insecure --proto $PROTO/a2a.proto -i $PROTO,/usr/include --call lf.a2a.v1.A2AService/SendMessage \
        -m '{"a2a-version":"1.0"}' -d '{"message":{"message_id":"{{.UUID}}","role":"ROLE_USER","parts":[{"text":"hello"}]}}' \
        -z "$DUR" -c "$c" -O json 127.0.0.1:$port > "$OUT/$sdk-grpc-c$c-r$r.json" 2>/dev/null
      local t1=$(cpu_ticks $SPID) rss=$(rss_kib $SPID)
      python3 - "$OUT/$sdk-grpc-c$c-r$r.json" "$sdk" "$c" "$r" "$((t1-t0))" "$rss" >> "$SUM" <<'EOF'
import json,sys
f,sdk,c,r,ticks,rss=sys.argv[1:]
d=json.load(open(f))
lat={x['percentage']:x['latency']/1e6 for x in d['latencyDistribution']}
n=d['count']; codes=d.get('statusCodeDistribution',{})
print(json.dumps({'sdk':sdk,'workload':'grpc_send','conc':int(c),'rep':int(r),'rps':d['rps'],
 'p50_ms':lat.get(50),'p99_ms':lat.get(99),'success_rate':codes.get('OK',0)/n if n else 0,'status':codes,
 'errors':d.get('errorDistribution',{}),'server_cpu_us_per_req':int(ticks)*10000/n if n else None,'rss_kib_end':int(rss)}))
EOF
    done
  done
}

# a2a-rust: JSON-RPC :7101, REST :7102, gRPC :7103
sdk_bin=rust; CARD=http://127.0.0.1:7101
run_http a2a-rust jsonrpc_send http://127.0.0.1:7101/ "$(jsonrpc_body SendMessage)"
run_http a2a-rust rest_send http://127.0.0.1:7102/message:send '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"hello"}]}}'
run_http a2a-rust jsonrpc_stream http://127.0.0.1:7101/ "$(jsonrpc_body SendStreamingMessage)" -H 'Accept: text/event-stream'
run_grpc a2a-rust 7103
# a2a-rs: JSON-RPC :7201/jsonrpc, REST :7201/rest, gRPC :7203
sdk_bin=rs; CARD=http://127.0.0.1:7201
run_http a2a-rs jsonrpc_send http://127.0.0.1:7201/jsonrpc "$(jsonrpc_body SendMessage)"
run_http a2a-rs rest_send http://127.0.0.1:7201/rest/message:send '{"message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"hello"}]}}'
run_http a2a-rs jsonrpc_stream http://127.0.0.1:7201/jsonrpc "$(jsonrpc_body SendStreamingMessage)" -H 'Accept: text/event-stream'
run_grpc a2a-rs 7203
pkill -f "$BIN/agent-"
echo "$OUT"
