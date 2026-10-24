# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Citation rules for `scripts/check_spec_ledger.py`.

This package is the body of the ledger checker's citation checks; that file
is the entry point and puts `scripts/lib` on `sys.path`. The split exists for
`scripts/check_file_lengths.sh`, which counts every tracked `.py` file
against a 500-line ratchet, helpers included.

`rust_items` finds the named items a Rust file defines and the line each is
defined on; `citations` holds the three rules a ledger citation must meet
and the `--fix-lines` rewrite.
"""
