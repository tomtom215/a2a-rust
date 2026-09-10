# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Source scrubbing: comments, strings and `#[cfg(test)]` items blanked, plus
the brace and argument matching every later stage leans on.

A detector that reads doc comments as code under-reports exactly where the
prose is most confident — Addendum 8 of docs/v0.9.0-post-release-review.md
measured that too — so the whole tree is scrubbed before any site is looked
for. Every function here preserves lengths and newlines, so an offset into
the scrubbed text still points at the same place in the real file.
"""

from __future__ import annotations

import re

CFG_TEST = re.compile(r"#\[cfg\((?:all\()?test\b[^\]]*\]")


def blank_noncode(src: str) -> str:
    """`src` with comments and string/char literals turned to spaces.

    Lengths and newlines are preserved so every offset still points at the
    real file. Blanking strings matters more than it looks: `format!("{e}")`
    contains braces, and brace matching over the raw text would pair them.
    """
    out = list(src)
    i, n = 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        two = src[i : i + 2]
        if two == "//":
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif two == "/*":
            depth, j = 1, i + 2
            while j < n and depth:
                if src[j : j + 2] == "/*":
                    depth, j = depth + 1, j + 2
                elif src[j : j + 2] == "*/":
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == '"' or (
            c in "rb"
            and re.match(r'b?r#*"', src[i:])
            and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_"))
        ) or (c == "b" and src[i : i + 2] == 'b"'):
            m = re.match(r'b?(r)?(#*)"', src[i:])
            assert m is not None
            raw, hashes = m.group(1), m.group(2)
            j = i + m.end()
            if raw:
                close = '"' + hashes
                k = src.find(close, j)
                k = n if k < 0 else k + len(close)
            else:
                k = j
                while k < n and src[k] != '"':
                    k += 2 if src[k] == "\\" else 1
                k = min(k + 1, n)
            blank(i, k)
            i = k
        elif c == "'":
            m = re.match(r"'(?:\\(?:u\{[0-9a-fA-F]+\}|.)|[^\\'\n])'", src[i:])
            if m:
                blank(i, i + m.end())
                i += m.end()
            else:
                i += 1  # a lifetime
        else:
            i += 1
    return "".join(out)


def matching_brace(text: str, open_at: int, opener: str = "{", closer: str = "}") -> int:
    """Index of the `closer` matching the `opener` at `open_at`, or len(text)."""
    depth, i = 0, open_at
    while i < len(text):
        if text[i] == opener:
            depth += 1
        elif text[i] == closer:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return len(text)


def blank_cfg_test(text: str) -> str:
    """Blank every `#[cfg(test)]` item — inline `mod tests { … }`, a
    test-only fn, a test-only `use`."""
    out = text
    for m in CFG_TEST.finditer(text):
        j = m.end()
        depth = 0
        while j < len(out):
            ch = out[j]
            if ch in "([":
                depth += 1
            elif ch in ")]":
                depth -= 1
            elif ch == ";" and depth == 0:
                break
            elif ch == "{" and depth == 0:
                j = matching_brace(out, j)
                break
            j += 1
        span = out[m.start() : j + 1]
        out = out[: m.start()] + re.sub(r"[^\n]", " ", span) + out[j + 1 :]
    return out


def split_args(text: str, open_paren: int) -> tuple[list[tuple[int, int]], int]:
    """Top-level argument spans of the call whose `(` is at `open_paren`,
    and the index of its `)`."""
    spans: list[tuple[int, int]] = []
    depth, i, start = 0, open_paren + 1, open_paren + 1
    while i < len(text):
        ch = text[i]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                if text[start:i].strip():
                    spans.append((start, i))
                return spans, i
            depth -= 1
        elif ch == "," and depth == 0:
            spans.append((start, i))
            start = i + 1
        i += 1
    return spans, len(text)
