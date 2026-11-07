#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The README's Quick Start, typed exactly as written into a fresh project that
# depends on the unpacked tarballs of 01-package.sh: build (stable toolchain,
# no -D warnings), run for a few seconds, show the output the README claims.
#
# Needs: 01-package.sh done; network to crates.io; port 3000 free.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
stranger_env readme-quickstart
python3 -I "$REPO/scripts/live/adopter/mkproject.py" readme-quickstart \
  --deps README.md:toml:0 --main README.md:rust:0 || exit 1
cd "$ADOPTER_SCRATCH/adopter-projects/readme-quickstart"
echo "--- cargo build"; time cargo build 2>&1 | tail -40
echo "--- cargo run (10 s)"
timeout 10 cargo run -q 2>&1 | head -40
echo "exit: ${PIPESTATUS[0]}"
