#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The book's install command for the CLI, as written:
#     cargo install --path tools/a2a-cli
# `cargo install` ignores Cargo.lock unless given --locked, so this resolves the
# dependency tree afresh from crates.io, as a stranger's machine would today. A
# transitive dependency that moved since the lock was written is what this finds.
# `--debug` only to save disk and time (the book's command builds release); no
# RUSTFLAGS, as a stranger has none.
#
# Needs: crates.io, ~2 GB, several minutes. Leaves nothing behind but the log
# (the target directory and the install root are deleted unless KEEP=1).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
unset RUSTFLAGS
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="$ADOPTER_SCRATCH/cli-install-target"
root="$ADOPTER_SCRATCH/cli-install"
cd "$REPO" || exit 1
echo "--- cargo install --path tools/a2a-cli --debug --root $root (no --locked)"
time cargo install --path tools/a2a-cli --debug --root "$root" 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)' | tail -40
echo "install exit: ${PIPESTATUS[0]}"
"$root/bin/a2a" --version
[ -n "${KEEP:-}" ] || rm -rf "$CARGO_TARGET_DIR" "$root"
