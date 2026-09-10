#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Pairs each timeout with the bound that encloses it, and fails when the inner
one is larger — or the same one is applied twice — with no recorded reason.

This is the entry point; the check itself is the `timeout_nesting` package
under scripts/lib (start with its `__init__.py`, which states the problem —
Addendum 8 of docs/v0.9.0-post-release-review.md, backlog item B14 — and maps
the modules). It is split so that every file stays under the 500-line
ratchet `scripts/check_file_lengths.sh` applies to `.py` files too.

Usage
-----
    python3 scripts/check_timeout_nesting.py            # the gate
    python3 scripts/check_timeout_nesting.py --explain  # every site, resolved,
                                                        # every pair, its verdict

Works from any cwd: the repository root and the helper package are both found
from this file's own location.

Exit codes: 0 clean, 1 at least one unjustified pair (or a stale allowlist
line), 2 the tree could not be read.
"""

from __future__ import annotations

import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / "lib"))

from timeout_nesting.report import main  # noqa: E402

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
