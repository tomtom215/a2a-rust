#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Rewrite Rust struct literals of selected types to `Type::new(..).with_x(..)` chains.

Why this exists: marking a struct `#[non_exhaustive]` forbids writing it as a
literal (and as a functional-update expression) outside its crate. Every caller
must move to constructors. Doing that by hand over ~700 sites is how a
regression ships; doing it by `sed` cannot tell a literal from a pattern, a
definition or a string. This scans Rust the way the compiler tokenises it
(strings, raw strings, chars, comments, nested block comments), so it only
touches code.

What it rewrites
    Type { a: x, b: Some(y), c: None }      -> Type::new(x).with_b(y)
    Type { a: x, b: expr }  (b optional, expr neither `Some(..)` nor `None`)
                                            -> { let mut built = Type::new(x); built.b = expr; built }
    Type { a: x, ..base }                   -> { let mut built = base; built.a = x; built }
    pattern  Type { a, b }                  -> Type { a, b, .. }

What it leaves, and reports
    a literal containing a comment, or whose fields it cannot split.

The rewrite is not formatted: run rustfmt afterwards.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field


@dataclass
class FieldSpec:
    name: str
    kind: str  # "req" positional in new; "opt" Option<T> -> with_x(T); "coll" Vec/Map -> with_x(v), omitted when empty;
    #            "val" plain field `new` defaults -> with_x(v) always, except for the spelled-out defaults in skip_values
    setter: str = ""
    skip_values: tuple = ()
    string: bool = False  # the constructor or setter takes `impl Into<String>`

    def __post_init__(self) -> None:
        if not self.setter and self.kind != "req":
            self.setter = "with_" + self.name


@dataclass
class TypeSpec:
    name: str
    fields: list[FieldSpec]
    default_base: bool = False  # the type has Default, and `new()` is that default

    def req(self) -> list[FieldSpec]:
        return [f for f in self.fields if f.kind == "req"]

    def by_name(self) -> dict[str, FieldSpec]:
        return {f.name: f for f in self.fields}


# ── Lexical masking ─────────────────────────────────────────────────────────

def mask(src: str, strings: bool = True) -> str:
    """Same length as src, with string/char contents and comments replaced by spaces
    (newlines kept), so regexes and bracket matching only see code. With strings=False only
    comments are blanked, so a `//` inside a URL string is not mistaken for a comment."""
    out = list(src)
    i, n = 0, len(src)

    def blank(a: int, b: int) -> None:
        for k in range(a, b):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        c = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif c == "r" and re.match(r'r#*"', src[i:i + 40]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            m = re.match(r'r(#*)"', src[i:])
            assert m
            hashes = m.group(1)
            end = src.find('"' + hashes, i + len(m.group(0)))
            end = n if end < 0 else end + 1 + len(hashes)
            if strings:
                blank(i + len(m.group(0)), max(i + len(m.group(0)), end - 1 - len(hashes)))
            i = end
        elif c == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            if strings:
                blank(i + 1, j)
            i = j + 1
        elif c == "'":
            # a char literal ('a', '\n', '\u{..}'), not a lifetime ('a followed by a non-quote)
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:])
            if m:
                if strings:
                    blank(i + 1, i + len(m.group(0)) - 1)
                i += len(m.group(0))
            else:
                i += 1
        else:
            i += 1
    return "".join(out)


def match_close(masked: str, open_idx: int) -> int:
    """Index of the bracket closing the one at open_idx, or -1."""
    pairs = {"{": "}", "(": ")", "[": "]"}
    stack = [pairs[masked[open_idx]]]
    i = open_idx + 1
    while i < len(masked):
        c = masked[i]
        if c in pairs:
            stack.append(pairs[c])
        elif c in ")]}":
            if not stack or c != stack.pop():
                return -1
            if not stack:
                return i
        i += 1
    return -1


# ── Splitting a literal's body into fields ──────────────────────────────────

_FIELD_START = re.compile(r"\s*([A-Za-z_][A-Za-z0-9_]*)\s*(:(?!:)|,|$)")


