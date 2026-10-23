# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""`check_spec_ledger.py --self-test`: each citation rule, shown to refuse.

A rule that accepts everything passes on the real ledger as surely as a
correct one, so each is run here against a small fixture tree with a citation
it must accept and the near misses it must refuse: the attribute or doc line
above a definition, the `#[test]` line above a test, a method under the wrong
type, a fragment one line off, and the bare `path:line` that every rule
replaced. `fix_lines` is shown to move a shifted line and to leave an
ambiguous one for a person.
"""

from __future__ import annotations

import tempfile
from pathlib import Path

from spec_ledger import citations, rust_items

RUST = '''\
/// A thing.
#[derive(Debug)]
pub struct Thing {
    a: u8,
}

impl Thing {
    /// Goes.
    pub fn go(&self) -> &'static str {
        let s = "fn fake() { mode.use_x }";
        s
    }
}

#[test]
fn it_works() {}

mod inner {
    pub(crate) const fn twice() {}
}

fn dup() {}
mod again {
    fn dup() {}
}
'''

DOC = "# Title\n\nThe card MUST be served over TLS.\n"

# (citation, is a test citation, expected: None for valid, else a fragment of the refusal)
CASES = [
    ("src/lib.rs:3 Thing", False, None),
    ("src/lib.rs:9 Thing::go", False, None),
    ("src/lib.rs:9 go", False, None),
    ("src/lib.rs:19 inner::twice", False, None),
    ('doc.md:3 "MUST be served over TLS"', False, None),
    ("src/", False, None),
    ("src/lib.rs:16 it_works", True, None),
    ("src/lib.rs:16 it_works (a remark)", True, None),
    ('doc.md:1 "# Title"', True, None),
    ("src/lib.rs:2 Thing", False, "not where 'Thing' is defined (it is at 3)"),
    ("src/lib.rs:1 Thing", False, "not where 'Thing' is defined"),
    ("src/lib.rs:9 Other::go", False, "'Other::go' is not defined"),
    ("src/lib.rs:10 fake", False, "'fake' is not defined"),
    ("src/lib.rs:9", False, "not `path:line Symbol`"),
    ("src/lib.rs Thing", False, "not `path:line Symbol`"),
    ('doc.md:2 "MUST be served over TLS"', False, "line 2 does not contain 'MUST be served over TLS' (it is on 3)"),
    ("doc.md", False, 'not `path:line "fragment"`'),
    ("src/nope.rs:1 Thing", False, "no such file"),
    ("src/lib.rs:15 it_works", True, "line 15 is not the `fn it_works` line (it is at 16)"),
    ("src/lib.rs:16 missing", True, "no `fn missing` in src/lib.rs"),
    ("src/lib.rs it_works", True, "not `path:line fn_name`"),
]


def _run_cases(root: Path) -> list[str]:
    failed = []
    for ev, test, want in CASES:
        got = (citations.check_test if test else citations.check_impl)(root, ev)
        ok = got is None if want is None else (got is not None and want in got)
        if not ok:
            failed.append(f"{ev!r}: wanted {'acceptance' if want is None else want!r}, got {got!r}")
    return failed


def _run_fix(root: Path) -> list[str]:
    entries = [{
        "source": "spec", "spec_line": 1,
        "impl_evidence": ["src/lib.rs:4 Thing", "src/lib.rs:1 dup", 'doc.md:1 "MUST be served"'],
        "test_evidence": ["src/lib.rs:20 it_works (kept remark)"],
    }]
    fixed, left = citations.fix_lines(root, entries)
    want_impl = ["src/lib.rs:3 Thing", "src/lib.rs:1 dup", 'doc.md:3 "MUST be served"']
    failed = []
    if entries[0]["impl_evidence"] != want_impl:
        failed.append(f"fix_lines impl: {entries[0]['impl_evidence']}")
    if entries[0]["test_evidence"] != ["src/lib.rs:16 it_works (kept remark)"]:
        failed.append(f"fix_lines tests: {entries[0]['test_evidence']}")
    if fixed != 3 or len(left) != 1 or "dup" not in left[0] or "found 2 times" not in left[0]:
        failed.append(f"fix_lines counts: fixed {fixed}, left {left}")
    return failed


def self_test() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "src").mkdir()
        (root / "src/lib.rs").write_text(RUST, encoding="utf-8")
        (root / "doc.md").write_text(DOC, encoding="utf-8")
        failed = _run_cases(root) + _run_fix(root)
        # The scanner must not take a string's `fn fake`, or `mode.use_x`
        # (`mod` then `e`), for an item: the second once made a call site
        # in messaging/mod.rs a module named `e`.
        names = {it.name for it in rust_items.items_of(root / "src/lib.rs")}
        if names & {"fake", "e"} or names != {"Thing", "go", "it_works", "inner", "twice", "dup", "again"}:
            failed.append(f"scanner found {sorted(names)}")
    total = len(CASES) + 2
    for f in failed:
        print(f"check_spec_ledger self-test FAIL: {f}")
    print(f"check_spec_ledger self-test: {total - len(failed)} of {total} passed")
    return 1 if failed else 0
