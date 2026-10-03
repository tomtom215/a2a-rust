#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Echo-agent throughput/latency matrix. Server pinned to CPUs 0-1, load
# generator to CPUs 2-3. Fresh server process per (server, mode, conns, rep)
# cell so no cell inherits another's store contents. Output: perf.jsonl
set -u
W=${WORK:?set WORK to the scratch directory, see README.md}; LG=$W/target-lg/release/loadgen; OUT=${OUT:-$W/bench/perf/perf.jsonl}
WARM=${WARM:-3}; MEAS=${MEAS:-10}; REPS=${REPS:-3}; CONNS=${CONNS:-"1 16 64 256"}; MODES=${MODES:-"send stream"}
declare -A BIN=( [a2a-rs-tuned]=$W/target-rs/release/agent-a2a-rs [a2a-rs-asshipped]=$W/target-rs/release/agent-a2a-rs [a2a-rust]=$W/target-rust/release/agent-a2a-rust )
declare -A ENVS=( [a2a-rs-tuned]="NODELAY=1" [a2a-rs-asshipped]="" [a2a-rust]="" )
SERVERS=(a2a-rs-tuned a2a-rust a2a-rs-asshipped)
: > $OUT
rss() { awk '/VmRSS|VmHWM/{printf "%s=%s ", $1, $2}' /proc/$1/status; }
for rep in $(seq 1 $REPS); do
 for mode in $MODES; do
  for c in $CONNS; do
   # rotate server order each rep
   for i in 0 1 2; do
    s=${SERVERS[$(( (i + rep) % 3 ))]}
    env PORT=3200 ${ENVS[$s]} taskset -c 0,1 ${BIN[$s]} > /dev/null 2>&1 &
    pid=$!; sleep 0.5
    r0=$(awk '/VmRSS/{print $2}' /proc/$pid/status)
    res=$(taskset -c 2,3 $LG 127.0.0.1:3200 $mode $c $WARM $MEAS)
    r1=$(awk '/VmRSS/{print $2}' /proc/$pid/status); h1=$(awk '/VmHWM/{print $2}' /proc/$pid/status)
    cpu=$(awk '{print $14+$15}' /proc/$pid/stat)
    kill $pid; wait $pid 2>/dev/null
    echo "$res" | python3 -c "import sys,json; d=json.load(sys.stdin); d.update(server='$s', rep=$rep, rss_start_kb=$r0, rss_end_kb=$r1, hwm_kb=$h1, cpu_ticks=$cpu); print(json.dumps(d))" >> $OUT
    tail -1 $OUT | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['server'], d['mode'], d['connections'], 'rep', d['rep'], 'rps %.0f'%d['rps'], 'p50', d['lat_us']['p50'], 'p99', d['lat_us']['p99'], 'err', d['errors'], 'rss', d['rss_end_kb'])"
   done
  done
 done
done
