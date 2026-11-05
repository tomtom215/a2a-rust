#!/bin/bash
# Sequential live runs on the PR head: nothing else may use the CPU while this runs.
set -u
cd /home/user/a2a-pr6
OUT=target/live/delegation/pr
mkdir -p "$OUT"
{
  echo "head: $(git rev-parse HEAD)  dirty files: $(git status --porcelain | wc -l)"
  uname -sr; nproc; free -m | sed -n 2p; rustc -V; psql --version
  PGPASSWORD=postgres psql -h 127.0.0.1 -U postgres -tAc 'select version()'
  sha256sum target/live/delegation/debug/a2a-live-worker target/live/delegation/debug/a2a-live-supervisor
} > "$OUT/environment.txt" 2>&1
date -u +"build grpc start %H:%M:%S" >> "$OUT/times.txt"
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j3 --features grpc \
  --manifest-path scripts/live/delegation/Cargo.toml --target-dir target/live/delegation-grpc \
  --bin a2a-live-grpc-idle > "$OUT/build-grpc.log" 2>&1
echo "grpc build exit $?" >> "$OUT/times.txt"
B=target/live/delegation/debug
D="python3 scripts/live/delegation/delegation_live.py --worker $B/a2a-live-worker --supervisor $B/a2a-live-supervisor --children 50 --hold-secs 60 --kill-at 20 --lease-secs 15 --seed 20261104"
date -u +"afail,afailkeep start %H:%M:%S" >> "$OUT/times.txt"
$D --scenarios afail,afailkeep --runs 3 --restart-delays 5.4 --out "$OUT/fail" > "$OUT/fail-stdout.log" 2>&1
echo "fail exit $?" >> "$OUT/times.txt"
date -u +"c,c2,cx start %H:%M:%S" >> "$OUT/times.txt"
$D --scenarios c,c2,cx --runs 3 --out "$OUT/term" > "$OUT/term-stdout.log" 2>&1
echo "term exit $?" >> "$OUT/times.txt"
date -u +"grpc probe start %H:%M:%S" >> "$OUT/times.txt"
python3 scripts/live/delegation/grpc_idle_live.py --bin target/live/delegation-grpc/debug/a2a-live-grpc-idle > "$OUT/grpc-stdout.log" 2>&1
echo "grpc exit $?" >> "$OUT/times.txt"
date -u +"done %H:%M:%S" >> "$OUT/times.txt"
