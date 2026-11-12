#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Instructions per blocking SendMessage for one agent binary, by callgrind, as the
# difference between a long and a short run so start-up cancels:
#   (Ir(long) - Ir(short)) / (requests(long) - requests(short)).
# Deterministic where throughput is not (a 4-vCPU shared machine varied 10-20% from
# one run to the next), and per function with callgrind_annotate. Two cautions it
# does not remove: callgrind counts a `rep movsb` copy per byte, so a large memcpy
# is far more "instructions" than time (check with scripts/run_perf.sh before
# believing a gain), and it sees the user-space work only, not the system calls or
# cross-thread wake-ups that cost most at two cores (strace -f -c shows those).
#
# usage: callgrind_per_request.sh AGENT_BINARY LABEL   (WORK names the build directory,
#   see README.md; the loadgen is $WORK/target-lg/release/loadgen; OUT, default
#   $WORK/callgrind, receives LABEL-6.out and LABEL-25.out for callgrind_annotate)
# Needs valgrind. The agent exits on its own (RUN_SECS), so callgrind writes its totals.
set -euo pipefail
bin=${1:?agent binary}; label=${2:?label}
W=${WORK:?set WORK, see README.md}; LG=$W/target-lg/release/loadgen; OUT=${OUT:-$W/callgrind}; PORT=${PORT:-3300}
mkdir -p "$OUT"
if fuser "$PORT/tcp" >/dev/null 2>&1; then echo "port $PORT is in use" >&2; exit 2; fi
run() { # seconds
  local secs=$1 out=$OUT/$label-$1.out
  rm -f "$out"
  RUN_SECS=$((secs + 40)) PORT=$PORT valgrind --tool=callgrind --callgrind-out-file="$out" "$bin" >/dev/null 2>"$OUT/$label-$secs.err" &
  local vp=$!
  for _ in $(seq 1 60); do (echo >/dev/tcp/127.0.0.1/$PORT) 2>/dev/null && break; sleep 1; done
  "$LG" 127.0.0.1:$PORT send 1 2 "$secs" >"$OUT/$label-$secs.res"
  wait "$vp" || true
  if grep -q AddrInUse "$OUT/$label-$secs.err"; then echo "the agent could not bind port $PORT: a stale process answered" >&2; exit 2; fi
  python3 - "$out" "$OUT/$label-$secs.res" <<'PY'
import json, re, sys
ir = int(re.search(r"^totals:\s+(\d+)", open(sys.argv[1]).read(), re.M).group(1))
print(json.load(open(sys.argv[2]))["ok"], ir)
PY
}
read -r ok1 ir1 < <(run 6)
read -r ok2 ir2 < <(run 25)
python3 -c "print(f'$label: {($ir2 - $ir1) / ($ok2 - $ok1):,.0f} instructions per request ({$ok2 - $ok1} requests between the runs)')"
