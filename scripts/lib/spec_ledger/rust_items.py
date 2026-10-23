# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The named items a Rust source file defines, and the line of each.

Not a Rust parser: a lexer that blanks comments, strings and character
literals, then a line scan for item heads and a brace match for their
bodies. That is enough for what the ledger needs, which is "does line N
define `fn x` (inside `impl T`)?" and "which item encloses line N?", on the
code this repository writes (rustfmt-formatted, one item head per line).
"""

from __future__ import annotations

import bisect
import re
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path

HEAD = re.compile(
    r"^\s*(?:#\[[^\]]*\]\s*)*"
    r"(?:pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:(?:default|const|async|unsafe|extern(?:\s+\"[^\"]*\")?)\s+)*"
    r"(?P<kind>(?:fn|struct|enum|union|trait|type|const|static(?:\s+mut)?|mod)\s|macro_rules!)"
    r"\s*(?:r#)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)"
)
IMPL = re.compile(r"^\s*(?:unsafe\s+)?impl\b")
# The implemented type: the last path segment before any generics, after
# `for` when there is one (`impl<T> Trait<U> for a::Type<T> where ...`).
IMPL_TYPE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\s*(?:<.*)?$")
RAW_STRING = re.compile(r'b?r(#*)"')
CHAR = re.compile(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'")


@dataclass
class Item:
    kind: str  # fn, struct, enum, union, trait, type, const, static, mod, macro, or impl
    name: str  # for an impl, the implemented type's name
    line: int  # 1-based line of the head
    end: int  # 1-based line of the body's last line
    parent: "Item | None" = field(default=None, repr=False)

    @property
    def owner(self) -> str | None:
        """The type a method belongs to: its impl's type or its trait."""
        p = self.parent
        if p is not None and p.kind in ("impl", "trait"):
            return p.name
        return None

    @property
    def qualified(self) -> str:
        return f"{self.owner}::{self.name}" if self.owner else self.name


def _ident_before(text: str, i: int) -> bool:
    """Whether `text[i]` continues an identifier (`br"` in `abr"` is not a
    raw string)."""
    return i > 0 and (text[i - 1].isalnum() or text[i - 1] == "_")


def blank_non_code(text: str) -> str:
    """`text` with comments, string and char literals replaced by spaces
    (newlines kept), so braces and keywords in them cannot be miscounted."""
    out = list(text)
    i, n = 0, len(text)

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif (m := RAW_STRING.match(text, i)) and not _ident_before(text, i):
            close = '"' + m.group(1)
            j = text.find(close, m.end())
            j = n if j < 0 else j + len(close)
            blank(i, j)
            i = j
        elif c == '"' or (text.startswith('b"', i) and not _ident_before(text, i)):
            j = i + (2 if c == "b" else 1)
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            blank(i, j + 1)
            i = j + 1
        elif c == "'":
            m = CHAR.match(text, i)
            if m:
                blank(i, m.end())
                i = m.end()
            else:
                i += 1  # a lifetime or a label
        else:
            i += 1
    return "".join(out)


def _closing_brace(code: str, i: int) -> int:
    """Offset of the `}` that closes the `{` at offset `i`."""
    depth = 0
    for j in range(i, len(code)):
        if code[j] == "{":
            depth += 1
        elif code[j] == "}":
            depth -= 1
            if depth == 0:
                return j
    return len(code) - 1


def _body_end(code: str, start: int, kind: str) -> int:
    """Offset of the end of the item whose head starts at offset `start`.

    A const, static or type alias runs to its `;` (braces in its value are
    skipped); anything else to the brace that closes its first top-level `{`,
    or to a `;` before one (`struct X;`, `mod tests;`, `fn f();` in a trait).
    """
    parens, i, n = 0, start, len(code)
    while i < n:
        c = code[i]
        if c in "([":
            parens += 1
        elif c in ")]":
            parens -= 1
        elif c == ";" and parens <= 0:
            return i
        elif c == "{":
            if kind not in ("const", "static", "type") and parens <= 0:
                return _closing_brace(code, i)
            i = _closing_brace(code, i)
        i += 1
    return n - 1


def scan(text: str) -> list[Item]:
    """Every item and impl block in `text`, with parents, in source order."""
    code = blank_non_code(text)
    lines = code.split("\n")
    line_starts = [0]
    for ln in lines[:-1]:
        line_starts.append(line_starts[-1] + len(ln) + 1)
    items: list[Item] = []
    for idx, ln in enumerate(lines):
        m = HEAD.match(ln)
        if m:
            kind = m.group("kind")
            kind = "macro" if kind == "macro_rules!" else kind.split()[0]
            off = line_starts[idx] + m.start("kind")
            end = bisect.bisect_right(line_starts, _body_end(code, off, kind))
            items.append(Item(kind, m.group("name"), idx + 1, end))
        elif IMPL.match(ln):
            off = line_starts[idx]
            brace = code.find("{", off)
            semi = code.find(";", off)
            if brace < 0 or (0 <= semi < brace):
                continue
            head = " ".join(code[off:brace].split())
            head = re.sub(r"\bwhere\b.*$", "", head).strip()
            head = re.sub(r"^(?:unsafe\s+)?impl\s*(<(?:[^<>]|<[^<>]*>)*>)?\s*", "", head)
            if re.search(r"\sfor\s", " " + head + " "):
                head = re.split(r"\sfor\s", " " + head + " ")[-1].strip()
            head = head.lstrip("&").removeprefix("mut ").removeprefix("dyn ").strip()
            # Strip generics, then take the last path segment.
            depth, plain = 0, ""
            for ch in head:
                depth += ch == "<"
                if depth == 0:
                    plain += ch
                depth -= ch == ">"
            tm = IMPL_TYPE.search(plain.strip())
            name = tm.group(1) if tm else plain.strip()
            end = bisect.bisect_right(line_starts, _body_end(code, off, "impl"))
            items.append(Item("impl", name, idx + 1, end))
    # Parents: the innermost earlier item whose span contains this one.
    stack: list[Item] = []
    for it in items:
        while stack and not (stack[-1].line <= it.line and it.end <= stack[-1].end):
            stack.pop()
        it.parent = stack[-1] if stack else None
        stack.append(it)
    return items


@lru_cache(maxsize=None)
def items_of(path: Path) -> tuple[Item, ...]:
    return tuple(scan(path.read_text(encoding="utf-8", errors="replace")))


def defined_at(path: Path, line: int) -> list[Item]:
    """Named items (not impl blocks) whose head is on `line`."""
    return [it for it in items_of(path) if it.line == line and it.kind != "impl"]


def matches(item: Item, symbol: str) -> bool:
    """Whether `item` is what `symbol` (`name`, `Owner::name` or
    `module::name`) names: the name, and the qualifier when there is one."""
    *quals, name = symbol.split("::")
    if item.kind == "impl" or item.name != name:
        return False
    if not quals:
        return True
    parent = item.parent
    return item.owner == quals[-1] or (parent is not None and parent.kind == "mod" and parent.name == quals[-1])


def find(path: Path, symbol: str) -> list[Item]:
    """Named items in `path` that `symbol` names."""
    return [it for it in items_of(path) if matches(it, symbol)]


def enclosing(path: Path, line: int) -> list[Item]:
    """Items whose span contains `line`, outermost first."""
    return [it for it in items_of(path) if it.line <= line <= it.end]
