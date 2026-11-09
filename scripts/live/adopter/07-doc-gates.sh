#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# The repository's documentation gates, each run on its own with its own exit
# code (scripts/preflight.sh --list names them), plus an mdBook build and its
# link check. Nothing here is a verdict about the prose: a gate checks what it
# was written to check.
#
# Needs: python3, mdbook; no network. Uses a scratch copy of the book output.
set -uo pipefail
. "$(dirname "${BASH_SOURCE[0]}")/env.sh"
cd "$REPO"
run() { echo "##### $*"; "$@"; echo "exit: $?"; echo; }
run python3 scripts/check_doc_versions.py
run python3 scripts/check_book_examples.py
run python3 scripts/check_feature_tables.py
run python3 scripts/check_api_reference.py
run python3 scripts/check_doc_escapes.py
run bash scripts/check_book_code.sh
run bash scripts/check_benchmark_prose.sh
run python3 scripts/gen_sitemap.py --check
run python3 scripts/check_readme_doctests.py
run python3 scripts/check_compliance_map.py --check
run python3 scripts/check_panic_paths.py
out="$ADOPTER_SCRATCH/book-out"
rm -rf "$out"
echo "##### mdbook build book -d $out"
mdbook build book -d "$out"; echo "exit: $?"
cp -r book/static/. "$out/"
run python3 scripts/check_book_links.py "$out"
