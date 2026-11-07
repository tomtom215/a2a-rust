#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Compile (and run, where the page does not say no_run) every Rust block of the
# README and the book against the unpacked tarballs, outside the workspace.
#
#   03-book-sweep.sh default   each a2a crate by version, default features
#   03-book-sweep.sh all       every feature of the four crates on
#
# Output: one line per doctest, `test <page> (line N) ... ok|FAILED`, then the
# compiler's message for each failure. Compare the two modes: a block that fails
# under `default` and passes under `all` needs a feature, and the page should say
# which.
#
# Needs: 01-package.sh done; crates/io; about 15 minutes and 4 GB of target dir
# per mode (the two modes share one, deleted by the caller).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
mode="${1:?default|all}"
export ADOPTER_SCRATCH
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
unset RUSTFLAGS
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/_sweep-target"
python3 -I "$REPO/scripts/live/adopter/mksweep.py" "sweep-$mode" "$mode" || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/sweep-$mode" || exit 1
cargo test --doc --no-fail-fast 2>&1
echo "cargo test --doc exit: $?"
