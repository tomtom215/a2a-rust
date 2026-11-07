#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Build the four unpacked tarballs of 01-package.sh with EVERY feature on, from
# a project outside the repository: this is where a `.proto`, a vendored-protoc
# lookup or a `build.rs` input missing from a tarball would fail. `cargo package`
# verifies default features only. `cargo check` is enough: build scripts still
# run (protoc, tonic-prost-build), only code generation is skipped.
#
# Needs: 01-package.sh done; crates.io; about 2 GB (the target dir is deleted
# at the end unless KEEP_TARGET=1).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export ADOPTER_SCRATCH
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
unset RUSTFLAGS
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/_check-target"
python3 -I "$REPO/scripts/live/adopter/mksweep.py" check-all all || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/check-all" || exit 1
echo "--- features requested"; grep -E '^a2a-protocol' Cargo.toml
time cargo check --lib 2>&1 | grep -v '^\s*\(Compiling\|Checking\|Downloaded\|Downloading\)'
echo "cargo check exit: ${PIPESTATUS[0]}"
echo "--- which protoc did the build scripts use?"
ls "$CARGO_TARGET_DIR"/debug/build 2>/dev/null | grep -c . | sed 's/^/build-script dirs: /'
[ -n "${KEEP_TARGET:-}" ] || rm -rf "$CARGO_TARGET_DIR"
