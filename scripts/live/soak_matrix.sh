#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The soak (tests/soak_multi_replica.rs, release build) three times each:
# with the upgrade guard on and off, direct to the database; then with turn
# claims on and off, through toxiproxy's pg_a link at 0, 1 and 5 ms added
# each way. Appends to $A2A_LIVE_OUT/soak-matrix.txt (default target/live).
# Needs the database at $A2A_LIVE_PG and toxiproxy's API on 127.0.0.1:8474
# (README.md). A2A_SOAK_SECS sets each run's length (default 30).
set -uo pipefail
ROOT=$(git rev-parse --show-toplevel)
OUT=${A2A_LIVE_OUT:-$ROOT/target/live}; mkdir -p "$OUT"
PG=${A2A_LIVE_PG:-postgres://a2a:a2a@127.0.0.1:5432}
DB=$(sed -E 's#^postgres://([^:]+):.*#\1#' <<<"$PG")   # the user's own database
PROXIED=$(sed -E 's#:[0-9]+$#:15432#' <<<"$PG")
export A2A_SOAK_SECS=${A2A_SOAK_SECS:-30}
T="http://127.0.0.1:8474/proxies/pg_a/toxics"
B=$(cd "$ROOT" && cargo test --release -p a2a-protocol-server --all-features --test soak_multi_replica --no-run --message-format=json 2>/dev/null \
    | python3 -c 'import json,sys; print([m["executable"] for m in map(json.loads, sys.stdin) if m.get("executable")][-1])')
toxics() {
  curl -s --noproxy '*' -X DELETE "$T/down" >/dev/null; curl -s --noproxy '*' -X DELETE "$T/up" >/dev/null
  if [ "$1" != 0 ]; then
    for dir in down up; do
      stream=$([ $dir = down ] && echo downstream || echo upstream)
      curl -s --noproxy '*' "$T" -d "{\"name\":\"$dir\",\"type\":\"latency\",\"stream\":\"$stream\",\"attributes\":{\"latency\":$1,\"jitter\":0}}" >/dev/null
    done
  fi
}
M=$OUT/soak-matrix.txt
for i in 1 2 3; do for g in on off; do
  echo "### direct guard=$g run=$i" >> "$M"
  A2A_TEST_POSTGRES_URL=$PG/$DB A2A_SOAK_UPGRADE_GUARD=$g "$B" --ignored --nocapture 2>&1 | grep -E "^soak:|pair latency|panicked" >> "$M"
done; done
for L in 0 1 5; do
  toxics $L; python3 -I "$ROOT/scripts/live/rtt.py" 15432 >> "$M"
  for i in 1 2 3; do for c in on off; do
    echo "### proxy latency_each_way=${L}ms claims=$c run=$i" >> "$M"
    A2A_TEST_POSTGRES_URL=$PROXIED/$DB A2A_SOAK_TURN_CLAIMS=$c "$B" --ignored --nocapture 2>&1 | grep -E "^soak:|pair latency|panicked" >> "$M"
  done; done
done
toxics 0; echo DONE >> "$M"
