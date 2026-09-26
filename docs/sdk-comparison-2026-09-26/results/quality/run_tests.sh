#!/bin/bash
# usage: run_tests.sh <proj> <srcdir> <label> [extra cargo args...]
proj=$1; src=$2; label=$3; shift 3
export CARGO_TARGET_DIR=/opt/bench/target-quality-$proj
out=/opt/bench/results/quality/$proj/test-$label.log
cd $src
echo "CMD: CARGO_TARGET_DIR=$CARGO_TARGET_DIR cargo test -j 2 --workspace $* (cwd $src)" > $out
echo "START: $(date -u +%FT%TZ)" >> $out
cargo test -j 2 --workspace "$@" >> $out 2>&1
echo "EXIT: $?" >> $out
echo "END: $(date -u +%FT%TZ)" >> $out
