#!/bin/bash
for n in a2a-rs a2a-rust; do d=/opt/bench/consumers/$n-combo-def
 export CARGO_TARGET_DIR=/opt/bench/target-quality-$n/geiger CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
 { echo "CMD: (cwd $d) CARGO_TARGET_DIR=$CARGO_TARGET_DIR CARGO_BUILD_JOBS=2 cargo geiger --output-format Ascii   (cargo-geiger 0.13.0; 'Failed to match (ignoring source)' noise lines filtered)"
   cd $d; timeout 1500 cargo geiger --output-format Ascii 2>&1 | grep -v "^Failed to match"; echo "EXIT: ${PIPESTATUS[0]}"; } > /opt/bench/results/quality/$n/geiger-combo-def.txt 2>&1
 rm -rf $CARGO_TARGET_DIR; done
