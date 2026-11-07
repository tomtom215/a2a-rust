#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Package the four published crates the way `cargo publish` would, verify each
# tarball builds from its own contents, and unpack them for 02-stranger-build.sh.
#
# Needs: a stable cargo, network to crates.io (through any proxy), about 6 GB.
# Writes: $ADOPTER_SCRATCH/unpacked/<crate>-<version>/ and a tarball listing
# per crate on stdout.
set -euo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
cd "$REPO"

CRATES=(a2a-protocol-types a2a-protocol-client a2a-protocol-server a2a-protocol-sdk)
# Cargo caches an unpacked copy of a just-packaged, unpublished crate and
# reuses it (docs/handoff.md, Environment). A stale one verifies stale source.
rm -rf "$HOME"/.cargo/registry/src/-*/a2a-protocol-*-0.16.0 2>/dev/null || true

# One invocation: cargo packages in dependency order and resolves each
# crate's sibling dependency against the tarball it has just written, so
# verification works although 0.16.0 is not on crates.io.
args=()
for c in "${CRATES[@]}"; do args+=(-p "$c"); done
cargo package "${args[@]}" "$@"

rm -rf "$ADOPTER_SCRATCH/unpacked"
mkdir -p "$ADOPTER_SCRATCH/unpacked"
for c in "${CRATES[@]}"; do
  tgz=$(ls "$CARGO_TARGET_DIR"/package/"$c"-*.crate)
  tar -xzf "$tgz" -C "$ADOPTER_SCRATCH/unpacked"
  echo "== $tgz ($(stat -c %s "$tgz") bytes, $(tar -tzf "$tgz" | wc -l) files)"
done
