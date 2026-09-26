#!/bin/bash
# MSRV check with an MSRV-aware fresh resolution (copy of tree; identical procedure for both).
proj=$1; src=$2; tc=$3; shift 3
d=/opt/bench/msrv-lock/$proj; rm -rf $d; mkdir -p $d; tar -C $src --exclude=./target --exclude=./.git -cf - . | tar -C $d -xf -
export CARGO_TARGET_DIR=/opt/bench/target-quality-$proj/msrv-fallback CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback
O=/opt/bench/results/quality/$proj; cd $d
{ echo "CMD: (cwd $d, copy) CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +$tc generate-lockfile"; cargo +$tc generate-lockfile; echo "EXIT: $?"; } > $O/msrv-fallback-$tc-generate-lockfile.log 2>&1
for c in "$@"; do f=$O/msrv-fallback-$tc-$c.log
 { echo "CMD: (cwd $d) CARGO_RESOLVER_INCOMPATIBLE_RUST_VERSIONS=fallback cargo +$tc check -j 2 --locked -p $c"; cargo +$tc check -j 2 --locked -p $c; echo "EXIT: $?"; } > $f 2>&1; echo "$proj fallback $tc $c $(grep ^EXIT $f)"; done
