#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Installs cargo-nextest into DIR from its release archive, checked against a
# pinned SHA-256 (scripts/ci/fetch_verified.sh), on the three runner platforms.
#
# usage: install_nextest.sh DIR
#
# `ci.yml`'s `test` job built nextest from source with `cargo install`: 5.9 to
# 7.1 minutes of each Windows and macOS job in run 45814698991 (2026-11-26).
# The archive downloads in seconds. The version is the one the mutation gate
# pins (`CARGO_NEXTEST_VERSION` in scripts/install_cargo_mutants.sh), read from
# there so the two cannot drift; a version this script has no digest for is
# refused, so a bump there fails here until the digests below are added.
#
# The digests are the SHA-256 of each archive as downloaded over TLS from the
# release URL on 2026-11-26, twice, with the same result; the release publishes
# no checksum file. The Linux binary was run and reported 0.9.146.

set -euo pipefail

if [ "$#" -ne 1 ]; then
  echo "usage: $0 DIR" >&2
  exit 2
fi
dest=$1
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
version=$(sed -n 's/^CARGO_NEXTEST_VERSION=//p' "$here/../install_cargo_mutants.sh")

case "$(uname -s)" in
  Linux) target=x86_64-unknown-linux-gnu exe=cargo-nextest ;;
  Darwin) target=universal-apple-darwin exe=cargo-nextest ;;
  MINGW* | MSYS* | CYGWIN*) target=x86_64-pc-windows-msvc exe=cargo-nextest.exe ;;
  *) echo "install_nextest: no archive for $(uname -s)" >&2; exit 1 ;;
esac

case "$version/$target" in
  0.9.146/x86_64-unknown-linux-gnu) sha=682c21b777c333e96fd532e114d3a5a894e0729ab88d94c0a9f20f8419695428 ;;
  0.9.146/universal-apple-darwin) sha=39785160b3c2f6ed9a765049cf4fa79f3b39aa02eb7598a5a0e2a1a0b9ffb9a8 ;;
  0.9.146/x86_64-pc-windows-msvc) sha=809b01f2c94d031ab6cf5f4d4a4f477c8f51b69c96ab3cb2fe4db7ecb222d0d3 ;;
  *) echo "install_nextest: no pinned digest for cargo-nextest $version on $target" >&2; exit 1 ;;
esac

mkdir -p "$dest"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive="cargo-nextest-$version-$target.tar.gz"
"$here/fetch_verified.sh" \
  "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-$version/$archive" \
  "$sha" "$work/$archive"
tar -xzf "$work/$archive" -C "$work" "$exe"
mv "$work/$exe" "$dest/$exe"
"$dest/$exe" nextest --version