def split_fields(masked_body: str, body: str) -> list[tuple[str, str]] | None:
    """[(name, value)] for `a: x, b, ..base`; `..base` is ('..', base). None if unsplittable."""
    parts: list[str] = []
    depth, start = 0, 0
    for i, c in enumerate(masked_body):
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append(body[start:i])
            start = i + 1
    tail = body[start:]
    if tail.strip():
        parts.append(tail)
    # A comma inside `|a, b|` or a turbofish looks like a field boundary; rejoin
    # pieces that do not start like a field.
    merged: list[str] = []
    for p in parts:
        if merged and not (p.strip().startswith("..") or _FIELD_START.match(p + ",")):
            merged[-1] += "," + p
        else:
            merged.append(p)
    out: list[tuple[str, str]] = []
    for p in merged:
        s = p.strip()
        if s.startswith(".."):
            out.append(("..", s[2:].strip()))
            continue
        m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\s*:(?!:)\s*(.*)\Z", s, re.S)
        if m:
            out.append((m.group(1), m.group(2).strip()))
        elif re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", s):
            out.append((s, s))  # shorthand `tenant`
        else:
            return None
    return out


def strip_into(v: str) -> str:
    """`x.into()` -> `x` for a value bound to an `impl Into<String>` parameter, where the `.into()`
    leaves the target type to inference and the compiler cannot choose one."""
    if v.endswith(".into()"):
        head = v[: -len(".into()")]
        if head and match_close(mask("(" + head + ")"), 0) == len(head) + 1:
            return head
    return v


def peel_some(v: str) -> str | None:
    m = re.fullmatch(r"Some\((.*)\)", v, re.S)
    if not m:
        return None
    inner = m.group(1)
    return inner if match_close(mask("(" + inner + ")"), 0) == len(inner) + 1 else None


_EMPTY = {"None", "vec![]", "Vec::new()", "HashMap::new()", "Default::default()", "std::collections::HashMap::new()",
          "BTreeMap::new()", "String::new()"}


# ── The rewrite ─────────────────────────────────────────────────────────────

@dataclass
class Result:
    text: str
    converted: int = 0
    patterns: int = 0
    skipped: list[str] = field(default_factory=list)


_DEF_PREV = {"struct", "enum", "impl", "for", "type", "trait", "union", "->", "dyn", "as", "mod", "use", "where"}


def prev_token(masked: str, idx: int) -> str:
    j = idx
    while j > 0 and masked[j - 1].isspace():
        j -= 1
    m = re.search(r"(->|[A-Za-z_][A-Za-z0-9_]*|\S)\Z", masked[:j])
    return m.group(1) if m else ""


def next_token(masked: str, idx: int) -> str:
    m = re.match(r"\s*(=>|==|=|\||:|if\b|[A-Za-z_][A-Za-z0-9_]*|\S)", masked[idx:])
    return m.group(1) if m else ""


