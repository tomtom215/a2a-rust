#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Keep `tck/untested-musts.toml` true.

The official A2A TCK does not grade 22 of its MUST requirements against this
SDK (docs/official-tck-findings.md §16 and §24). The map names, for each, the
tests in this repository that assert it. A map like that rots quietly: a test
is renamed, deleted or ignored, and the entry goes on reading as evidence.
This makes the citations load-bearing.

Fails (exit 1) when:

  * an entry lacks `id`, `title`, `section`, `level`, `upstream` or `scope`,
    or has an empty one;
  * an entry has no tests;
  * a cited test does not exist: the file is missing, it declares no
    `fn <name>(`, or that function carries no `#[test]` / `#[tokio::test]`
    attribute in the lines above it;
  * a cited test is `#[ignore]`d, so CI would not run it;
  * two entries share an id, or one entry cites the same test twice.

Exit 2 when the file cannot be read or parsed.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "tck/untested-musts.toml"
FIELDS = ("id", "title", "section", "level", "upstream", "scope")
TEST_ATTR = re.compile(r"#\[\s*(?:tokio::)?test\b")
IGNORE_ATTR = re.compile(r"#\[\s*ignore\b")
# How far above `fn` the attributes may sit: doc comments, `#[cfg]`,
# `#[allow]` and the test attribute itself.
WINDOW = 8


def test_problem(ref: str) -> str | None:
    """None if `path::name` names a test function CI runs, else why not."""
    if "::" not in ref:
        return f"{ref!r} is not of the form path/to/file.rs::test_name"
    path, name = ref.rsplit("::", 1)
    f = ROOT / path
    if not f.is_file():
        return f"{ref}: {path} does not exist"
    lines = f.read_text(encoding="utf-8").splitlines()
    pat = re.compile(rf"\bfn {re.escape(name)}\s*[(<]")
    for i, line in enumerate(lines):
        if pat.search(line):
            window = lines[max(0, i - WINDOW) : i]
            if not any(TEST_ATTR.search(w) for w in window):
                return f"{ref}: `fn {name}` exists but carries no test attribute"
            if any(IGNORE_ATTR.search(w) for w in window):
                return f"{ref}: `fn {name}` is #[ignore]d, so CI does not run it"
            return None
    return f"{ref}: no `fn {name}` in {path}"


def check(entries: list) -> list[str]:
    problems = []
    seen: set[str] = set()
    for n, entry in enumerate(entries, 1):
        if not isinstance(entry, dict):
            problems.append(f"entry {n}: not a table")
            continue
        rid = entry.get("id") or f"entry {n}"
        for field in FIELDS:
            value = entry.get(field)
            if not isinstance(value, str) or not value.strip():
                problems.append(f"{rid}: `{field}` is missing or empty")
        if rid in seen:
            problems.append(f"{rid}: listed more than once")
        seen.add(rid)
        tests = entry.get("tests")
        if not isinstance(tests, list) or not tests:
            problems.append(f"{rid}: cites no tests")
            continue
        if len(set(tests)) != len(tests):
            problems.append(f"{rid}: cites a test more than once")
        for ref in tests:
            if not isinstance(ref, str):
                problems.append(f"{rid}: {ref!r} is not a string")
                continue
            why = test_problem(ref)
            if why:
                problems.append(f"{rid}: {why}")
    return problems


def main() -> int:
    try:
        data = tomllib.loads(SOURCE.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as e:
        print(f"check_tck_untested_map: cannot read {SOURCE}: {e}", file=sys.stderr)
        return 2
    entries = data.get("requirement")
    if not isinstance(entries, list) or not entries:
        print(f"check_tck_untested_map: {SOURCE} has no [[requirement]] entries", file=sys.stderr)
        return 2
    problems = check(entries)
    rel = SOURCE.relative_to(ROOT)
    if problems:
        for p in problems:
            print(f"FAIL {p}")
        print(f"check_tck_untested_map: {len(problems)} problem(s) in {rel}")
        return 1
    cited = sum(len(e["tests"]) for e in entries)
    print(f"check_tck_untested_map: OK, {len(entries)} requirements, {cited} test citations ({rel})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
