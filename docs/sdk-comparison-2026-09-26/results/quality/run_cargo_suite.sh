#!/bin/bash
# Identical cargo procedure for both projects.
# usage: run_cargo_suite.sh <proj> <srcdir> "<lib crates space-separated>" "<extra workspace args, e.g. --exclude X>"
proj=$1; src=$2; libs=$3; extra=$4
export CARGO_TARGET_DIR=/opt/bench/target-quality-$proj
# Disk on this VM is ~38 GB total and shared with another job; full debuginfo filled it (see *-DISKFULL.log).
# Same settings for both projects; they do not change test semantics.
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
O=/opt/bench/results/quality/$proj
cd $src
run(){ label=$1; shift; f=$O/$label.log; { echo "CMD: (cwd $src) CARGO_TARGET_DIR=$CARGO_TARGET_DIR CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 $*"; echo "START: $(date -u +%FT%TZ)"; "$@"; echo "EXIT: $?"; echo "END: $(date -u +%FT%TZ)"; } > $f 2>&1; echo "$label $(grep ^EXIT $f)"; }
case "$5" in ""|tests)
run test-default        cargo test -j 2 --workspace $extra
run test-all-features   cargo test -j 2 --workspace --all-features $extra
;; esac
case "$5" in ""|lint)
run clippy-default-Dwarnings cargo clippy -j 2 --workspace --all-targets $extra -- -D warnings
run clippy-default-count     cargo clippy -j 2 --workspace --all-targets $extra
run clippy-pedantic-count    cargo clippy -j 2 --workspace --all-targets $extra -- -W clippy::pedantic
pargs=""; for c in $libs; do pargs="$pargs -p $c"; done
run doc-no-deps cargo doc -j 2 --no-deps $pargs
for c in $libs; do run rustdoc-missing_docs-$c cargo rustdoc -j 2 -p $c -- -W missing_docs; done
;; esac
