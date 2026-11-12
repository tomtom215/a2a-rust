#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Installs mdBook into DIR, the one version every workflow that builds the book
# uses, checked against a pinned SHA-256 (scripts/ci/fetch_verified.sh).
#
# usage: install_mdbook.sh DIR
#
# `ci.yml` (the book's link check), `benchmarks.yml` (the pages it commits) and
# `docs.yml` (the deployed site) each carried their own copy of
# `curl -sSL <url> | tar -xz`; one of them failed on 2026-11-09 with "not in
# gzip format" and stopped every later gate of its job. They share this script,
# so the version and its digest change in one place, and
# scripts/check_workflow_downloads.py refuses a workflow that fetches anything
# else without a digest.
#
# The digest is the SHA-256 of mdbook-v0.4.40-x86_64-unknown-linux-gnu.tar.gz
# as downloaded over TLS from the release URL on 2026-11-11, twice, with the
# same result; the release publishes no checksum file. Change a version and its
# digest together.

set -euo pipefail

MDBOOK_VERSION=0.4.40
MDBOOK_SHA256=9ef07fd288ba58ff3b99d1c94e6d414d431c9a61fdb20348e5beb74b823d546b

if [ "$#" -ne 1 ]; then
  echo "usage: $0 DIR" >&2
  exit 2
fi
dest=$1
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

if [ -x "$dest/mdbook" ] && "$dest/mdbook" --version 2>&1 | grep -q "mdbook v$MDBOOK_VERSION"; then
  echo "install_mdbook: $dest/mdbook is already $MDBOOK_VERSION"
  exit 0
fi

mkdir -p "$dest"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
archive="mdbook-v$MDBOOK_VERSION-x86_64-unknown-linux-gnu.tar.gz"
"$here/fetch_verified.sh" \
  "https://github.com/rust-lang/mdBook/releases/download/v$MDBOOK_VERSION/$archive" \
  "$MDBOOK_SHA256" "$work/$archive"
tar -xzf "$work/$archive" -C "$work" mdbook
mv "$work/mdbook" "$dest/mdbook"
"$dest/mdbook" --version
