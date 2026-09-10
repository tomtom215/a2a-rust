# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Resolution: what a `<bound>` expression is worth, as far as the text allows.

Each bound found by `sites` is resolved to one of: a `Duration::from_*`
literal, a `const`, a `let` alias in the same function, a parameter (resolved
again at each call site, see the helper discovery in `sites`), or a struct
field ("knob") whose default is read from the field's `impl Default` /
constructor literal. `Instant::now() + X` is a deadline of X, and
`deadline.saturating_duration_since(now)`, `deadline - now` or
`remaining.min(..)` is the remainder of that deadline — a bound derived from
the deadline, which the pairings deliberately never compare against it.

Two of the blind spots recorded in the package docstring live here. A knob's
default is found by field name, so a field that two structs both declare with
different defaults is reported as ambiguous and not compared. And arithmetic
on anything that is not a deadline (`idle.saturating_sub(idle_for)`) is
listed as unresolved and never compared.
"""

from __future__ import annotations

import re

from .entities import Bound, Fn, SourceFile
from .scrub import matching_brace

DUR_LIT = re.compile(
    r"^(?:(?:std|core|tokio)::time::)?Duration::from_(secs|millis|micros|nanos|secs_f64|secs_f32)"
    r"\(\s*([\d_]+(?:\.\d+)?)\s*\)$"
)
SCALE = {"secs": 1.0, "millis": 1e-3, "micros": 1e-6, "nanos": 1e-9, "secs_f64": 1.0, "secs_f32": 1.0}
INSTANT_PLUS = re.compile(r"^(?:(?:tokio|std)::time::)?Instant::now\(\)\s*\+\s*(.+)$", re.S)
IDENT = re.compile(r"^[A-Za-z_]\w*$")
FIELD = re.compile(r"^[\w.()?]+\.([A-Za-z_]\w*)$")
TYPE_ONLY = re.compile(r"^(?:Option<)?(?:std::time::|core::time::)?Duration>?$|^u\d+$|^usize$|^bool$")


class ResolveMixin:
    """The resolution half of `model.Model`; expects `self.by_crate`."""

    by_crate: dict[str, list[SourceFile]]

    def strip(self, expr: str) -> str:
        e = " ".join(expr.split())
        while e.startswith("(") and e.endswith(")") and matching_brace(e, 0, "(", ")") == len(e) - 1:
            e = e[1:-1].strip()
        return e

    def resolve(self, expr: str, sf: SourceFile, fn: Fn | None, before: int, depth: int = 0) -> Bound:
        e = self.strip(expr)
        if depth > 8 or not e:
            return Bound(e, "unresolved")
        m = DUR_LIT.match(e)
        if m:
            return Bound(e, "literal", float(m.group(2).replace("_", "")) * SCALE[m.group(1)])
        if re.match(r"^(?:(?:std|core)::time::)?Duration::ZERO$", e):
            return Bound(e, "literal", 0.0)
        m = INSTANT_PLUS.match(e)
        if m:
            return Bound(e, "deadline", inner=self.resolve(m.group(1), sf, fn, before, depth + 1))
        m = re.match(r"^Some\((.+)\)$", e)
        if m:
            return self.resolve(m.group(1), sf, fn, before, depth + 1)
        # `deadline.saturating_duration_since(now)`, `deadline - now`, `remaining.min(..)`
        m = re.match(r"^([A-Za-z_]\w*)\s*(?:\.saturating_duration_since\(|-\s*(?:tokio::time::)?Instant::now\(\)|\.min\()", e)
        if m:
            base = self.resolve(m.group(1), sf, fn, before, depth + 1)
            if base.kind == "deadline":
                return Bound(e, "remainder", inner=base.inner)
            if base.kind == "remainder":
                return Bound(e, "remainder", inner=base.inner)
            return Bound(e, "unresolved", note=f"arithmetic on {base.show()}")
        if e == "None":
            return Bound(e, "unresolved", note="None")
        if IDENT.match(e):
            if e.isupper():
                return self.resolve_const(e, sf, depth)
            if fn is not None:
                body = sf.code[fn.body_open:before]
                lets = list(re.finditer(rf"\blet\s+(?:mut\s+)?{re.escape(e)}\s*(?::[^=]+?)?=\s*([^;]+);", body))
                if lets:
                    return self.resolve(lets[-1].group(1), sf, fn, fn.body_open + lets[-1].start(), depth + 1)
                somes = list(re.finditer(rf"\bSome\({re.escape(e)}\)\s*=\s*([\w.]+)", body))
                if somes:
                    return self.resolve(somes[-1].group(1), sf, fn, fn.body_open + somes[-1].start(), depth + 1)
                if e in fn.params:
                    return Bound(e, "param", name=e)
                # `match self.first_event_timeout { Some(timeout) if .. => timeout(timeout, ..) }`
                for mm in reversed(list(re.finditer(r"\bmatch\s+([\w.()?]+)\s*\{", body))):
                    if re.search(rf"\bSome\({re.escape(e)}\)", body[mm.end() :]):
                        return self.resolve(mm.group(1), sf, fn, fn.body_open + mm.start(), depth + 1)
            return Bound(e, "unresolved", note="local binding not traced")
        m = FIELD.match(e)
        if m:
            return self.resolve_knob(m.group(1), e, sf, depth)
        return Bound(e, "unresolved")

    def resolve_const(self, name: str, sf: SourceFile, depth: int) -> Bound:
        pat = re.compile(rf"\bconst\s+{name}\s*:[^=]*=\s*([^;]+);")
        for f in [sf] + [x for x in self.by_crate[sf.crate] if x is not sf]:
            m = pat.search(f.code)
            if m:
                inner = self.resolve(m.group(1), f, None, m.start(), depth + 1)
                return Bound(name, "const", inner.secs, name=name, note=f"{f.rel}:{f.line_of(m.start())}")
        return Bound(name, "const", None, name=name, note="definition not found")

    def resolve_knob(self, name: str, expr: str, sf: SourceFile, depth: int) -> Bound:
        """A struct field's default, read from `impl Default` first, then a
        constructor, then any struct literal — same file first, then crate."""
        field = re.compile(rf"(?<![\w.]){name}\s*:\s*([^,\n]+?)\s*,?\s*\n")
        files = [sf] + [x for x in self.by_crate[sf.crate] if x is not sf]
        found: list[tuple[float | None, str]] = []
        nones: list[str] = []

        def scan(f: SourceFile, region: str, base: int) -> None:
            for m in field.finditer(region):
                rhs = m.group(1).strip()
                if TYPE_ONLY.match(rhs) or rhs.startswith("Option<") or rhs.startswith("Vec<"):
                    continue
                where = f"{f.rel}:{f.line_of(base + m.start())}"
                b = self.resolve(rhs, f, f.fn_at(base + m.start()), base + m.start(), depth + 1)
                if b.kind in ("literal", "const", "knob") and b.secs is not None:
                    found.append((b.secs, where))
                elif b.kind in ("literal", "const"):
                    found.append((b.secs, where))
                elif b.note == "None":
                    nones.append(where)

        for selector in ("default", "new", "any"):
            for f in files:
                if selector == "any":
                    scan(f, f.code, 0)
                else:
                    pat = r"\bimpl\s+Default\s+for\s+[^{]+\{" if selector == "default" else r"\bfn\s+new\s*\([^)]*\)[^{;]*\{"
                    for m in re.finditer(pat, f.code):
                        end = matching_brace(f.code, m.end() - 1)
                        scan(f, f.code[m.end() : end], m.end())
                if found or nones:
                    break
            if found or nones:
                break
        vals = sorted({v for v, _ in found if v is not None})
        if len(vals) == 1 and not nones:
            return Bound(expr, "knob", vals[0], name=name, note=f"from {found[0][1]}")
        if len(vals) > 1 or (vals and nones):
            return Bound(expr, "knob", None, name=name, note="ambiguous default: " + ", ".join(f"{v:g}s@{w}" for v, w in found if v is not None) + "".join(f", None@{w}" for w in nones))
        if nones:
            return Bound(expr, "knob", None, name=name, note=f"default None, no bound ({nones[0]})")
        return Bound(expr, "knob", None, name=name, note="default unresolved")
