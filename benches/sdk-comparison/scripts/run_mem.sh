#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# RSS growth under sustained SendMessage load: 64 connections for DUR seconds,
# RSS sampled every 5 s. Server CPUs 0-1, load CPUs 2-3. Output: mem.jsonl
W=${WORK:?set WORK to the scratch directory, see README.md}; LG=$W/target-lg/release/loadgen; OUT=${OUT:-$W/bench/perf/mem.jsonl}; DUR=${DUR:-60}
: > $OUT
for s in a2a-rs a2a-rust; do
  if [ $s = a2a-rs ]; then B="env NODELAY=1 $W/target-rs/release/agent-a2a-rs"; else B=$W/target-rust/release/agent-a2a-rust; fi
  PORT=3300 taskset -c 0,1 $B > /dev/null 2>&1 & pid=$!; sleep 0.5
  # `env` execs, so $pid is the server itself
  (taskset -c 2,3 $LG 127.0.0.1:3300 send 64 0 $DUR > $W/bench/perf/mem-$s-load.json) & lpid=$!
  t=0; while kill -0 $lpid 2>/dev/null; do
    echo "{\"server\":\"$s\",\"t_s\":$t,\"rss_kb\":$(awk '/VmRSS/{print $2}' /proc/$pid/status)}" >> $OUT; sleep 5; t=$((t+5)); done
  echo "{\"server\":\"$s\",\"t_s\":\"end\",\"rss_kb\":$(awk '/VmRSS/{print $2}' /proc/$pid/status),\"requests_ok\":$(python3 -c "import json;print(json.load(open('$W/bench/perf/mem-$s-load.json'))['ok'])")}" >> $OUT
  kill $pid; wait $pid 2>/dev/null
done
cat $OUT
