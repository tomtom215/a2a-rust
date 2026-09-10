# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""The model: a function, a resolved bound, a site where a bound is applied,
a finding, and a scrubbed source file with its functions indexed."""

from __future__ import annotations

import dataclasses
import pathlib
import re

from . import REPO
from .scrub import blank_cfg_test, blank_noncode, matching_brace, split_args


@dataclasses.dataclass
class Fn:
    name: str
    start: int
    body_open: int
    body_close: int
    params: list[str]  # names of non-self parameters, in order
    param_types: dict[str, str]


@dataclasses.dataclass
class Bound:
    """What a bound expression resolved to."""

    expr: str
    kind: str  # literal | const | knob | deadline | remainder | param | unresolved
    secs: float | None = None
    name: str = ""  # knob or const name
    note: str = ""
    inner: Bound | None = None  # for deadline / remainder

    def ident(self) -> str:
        if self.kind == "knob":
            return self.name
        if self.kind == "const":
            return self.name
        if self.kind == "literal":
            return f"{self.secs:g}s"
        if self.kind in ("deadline", "remainder") and self.inner is not None:
            return f"{self.kind}({self.inner.ident()})"
        return f"?{self.expr}"

    def show(self) -> str:
        if self.kind == "literal":
            return f"{self.secs:g}s literal"
        if self.kind == "const":
            v = f"{self.secs:g}s" if self.secs is not None else "?"
            return f"const {self.name} = {v}"
        if self.kind == "knob":
            v = f"default {self.secs:g}s" if self.secs is not None else self.note or "default unresolved"
            return f"knob {self.name} ({v})"
        if self.kind == "deadline" and self.inner is not None:
            return f"deadline of {self.inner.show()}"
        if self.kind == "remainder" and self.inner is not None:
            return f"remainder of {self.inner.show()}"
        if self.kind == "param":
            return f"parameter {self.name} (resolved at each call site)"
        return f"unresolved `{self.expr}`"

    def comparable(self) -> bool:
        return self.kind in ("literal", "const", "knob") and self.secs is not None


@dataclasses.dataclass
class Site:
    rel: str
    line: int
    offset: int
    kind: str  # timeout | timeout_at | send_timeout | deadline | via <helper> | sleep
    expr: str
    bound: Bound
    fn: Fn | None
    fut: tuple[int, int] | None = None
    note: str = ""

    def where(self) -> str:
        return f"{self.rel}:{self.line}"


@dataclasses.dataclass
class Finding:
    kind: str
    rel: str
    fn: str
    inner: str
    outer: str
    lines: list[int]
    why: str
    detail: list[str]
    reason: str | None = None  # set when justified

    def key(self) -> tuple[str, str, str, str, str]:
        return (self.rel, self.fn, self.kind, self.inner, self.outer)


class SourceFile:
    def __init__(self, path: pathlib.Path):
        self.path = path
        self.rel = path.relative_to(REPO).as_posix()
        self.raw = path.read_text(encoding="utf-8")
        self.code = blank_cfg_test(blank_noncode(self.raw))
        self.fns = self._index_fns()
        self.crate = path.relative_to(REPO).parts[1]

    def line_of(self, offset: int) -> int:
        return self.raw.count("\n", 0, offset) + 1

    def _index_fns(self) -> list[Fn]:
        fns: list[Fn] = []
        for m in re.finditer(r"\bfn\s+([A-Za-z_]\w*)\s*(?:<[^>]*>)?\s*\(", self.code):
            popen = m.end() - 1
            spans, pclose = split_args(self.code, popen)
            params, types = [], {}
            for a, b in spans:
                p = self.code[a:b].strip()
                if p.startswith("&") and p.lstrip("&").split()[-1] == "self" or p in ("self", "mut self"):
                    continue
                if ":" in p:
                    name, ty = p.split(":", 1)
                    name = name.replace("mut ", "").strip()
                    params.append(name)
                    types[name] = ty.strip()
            j, depth = pclose + 1, 0
            body_open = -1
            while j < len(self.code):
                ch = self.code[j]
                if ch in "([":
                    depth += 1
                elif ch in ")]":
                    depth -= 1
                elif ch == ";" and depth == 0:
                    break
                elif ch == "{" and depth == 0:
                    body_open = j
                    break
                j += 1
            if body_open < 0:
                continue
            fns.append(Fn(m.group(1), m.start(), body_open, matching_brace(self.code, body_open), params, types))
        return fns

    def fn_at(self, offset: int) -> Fn | None:
        best = None
        for f in self.fns:
            if f.body_open <= offset <= f.body_close and (
                best is None or (f.body_close - f.body_open) < (best.body_close - best.body_open)
            ):
                best = f
        return best
