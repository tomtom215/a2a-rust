#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Every tracked Cargo.lock must satisfy its manifests as committed.
#
# The repository holds four lockfiles, and a release refreshes the ones its
# checklist names. `itk/Cargo.lock` was not among them: on 2026-09-24 it still
# pinned this workspace's crates at 0.11.0 against 0.13.0 manifests, two
# releases stale. Nothing here built `itk/` with `--locked`, so nothing
# noticed — but the upstream a2a-itk harness does (`cargo build --locked
# --release`), and it refused to build this repository's agent, which is how
# the ACTS conformance run found it.
#
# `cargo metadata --locked` resolves without building and fails exactly when
# the lockfile would have to change. Exit codes: 0 all current, 1 any stale.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

stale=0
while IFS= read -r lock; do
    dir=$(dirname "$lock")
    if (cd "$dir" && cargo metadata --locked --format-version 1 >/dev/null 2>&1); then
        echo "ok     $lock"
    else
        echo "STALE  $lock — run \`cargo metadata --format-version 1 >/dev/null\` in $dir and commit it"
        stale=1
    fi
done < <(git ls-files '*Cargo.lock')
exit "$stale"