def convert_once(src: str, specs: dict[str, TypeSpec], wrap: int = 0) -> Result:
    res = Result(src)
    names = "|".join(sorted(specs, key=len, reverse=True))
    rx = re.compile(r"(?<![A-Za-z0-9_:])(?:[A-Za-z_][A-Za-z0-9_]*::)*(" + names + r")\s*\{")
    edits: list[tuple[int, int, str]] = []
    masked = mask(src)
    pos = 0
    while True:
        m = rx.search(masked, pos)
        if not m:
            break
        tname = m.group(1)
        spec = specs[tname]
        open_idx = m.end() - 1
        close_idx = match_close(masked, open_idx)
        pos = m.end()
        if close_idx < 0:
            res.skipped.append(f"{tname}: unbalanced at offset {m.start()}")
            continue
        prev = prev_token(masked, m.start())
        if prev in _DEF_PREV:
            continue
        # The prost types in `proto`/`pb` share these names and are not the types being changed.
        if set(src[m.start():m.start(1)].rstrip(":").split("::")) & {"pb", "proto"}:
            pos = close_idx + 1
            continue
        body, mbody = src[open_idx + 1:close_idx], masked[open_idx + 1:close_idx]
        nxt = next_token(masked, close_idx + 1)
        is_pattern = (
            prev in ("let",) or nxt in ("=>", "|") or (nxt == "=" and not masked[close_idx + 1:].lstrip().startswith("=="))
            or nxt == "if" or re.search(r"(^|,)\s*\.\.\s*$", mbody) is not None
            or (prev == "(" and nxt == ":")
        )
        path_prefix = src[m.start():m.start(1)]
        if is_pattern:
            if not re.search(r"\.\.\s*$", mbody):
                end = len(body.rstrip())
                new_body = (body[:end].rstrip().rstrip(",") + ", .." + body[end:]) if body.strip() else " .. "
                edits.append((open_idx + 1, close_idx, new_body))
                res.patterns += 1
            pos = close_idx + 1
            continue
        if mask(src[open_idx:close_idx], strings=False) != src[open_idx:close_idx]:
            res.skipped.append(f"{tname}: contains a comment at offset {m.start()}")
            pos = close_idx + 1
            continue
        fields = split_fields(mbody, body)
        if fields is None:
            res.skipped.append(f"{tname}: cannot split fields at offset {m.start()}")
            pos = close_idx + 1
            continue
        known = spec.by_name()
        base = None
        given: dict[str, str] = {}
        bad = False
        for fname, val in fields:
            if fname == "..":
                base = val
            elif fname in known:
                given[fname] = val
            else:
                res.skipped.append(f"{tname}: unknown field {fname!r} at offset {m.start()}")
                bad = True
        if bad:
            pos = close_idx + 1
            continue
        if base is None and any(f.name not in given for f in spec.req()):
            res.skipped.append(f"{tname}: a required field is missing at offset {m.start()}")
            pos = close_idx + 1
            continue
        default_base = (
            base is not None and spec.default_base and base in ("Default::default()", f"{tname}::default()")
        )
        if base is not None and not default_base:
            # `Type { .. }` over some other value: a block that starts from that value.
            assigns = "".join(f" built.{k} = {v};" for k, v in given.items())
            repl = f"{{ let mut built = {base};{assigns} built }}"
        else:
            args = ", ".join(strip_into(given[f.name]) if f.string else given[f.name] for f in spec.req())
            chain = ""
            fallback: list[tuple[str, str]] = []
            for f in spec.fields:
                if f.kind == "req" or f.name not in given:
                    continue
                v = given[f.name]
                if v in f.skip_values or (f.kind in ("opt", "coll") and v in _EMPTY):
                    continue
                if f.kind == "opt":
                    inner = peel_some(v)
                    if inner is None:
                        fallback.append((f.name, v))
                    else:
                        chain += f".{f.setter}({strip_into(inner) if f.string else inner})"
                else:
                    chain += f".{f.setter}({strip_into(v) if f.string else v})"
            base_expr = f"{path_prefix}{tname}::new({args})" + chain
            if fallback:
                assigns = "".join(f" built.{k} = {v};" for k, v in fallback)
                repl = f"{{ let mut built = {base_expr};{assigns} built }}"
            else:
                repl = base_expr
        if wrap:
            repl = wrap_chain(src, m.start(), repl, wrap)
        edits.append((m.start(), close_idx + 1, repl))
        res.converted += 1
        pos = close_idx + 1
    # apply from the end so earlier offsets stay valid
    out = src
    for a, b, text in sorted(edits, reverse=True):
        out = out[:a] + text + out[b:]
    res.text = out
    return res


def convert(src: str, specs: dict[str, TypeSpec], max_passes: int = 8, wrap: int = 0) -> Result:
    """convert_once until nothing changes: a literal nested in another is found on the next pass."""
    total = Result(src)
    for _ in range(max_passes):
        r = convert_once(total.text, specs, wrap)
        total.text = r.text
        total.converted += r.converted
        total.patterns += r.patterns
        for item in r.skipped:
            if item not in total.skipped:
                total.skipped.append(item)
        if r.converted == 0 and r.patterns == 0:
            break
    return total


def wrap_chain(src: str, at: int, repl: str, width: int) -> str:
    """Break a `Type::new(..).with_a(..).with_b(..)` chain onto one line per call when it would pass `width`."""
    line_start = src.rfind("\n", 0, at) + 1
    prefix = src[line_start:at]
    if "\n" in repl or len(prefix) + len(repl) <= width or repl.startswith("{"):
        return repl
    indent = len(prefix) - len(prefix.lstrip())
    m = mask(repl)
    parts, depth, start = [], 0, 0
    for i, c in enumerate(m):
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif c == "." and depth == 0 and m.startswith(".with_", i):
            parts.append(repl[start:i])
            start = i
    parts.append(repl[start:])
    if len(parts) < 2:
        return repl
    return parts[0] + "".join("\n" + " " * (indent + 4) + p for p in parts[1:])
