#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The README's MSRV claim (Rust 1.94) as an adopter meets it: rustc 1.94.0 and a
# stranger's project with no lockfile and no rust-version of its own, built against
# the unpacked tarballs. Cargo then resolves each dependency to the newest version the
# running rustc accepts (resolver 3's `fallback`), so a dependency whose MSRV is above
# 1.94 would fail here and not in CI's locked build.
#
# Needs: `rustup toolchain install 1.94.0 --profile minimal`; 01-package.sh done;
# crates.io; ~1 GB (the target directory is deleted at the end).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export ADOPTER_SCRATCH
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0; unset RUSTFLAGS
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/_msrv-target"
export RUSTUP_TOOLCHAIN=1.94.0
rustc --version; cargo --version
python3 -I "$REPO/scripts/live/adopter/mkproject.py" msrv-readme --deps README.md:toml:0 --main README.md:rust:0 || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/msrv-readme" || exit 1
rm -f Cargo.lock
cargo build 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)' | tail -25
echo "build exit: ${PIPESTATUS[0]}"
timeout 20 "$CARGO_TARGET_DIR/debug/msrv-readme" 2>&1 | head -5
echo "--- the same project with every default feature of the SDK and the tool-loop, grpc, websocket, sqlite features (what a larger adopter enables)"
sed -i 's|^a2a-protocol-sdk = "0.16"|a2a-protocol-sdk = { version = "0.16", features = ["grpc", "websocket", "sqlite", "tool-loop", "axum", "signing", "auth-jwt"] }|' Cargo.toml
cargo check 2>&1 | grep -v '^\s*\(Compiling\|Checking\|Downloaded\|Downloading\)' | tail -15
echo "check exit: ${PIPESTATUS[0]}"
rm -rf "$CARGO_TARGET_DIR"
