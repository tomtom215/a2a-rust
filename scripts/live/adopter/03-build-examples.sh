#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Build, from this checkout and with the repository's CI environment
# (RUSTFLAGS="-D warnings"), the binaries the example and CLI walk-throughs run.
# Debug profile without debuginfo, to stay inside the disk allowance.
#
# Usage: 03-build-examples.sh PACKAGE...     (default: the offline set below)
# Needs: crates.io; several GB of target dir (ADOPTER_REPO_TARGET).
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
repo_env
cd "$REPO"
pkgs=("$@")
[ ${#pkgs[@]} -eq 0 ] && pkgs=(a2a-cli hello-agent echo-agent deploy-agent)
args=()
for p in "${pkgs[@]}"; do args+=(-p "$p"); done
time cargo build "${args[@]}" 2>&1 | grep -v '^\s*\(Compiling\|Downloaded\|Downloading\)'
echo "exit: ${PIPESTATUS[0]}"
