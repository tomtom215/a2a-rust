#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Shared environment for the adopter walk-through scripts. Source it; it
# changes nothing outside the shell that sources it.
#
#   ADOPTER_SCRATCH  where unpacked tarballs, stranger projects and target
#                    directories go (default: $TMPDIR/a2a-adopter, or /tmp/a2a-adopter)
#   REPO             this checkout (found from this file's location)
#
# `repo_env` is the repository's CI environment (warnings are errors); use it
# for anything built from this checkout. `stranger_env NAME` is what a person
# who just ran `cargo new` has: no RUSTFLAGS, its own target directory.

REPO="${REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)}"
ADOPTER_SCRATCH="${ADOPTER_SCRATCH:-${TMPDIR:-/tmp}/a2a-adopter}"
export REPO ADOPTER_SCRATCH
mkdir -p "$ADOPTER_SCRATCH"

repo_env() {
  export RUSTFLAGS="-D warnings" CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
  export CARGO_TARGET_DIR="${ADOPTER_REPO_TARGET:-$ADOPTER_SCRATCH/target-repo}"
}

stranger_env() {
  unset RUSTFLAGS
  export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
  export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/adopter-projects/$1/target"
}
