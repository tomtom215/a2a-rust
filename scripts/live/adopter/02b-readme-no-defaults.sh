#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The README says `default-features = false` opts the client and the SDK out of
# rustls, logging, OpenTelemetry and the webhook receiver. Does the README's own
# Quick Start still build and run when an adopter does that? And does the types
# crate, which the README sells as "no I/O", build on its own?
#   A  the README Quick Start with the SDK at default-features = false;
#   B  a stranger who wants only the wire types: a2a-protocol-types and serde_json.
#
# Needs: 01-package.sh done; crates.io; port 3000 free.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
export ADOPTER_SCRATCH
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0; unset RUSTFLAGS
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/_gs-target"
P="python3 -I $REPO/scripts/live/adopter/mkproject.py"

echo "##### A: README Quick Start, a2a-protocol-sdk with default-features = false"
$P readme-nodefaults --deps README.md:toml:0 --main README.md:rust:0 || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/readme-nodefaults" || exit 1
sed -i 's|^a2a-protocol-sdk = "0.16"|a2a-protocol-sdk = { version = "0.16", default-features = false }|' Cargo.toml
grep -n 'a2a-protocol-sdk' Cargo.toml
cargo build 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)' | head -30
echo "build exit: ${PIPESTATUS[0]}"
timeout 20 "$CARGO_TARGET_DIR/debug/readme-nodefaults" 2>&1 | head; echo "run exit: ${PIPESTATUS[0]}"

echo "##### B: a2a-protocol-types alone (no I/O): serde_json round trip of a message"
mkdir -p "$ADOPTER_SCRATCH/adopter-projects/types-only/src"
cd "$ADOPTER_SCRATCH/adopter-projects/types-only" || exit 1
cat > Cargo.toml <<EOF
[package]
name = "types-only"
version = "0.1.0"
edition = "2024"
[dependencies]
a2a-protocol-types = "0.16"
serde_json = "1"
$(sed -n '/^\[patch.crates-io\]/,$p' "$ADOPTER_SCRATCH/adopter-projects/readme-nodefaults/Cargo.toml")
EOF
cat > src/main.rs <<'EOF'
use a2a_protocol_types::{Message, Part};
fn main() {
    let m = Message::user_text("m1", "hello");
    let json = serde_json::to_string(&m).unwrap();
    println!("{json}");
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back.text(), Some("hello"));
    let _ = Part::text("x");
}
EOF
cargo run 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)' | head -20
