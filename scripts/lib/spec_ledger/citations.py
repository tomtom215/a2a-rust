# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The forms a ledger citation must take, and the check that each still holds.

Until 2026-10-23 the checker read a citation's file name and, at most, looked
for its symbol anywhere in the file: 1,145 of 1,313 `impl_evidence` entries
were a bare `path:line`, so an edit above a cited line moved the citation
silently, and 419 had pointed at a comment, a blank line or a brace from the
day they were written. Every citation now names what it points at, and the
line has to be where that thing is:

* **Rust implementation evidence** is `path:line Symbol`. `Symbol` is a named
  item (fn, struct, enum, union, trait, type, const, static, mod or
  macro_rules!) defined in that file, and `line` is the line of its head: the
  one with the keyword and the name. Attribute and doc-comment lines above it
  are not the definition. A method or associated item is cited `Type::name`,
  where `Type` is its impl's type or its trait (or `module::name` for an item
  in an inline module); a free item by its bare name.
* **Other implementation evidence** (Markdown, TOML, YAML, JSON, proto) is
  `path:line "fragment"`, and the fragment must appear on that line. A
  directory may be cited whole as `path/`.
* **Test evidence** is `path:line name`, optionally followed by a
  parenthesised remark, and the cited line must hold `fn name`: the `fn` line
  itself, not the `#[test]`-family attribute above it. A doctest's `fn` in a
  doc comment counts, as text. Non-Rust test evidence (a CI step, a script) is
  `path:line "fragment"` as above.

`fix_lines` moves a citation's line number to where its symbol, `fn` or
fragment now is when it is found exactly once in the file, and reports the
rest for a person: a citation whose target is gone or ambiguous is a question
about the evidence, not about a line number.
"""

from __future__ import annotations

import re
from pathlib import Path

from spec_ledger import rust_items

RUST_IMPL = re.compile(r"(?P<path>[\w./+-]+\.rs):(?P<line>\d+) (?P<sym>[A-Za-z_][\w]*(?:::[A-Za-z_]\w*)*)")
QUOTED = re.compile(r'(?P<path>[\w./+-]+):(?P<line>\d+) "(?P<frag>.+)"')
DIRECTORY = re.compile(r"(?P<path>[\w./+-]+)/")
TEST = re.compile(
    r"(?P<path>[\w./+-]+\.rs):(?P<line>\d+) (?P<name>[A-Za-z_]\w*)(?P<remark> \(.*\))?"
)


def _lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8", errors="replace").split("\n")


def _fn_lines(path: Path, name: str) -> list[int]:
    rx = re.compile(r"\bfn\s+" + re.escape(name) + r"\b")
    return [n for n, text in enumerate(_lines(path), 1) if rx.search(text)]


def _fragment_lines(path: Path, frag: str) -> list[int]:
    return [n for n, text in enumerate(_lines(path), 1) if frag in text]


def _symbol_lines(path: Path, sym: str) -> list[int]:
    return [it.line for it in rust_items.find(path, sym)]


def _defines(path: Path, line: int, sym: str) -> bool:
    return any(rust_items.matches(it, sym) for it in rust_items.defined_at(path, line))


def check_impl(root: Path, ev: str) -> str | None:
    """Why `ev` is not a valid implementation citation, or None."""
    m = RUST_IMPL.fullmatch(ev)
    if m:
        path = root / m["path"]
        if not path.is_file():
            return "no such file"
        n, sym = int(m["line"]), m["sym"]
        if _defines(path, n, sym):
            return None
        where = _symbol_lines(path, sym)
        if not where:
            return f"{sym!r} is not defined in {m['path']}"
        return f"line {n} is not where {sym!r} is defined (it is at {', '.join(map(str, where))})"
    m = QUOTED.fullmatch(ev)
    if m and not m["path"].endswith(".rs"):
        return _check_fragment(root, m)
    m = DIRECTORY.fullmatch(ev)
    if m and (root / m["path"]).is_dir():
        return None
    if re.match(r"[\w./+-]+\.rs\b", ev):
        return "not `path:line Symbol`, the form a Rust citation takes"
    return 'not `path:line "fragment"` (or `dir/`), the form a non-Rust citation takes'


def _check_fragment(root: Path, m: re.Match[str]) -> str | None:
    path = root / m["path"]
    if not path.is_file():
        return "no such file"
    lines = _lines(path)
    n = int(m["line"])
    if 1 <= n <= len(lines) and m["frag"] in lines[n - 1]:
        return None
    where = _fragment_lines(path, m["frag"])
    found = f" (it is on {', '.join(map(str, where))})" if where else " (nor anywhere in the file)"
    return f"line {n} does not contain {m['frag']!r}{found}"


def check_test(root: Path, ev: str) -> str | None:
    """Why `ev` is not a valid test citation, or None."""
    m = TEST.fullmatch(ev)
    if m:
        path = root / m["path"]
        if not path.is_file():
            return "no such file"
        where = _fn_lines(path, m["name"])
        if not where:
            # The wording is load-bearing: prove_gates_fail.sh greps it.
            return f"no `fn {m['name']}` in {m['path']}"
        if int(m["line"]) in where:
            return None
        return f"line {m['line']} is not the `fn {m['name']}` line (it is at {', '.join(map(str, where))})"
    m = QUOTED.fullmatch(ev)
    if m and not m["path"].endswith(".rs"):
        return _check_fragment(root, m)
    return "not `path:line fn_name` (Rust) or `path:line \"fragment\"` (anything else)"


def _relocate(root: Path, ev: str, test: bool) -> tuple[str | None, str]:
    """`ev` with its line number moved to its target, or None and why not."""
    for rx, kind in ((TEST if test else RUST_IMPL, "rust"), (QUOTED, "quoted")):
        m = rx.fullmatch(ev)
        if not m or (kind == "quoted" and m["path"].endswith(".rs")):
            continue
        path = root / m["path"]
        if not path.is_file():
            return None, "no such file"
        if kind == "quoted":
            where = _fragment_lines(path, m["frag"])
        elif test:
            where = _fn_lines(path, m["name"])
        else:
            where = _symbol_lines(path, m["sym"])
        if len(where) != 1:
            hint = ("; cite a line whose text is unique instead (a heading above "
                    "it), which this can move on its own") if len(where) > 1 else ""
            return None, f"found {len(where)} times{hint}"
        start, end = m.span("line")
        return ev[:start] + str(where[0]) + ev[end:], ""
    return None, "not in a citation form"


def fix_lines(root: Path, entries: list[dict]) -> tuple[int, list[str]]:
    """Rewrites stale line numbers in place; returns (fixed, unfixable)."""
    fixed, left = 0, []
    for e in entries:
        for key, check, test in (("impl_evidence", check_impl, False), ("test_evidence", check_test, True)):
            out = []
            for ev in e.get(key, []):
                if check(root, ev) is None:
                    out.append(ev)
                    continue
                new, why = _relocate(root, ev, test)
                if new is not None and check(root, new) is None:
                    out.append(new)
                    fixed += 1
                else:
                    out.append(ev)
                    left.append(f"{e.get('source', 'after_release')}:{e.get('spec_line', e.get('section'))} "
                                f"{ev!r}: {why or check(root, ev)}")
            e[key] = out
    return fixed, left
