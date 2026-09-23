#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fails when a published crate's README is no longer compiled as a doctest.

Each crate's README is its crates.io page, and until 2026-09-23 nothing
compiled it: the client's documented three methods that did not exist, and
seven of the eight Rust blocks across the four READMEs failed to compile when
first tried (audit C5, T8; escape class 1). Each `lib.rs` now carries

    #[cfg(doctest)]
    #[doc = include_str!("../README.md")]
    struct ReadmeDoctests;

so `cargo test` runs every Rust block in the README. The doctests themselves
are what catch a wrong README. This catches the other failure: the three
lines being deleted or edited in a refactor, after which every README is
unchecked again and every gate stays green.

Exit 0 when all four crates carry the include; 1 otherwise.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ("a2a-protocol-types", "a2a-protocol-client", "a2a-protocol-server", "a2a-protocol-sdk")
INCLUDE = re.compile(
    r'#\[cfg\(doctest\)\]\s*#\[doc = include_str!\("\.\./README\.md"\)\]\s*(pub )?struct \w+;'
)


def main() -> int:
    missing = [
        c for c in CRATES if not INCLUDE.search((ROOT / "crates" / c / "src" / "lib.rs").read_text())
    ]
    for c in missing:
        print(f"crates/{c}/src/lib.rs: README.md is not included as a doctest, so nothing compiles it")
    if missing:
        return 1
    print(f"check_readme_doctests: all {len(CRATES)} READMEs compile as doctests")
    return 0


if __name__ == "__main__":
    sys.exit(main())
