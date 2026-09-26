#!/bin/bash
# usage: run_msrv.sh <proj> <src> <toolchain> <crates...>
proj=$1; src=$2; tc=$3; shift 3
export CARGO_TARGET_DIR=/opt/bench/target-quality-$proj/msrv CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
O=/opt/bench/results/quality/$proj; cd $src
for c in "$@"; do f=$O/msrv-$tc-$c.log
 { echo "CMD: (cwd $src) CARGO_TARGET_DIR=$CARGO_TARGET_DIR cargo +$tc check -j 2 --locked -p $c"; rustc +$tc -V; cargo +$tc check -j 2 --locked -p $c; echo "EXIT: $?"; } > $f 2>&1; echo "$proj $tc $c $(grep ^EXIT $f)"; done
