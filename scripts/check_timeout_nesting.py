#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Pairs each timeout with the bound that encloses it, and fails when the inner
one is larger — or the same one is applied twice — with no recorded reason.

What problem this solves
------------------------
Addendum 8 of docs/v0.9.0-post-release-review.md names a class with eight
shipped instances: **a documented, configurable bound that a wider bound
silently preempts.** Two shapes account for all of them:

* *The budget applied twice.* `shutdown_with_timeout(t)` drained to
  `now + t` and then handed the cleanup hook a fresh `t`; measured 2x. The
  discovery fetch bounded headers at 30 s and then the body at 30 s again;
  measured 55 s. The client's JSON-RPC transport had fixed exactly this and
  written down why (`transport/jsonrpc.rs`, "single deadline across header
  fetch AND body read"), and the fix recurred twice more after being written
  down.
* *The inner bound the outer never lets run.* `HttpPushSender` promises three
  attempts at 30 s with backoff — 98 s with the DNS bound — and the handler
  wraps every `send` in `push_delivery_timeout`, 5 s. Measured: one attempt,
  cut at 5.001 s. `max_attempts` and `backoff` were configuration that could
  not take effect.

A fix is not a lesson until something checks for the next instance. This is
that check: B14 in the review's backlog.

What this checks, and what it does not
--------------------------------------
Over every non-test `.rs` file under `crates/*/src` and `bindings/*/src`, with
comments, strings and `#[cfg(test)]` items blanked (a detector that reads doc
comments as code under-reports exactly where the prose is most confident —
Addendum 8 measured that too), it finds every bound:

    tokio::time::timeout(<bound>, <future>)
    tokio::time::timeout_at(<deadline>, <future>)
    <sender>.send_timeout(<value>, <bound>)
    let <name> = tokio::time::Instant::now() + <bound>;
    <helper>(.., <bound>, ..)   — a same-crate fn whose `Duration` parameter
                                  is what it hands to one of the above

and resolves each `<bound>` as far as the text allows: a `Duration::from_*`
literal, a `const`, a `let` alias in the same function, or a struct field
("knob") whose default it reads from the field's `impl Default` / constructor
literal. Then four pairings:

  nested     a bound lexically inside another bound's future. Inner larger
             than outer is a finding.
  deadline   a bound in a function that earlier set `Instant::now() + X` and
             is not derived from that deadline. Inner larger than X is a
             finding; two distinct knobs are a finding unless a comment block
             or a `build`/`validate` fn mentions both.
  twice      the same knob, const or literal applied to two sequential
             phases of one function (one level of same-file calls is
             followed). That is the 2x shape; a finding.
  push       `HttpPushSender`'s worst-case schedule — computed from the same
             constants and `PushRetryPolicy` default the code uses — against
             `HandlerLimits::push_delivery_timeout`. At the shipped defaults
             they contradict (98 s inside 5 s), and the recorded reason is
             *code*, not prose: the sender must report the schedule
             (`max_delivery_duration`) and the deliverer must compare it to
             the bound (`TIMEOUT_TRUNCATED`). Either going missing is the
             contradiction reintroduced, and this pairing takes no allowlist
             entry, because "documented" is what that pair had before and it
             was not enough.

A finding is silenced by a `// timeout-nesting: <reason>` comment within six
lines above the inner bound, or by a line in scripts/timeout_nesting_allowlist.txt
(`path | fn | kind | inner | outer | reason`). An allowlist line nothing matches
is itself a finding, so a fixed defect cannot keep its exemption.

What it cannot see, stated so that a clean run is read for what it is:

* **Bounds across function calls.** The push pairing is hand-modelled
  precisely because the enclosing `timeout` and the enclosed retry loop are
  two functions apart. Any other such pair is invisible until someone adds
  it here.
* **A knob renamed at a boundary, or with several defaults.** The default is
  found by field name; a field that two structs both declare with different
  defaults is reported as ambiguous and not compared.
* **Arithmetic.** `idle.saturating_sub(idle_for)` is a bound this script
  lists as unresolved and never compares.
* **Sleeps.** `tokio::time::sleep` is listed under `--explain` as a timer,
  not paired: keep-alives and backoffs are not budgets.

Usage
-----
    python3 scripts/check_timeout_nesting.py            # the gate
    python3 scripts/check_timeout_nesting.py --explain  # every site, resolved,
                                                        # every pair, its verdict

Exit codes: 0 clean, 1 at least one unjustified pair (or a stale allowlist
line), 2 the tree could not be read.
"""

from __future__ import annotations

import dataclasses
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]
ALLOWLIST = REPO / "scripts" / "timeout_nesting_allowlist.txt"

SENDER_RS = "crates/a2a-protocol-server/src/push/sender.rs"
LIMITS_RS = "crates/a2a-protocol-server/src/handler/limits.rs"

JUSTIFY = re.compile(r"timeout-nesting:\s*(\S.*)")

# ── Source scrubbing ──────────────────────────────────────────────────────────

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


# ── The model ─────────────────────────────────────────────────────────────────


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


DUR_LIT = re.compile(
    r"^(?:(?:std|core|tokio)::time::)?Duration::from_(secs|millis|micros|nanos|secs_f64|secs_f32)"
    r"\(\s*([\d_]+(?:\.\d+)?)\s*\)$"
)
SCALE = {"secs": 1.0, "millis": 1e-3, "micros": 1e-6, "nanos": 1e-9, "secs_f64": 1.0, "secs_f32": 1.0}
INSTANT_PLUS = re.compile(r"^(?:(?:tokio|std)::time::)?Instant::now\(\)\s*\+\s*(.+)$", re.S)
IDENT = re.compile(r"^[A-Za-z_]\w*$")
FIELD = re.compile(r"^[\w.()?]+\.([A-Za-z_]\w*)$")
TYPE_ONLY = re.compile(r"^(?:Option<)?(?:std::time::|core::time::)?Duration>?$|^u\d+$|^usize$|^bool$")


class Model:
    def __init__(self, files: list[SourceFile]):
        self.files = files
        self.by_crate: dict[str, list[SourceFile]] = {}
        for f in files:
            self.by_crate.setdefault(f.crate, []).append(f)
        self.sites: list[Site] = []
        self.helpers: dict[str, tuple[SourceFile, Fn, int]] = {}  # name -> (file, fn, arg index)

    # ── resolution ────────────────────────────────────────────────────────

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

    # ── site discovery ────────────────────────────────────────────────────

    SITE = re.compile(r"(?<![\w:])(?:tokio::)?time::(timeout_at|timeout|sleep)\s*\(|\.(send_timeout)\s*\(")
    DEADLINE = re.compile(r"\blet\s+(?:mut\s+)?([A-Za-z_]\w*)\s*(?::[^=]+?)?=\s*((?:tokio::time::|std::time::)?Instant::now\(\)\s*\+\s*[^;]+);")

    def discover(self) -> None:
        for sf in self.files:
            for m in self.SITE.finditer(sf.code):
                kind = m.group(1) or m.group(2)
                popen = m.end() - 1
                spans, pclose = split_args(sf.code, popen)
                if not spans:
                    continue
                if kind == "send_timeout":
                    if len(spans) < 2:
                        continue
                    a, b = spans[1]
                    fut = None
                else:
                    a, b = spans[0]
                    fut = (spans[1][0], pclose) if len(spans) > 1 else None
                fn = sf.fn_at(m.start())
                expr = sf.code[a:b]
                bound = self.resolve(expr, sf, fn, m.start())
                self.sites.append(Site(sf.rel, sf.line_of(m.start()), m.start(), kind, self.strip(expr), bound, fn, fut))
            for m in self.DEADLINE.finditer(sf.code):
                fn = sf.fn_at(m.start())
                bound = self.resolve(m.group(2), sf, fn, m.start())
                self.sites.append(Site(sf.rel, sf.line_of(m.start()), m.start(), "deadline", m.group(1), bound, fn, note=m.group(1)))
        self._discover_helpers()

    def _discover_helpers(self) -> None:
        """A fn whose `Duration` parameter is what it hands to a timeout site
        is a bound by delegation; each call to it is a site in the caller."""
        # To a fixpoint: a fn that hands its parameter to a helper is a helper too
        # (`validate_webhook_url_with_dns` -> `validate_webhook_url_with_resolver`).
        while True:
            new: dict[str, tuple[SourceFile, Fn, int]] = {}
            for sf in self.files:
                for fn in sf.fns:
                    if fn.name in self.helpers:
                        continue
                    for i, p in enumerate(fn.params):
                        if "Duration" not in fn.param_types.get(p, "") or "Option" in fn.param_types.get(p, ""):
                            continue
                        if any(s.rel == sf.rel and s.fn is fn and s.kind != "sleep" and s.bound.kind == "param" and s.bound.name == p for s in self.sites):
                            new[fn.name] = (sf, fn, i)
            if not new:
                break
            self.helpers.update(new)
            for name, (hf, hfn, idx) in new.items():
                pat = re.compile(rf"(?<![\w])(?:[\w]+::)*{re.escape(name)}\s*\(")
                for sf in self.by_crate[hf.crate]:
                    for m in pat.finditer(sf.code):
                        if sf.code[max(0, m.start() - 3) : m.start()].strip().endswith("fn"):
                            continue
                        spans, _ = split_args(sf.code, m.end() - 1)
                        if len(spans) <= idx:
                            continue
                        a, b = spans[idx]
                        fn = sf.fn_at(m.start())
                        if fn is hfn:
                            continue
                        expr = sf.code[a:b]
                        bound = self.resolve(expr, sf, fn, m.start())
                        self.sites.append(Site(sf.rel, sf.line_of(m.start()), m.start(), f"via {name}", self.strip(expr), bound, fn))
        self.sites.sort(key=lambda s: (s.rel, s.offset))

    # ── pairing ───────────────────────────────────────────────────────────

    def bound_sites(self) -> list[Site]:
        return [s for s in self.sites if s.kind != "sleep" and s.kind != "deadline"]

    def pairs(self) -> tuple[list[Finding], list[str]]:
        findings: list[Finding] = []
        checked: list[str] = []
        by_fn: dict[tuple[str, int], list[Site]] = {}
        for s in self.sites:
            if s.fn is not None and s.kind != "sleep":
                by_fn.setdefault((s.rel, s.fn.start), []).append(s)
        file_by_rel = {f.rel: f for f in self.files}

        for (rel, _), sites in by_fn.items():
            sf = file_by_rel[rel]
            fn = sites[0].fn
            assert fn is not None
            bounds = [s for s in sites if s.kind != "deadline"]
            deadlines = [s for s in sites if s.kind == "deadline"]

            # (a) nested: a bound inside another's future.
            for outer in bounds:
                if outer.fut is None:
                    continue
                for inner in bounds:
                    if inner is outer or not (outer.fut[0] <= inner.offset < outer.fut[1]):
                        continue
                    found, verdict = self._compare(sf, fn, "nested", inner, outer, inner.bound, outer.bound)
                    findings.extend(found)
                    checked.append(f"nested   {inner.where()} {inner.bound.show()}  inside  {outer.where()} {outer.bound.show()}\n           -> {verdict}")

            # (d) deadline: a fixed bound after `let d = Instant::now() + X` in the same fn.
            for d in deadlines:
                if d.bound.kind != "deadline" or d.bound.inner is None:
                    continue
                for inner in bounds:
                    if inner.offset < d.offset or inner.bound.kind in ("remainder", "deadline") or inner.kind == "timeout_at":
                        continue
                    found, verdict = self._compare(sf, fn, "deadline", inner, d, inner.bound, d.bound.inner)
                    findings.extend(found)
                    checked.append(f"deadline {inner.where()} {inner.bound.show()}  inside  {d.where()} {d.bound.show()}\n           -> {verdict}")

            # (c) twice: the same identity applied to two sequential phases.
            seq = list(bounds)
            # one level of same-file calls: the callee's direct bounds count as the caller's.
            body = sf.code[fn.body_open : fn.body_close]
            for callee in sf.fns:
                if callee is fn or not re.search(rf"(?<![\w])(?:self\.|Self::)?{re.escape(callee.name)}\s*\(", body):
                    continue
                for s in self.sites:
                    if s.rel == rel and s.fn is callee and s.kind not in ("sleep", "deadline"):
                        seq.append(dataclasses.replace(s, note=f"reached through {callee.name}()"))
            seq.sort(key=lambda s: s.offset)
            groups: dict[str, list[Site]] = {}
            for s in seq:
                if s.bound.kind in ("literal", "const", "knob"):
                    groups.setdefault(s.bound.ident(), []).append(s)
            for ident, members in groups.items():
                distinct = [m for m in members if not any(o is not m and o.fut and o.fut[0] <= m.offset < o.fut[1] for o in members)]
                phases = self._sequential(sf, distinct)
                if len(phases) < 2:
                    continue
                checked.append(f"twice    {rel} fn {fn.name}: {ident} at lines " + ", ".join(str(p.line) for p in phases) + "\n           -> FINDING unless justified: the same bound on more than one phase")
                lines = [p.line for p in phases]
                total = phases[0].bound.secs
                # "At least 2x": sites in different early-return branches are
                # alternatives this script cannot always tell apart, so the
                # multiplier it can stand behind is two, not the site count.
                worst = f" — the call can take at least 2x = {total * 2:g}s" if total is not None else " — the call can take at least 2x the bound"
                findings.append(
                    Finding(
                        "twice", rel, fn.name, ident, ident, lines,
                        f"`{ident}` bounds one phase and then a later phase of the same call again{worst}; the documented bound is per phase, not per call",
                        [f"{p.where()}  {p.kind}({p.expr})" + (f"  [{p.note}]" if p.note else "") for p in phases],
                    )
                )
        return findings, checked

    def _sequential(self, sf: SourceFile, sites: list[Site]) -> list[Site]:
        """Drop sites that are alternatives of an earlier one (else-branches,
        sibling match arms) rather than phases after it."""
        kept: list[Site] = []
        for s in sites:
            if any(self._alternatives(sf, k, s) for k in kept):
                continue
            kept.append(s)
        return kept

    def _alternatives(self, sf: SourceFile, a: Site, b: Site) -> bool:
        code = sf.code

        def block_excluding(inside: int, exclude: int) -> tuple[int, int] | None:
            best = None
            for m in re.finditer(r"\{", code[: inside + 1]):
                close = matching_brace(code, m.start())
                if m.start() <= inside <= close and not (m.start() <= exclude <= close):
                    if best is None or m.start() > best[0]:
                        best = (m.start(), close)
            return best

        ba, bb = block_excluding(a.offset, b.offset), block_excluding(b.offset, a.offset)
        if not ba or not bb or ba[1] > bb[0]:
            return False
        between = code[ba[1] + 1 : bb[0]]
        if re.match(r"^\s*else(\s+if\b[^{]*)?\s*$", between):
            return True
        if "=>" in between and "{" not in between and "}" not in between and ";" not in between:
            return True
        return False

    def _compare(self, sf: SourceFile, fn: Fn, kind: str, inner: Site, outer: Site, ib: Bound, ob: Bound) -> tuple[list[Finding], str]:
        """Findings for one enclosure, and a one-line verdict for --explain."""
        if ib.kind in ("remainder", "param", "unresolved") or ob.kind in ("param", "unresolved"):
            return [], "not compared: " + ("inner is " + ib.kind if ib.kind in ("remainder", "param", "unresolved") else "outer is " + ob.kind)
        detail = [f"inner {inner.where()}  {inner.kind}({inner.expr}) = {ib.show()}", f"outer {outer.where()}  {outer.kind}({outer.expr}) = {ob.show()}"]
        verdict = []
        if ib.comparable() and ob.comparable():
            if ib.secs > ob.secs:
                return [Finding(kind, sf.rel, fn.name, ib.ident(), ob.ident(), [inner.line, outer.line], f"inner {ib.secs:g}s > outer {ob.secs:g}s: the outer bound fires first and the inner one is inert", detail)], f"FINDING: {ib.secs:g}s inside {ob.secs:g}s"
            verdict.append(f"{ib.secs:g}s fits in {ob.secs:g}s at the defaults")
        if ib.kind == "knob" and ob.kind == "knob" and ib.name != ob.name:
            why = self.pair_recorded(sf, ib.name, ob.name)
            if why:
                verdict.append(why)
            else:
                return [Finding(kind, sf.rel, fn.name, ib.ident(), ob.ident(), [inner.line, outer.line], f"two knobs, one inside the other, and nothing documents or validates that `{ib.name}` fits in `{ob.name}`", detail)], "FINDING: two knobs, relationship not recorded"
        return [], "; ".join(verdict) or "nothing to compare"

    def pair_recorded(self, sf: SourceFile, a: str, b: str) -> str | None:
        """A comment block or a build/validate fn that names both knobs."""
        for f in self.by_crate[sf.crate]:
            for m in re.finditer(r"(?:^[ \t]*//[/!]?[^\n]*\n)+", f.raw, re.M):
                if re.search(rf"\b{a}\b", m.group(0)) and re.search(rf"\b{b}\b", m.group(0)):
                    return f"documented together at {f.rel}:{f.line_of(m.start())}"
            for fn in f.fns:
                if fn.name in ("build", "validate", "try_build", "check") or fn.name.startswith("validate_"):
                    body = f.code[fn.body_open : fn.body_close]
                    if re.search(rf"\b{a}\b", body) and re.search(rf"\b{b}\b", body):
                        return f"checked in {f.rel} fn {fn.name}"
        return None

    # ── the push pairing ──────────────────────────────────────────────────

    def push_pair(self) -> tuple[list[Finding], list[str]]:
        notes: list[str] = []
        by_rel = {f.rel: f for f in self.files}
        sender, limits = by_rel.get(SENDER_RS), by_rel.get(LIMITS_RS)
        if sender is None or limits is None:
            return [Finding("push", SENDER_RS, "-", "HttpPushSender", "push_delivery_timeout", [], "sender.rs or limits.rs not found; the push pairing cannot be modelled", [])], notes

        def const(name: str) -> Bound:
            return self.resolve_const(name, sender, 0)

        req, dns = const("DEFAULT_PUSH_REQUEST_TIMEOUT"), const("DEFAULT_DNS_LOOKUP_TIMEOUT")
        m = re.search(r"\bimpl\s+Default\s+for\s+PushRetryPolicy\s*\{", sender.code)
        attempts, backoff = None, None
        if m:
            body = sender.code[m.end() : matching_brace(sender.code, m.end() - 1)]
            a = re.search(r"\bmax_attempts\s*:\s*(\d+)", body)
            b = re.search(r"\bbackoff\s*:\s*vec!\[([^\]]*)\]", body)
            attempts = int(a.group(1)) if a else None
            if b:
                backoff = [self.resolve(x, sender, None, 0).secs for x in b.group(1).split(",") if x.strip()]
        outer = self.resolve_knob("push_delivery_timeout", "HandlerLimits::push_delivery_timeout", limits, 0)
        if req.secs is None or dns.secs is None or attempts is None or backoff is None or None in backoff or outer.secs is None:
            return [Finding("push", SENDER_RS, "-", "HttpPushSender", "push_delivery_timeout", [], "could not read the sender's constants, PushRetryPolicy's default, or HandlerLimits' default — the model no longer matches the code; update it", [f"request={req.show()} dns={dns.show()} attempts={attempts} backoff={backoff} outer={outer.show()}"])], notes
        sched = dns.secs + req.secs * attempts
        waits = [backoff[i] if i < len(backoff) else backoff[-1] for i in range(attempts - 1)] if backoff else []
        sched += sum(waits)
        notes.append(
            f"push     HttpPushSender::send worst case = dns {dns.secs:g}s + {attempts} x request {req.secs:g}s + backoff {'+'.join(f'{w:g}' for w in waits) or '0'}s = {sched:g}s;"
            f" enclosed by HandlerLimits::push_delivery_timeout default {outer.secs:g}s"
        )
        if sched <= outer.secs:
            notes.append("push     fits: no justification needed")
            return [], notes
        # The contradiction stands. The recorded reason must be code.
        problems: list[str] = []
        m = re.search(r"\bimpl\s+PushSender\s+for\s+HttpPushSender\s*\{", sender.code)
        reports = bool(m) and re.search(r"\bfn\s+max_delivery_duration\s*\(", sender.code[m.end() : matching_brace(sender.code, m.end() - 1)]) is not None
        if not reports:
            problems.append(f"{SENDER_RS}: `impl PushSender for HttpPushSender` does not override `max_delivery_duration`, so the schedule is not reported")
        compared = None
        for f in self.by_crate[sender.crate]:
            if not any(s.rel == f.rel and s.kind == "timeout" and s.bound.name == "push_delivery_timeout" for s in self.sites):
                continue
            c = re.search(r"max_delivery_duration\(\)[^;]{0,200}\bpush_delivery_timeout\b", f.code)
            if c:
                compared = f"{f.rel}:{f.line_of(c.start())}"
        if compared is None:
            problems.append("no file that bounds `send` with `push_delivery_timeout` compares `max_delivery_duration()` against it, so a truncated schedule is reported as a slow webhook")
        detail = [
            f"inner {SENDER_RS}  HttpPushSender schedule = {sched:g}s (dns {dns.secs:g}s + {attempts} x {req.secs:g}s + backoff {sum(waits):g}s)",
            f"outer {LIMITS_RS}  HandlerLimits::push_delivery_timeout default = {outer.secs:g}s",
        ] + problems
        if problems:
            return [Finding("push", SENDER_RS, "HttpPushSender::send", "HttpPushSender schedule", "push_delivery_timeout", [], f"push_delivery_timeout / HttpPushSender contradiction ({sched:g}s inside {outer.secs:g}s) with the truncation no longer reported", detail)], notes
        notes.append(f"push     justified in code: the sender reports max_delivery_duration and {compared} compares it to push_delivery_timeout (TIMEOUT_TRUNCATED)")
        return [], notes


# ── Justifications ────────────────────────────────────────────────────────────


def inline_reason(files: dict[str, SourceFile], f: Finding) -> str | None:
    sf = files.get(f.rel)
    if sf is None or not f.lines:
        return None
    lines = sf.raw.splitlines()
    for ln in f.lines:
        for k in range(max(0, ln - 7), ln):
            m = JUSTIFY.search(lines[k]) if k < len(lines) else None
            if m:
                return f"{f.rel}:{k + 1}: {m.group(1).strip()}"
    return None


def load_allowlist() -> tuple[dict[tuple[str, str, str, str, str], str], list[str]]:
    entries: dict[tuple[str, str, str, str, str], str] = {}
    bad: list[str] = []
    if not ALLOWLIST.exists():
        return entries, bad
    for n, line in enumerate(ALLOWLIST.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = [p.strip() for p in line.split("|")]
        if len(parts) != 6 or not parts[5]:
            bad.append(f"{ALLOWLIST.relative_to(REPO)}:{n}: expected `path | fn | kind | inner | outer | reason`")
            continue
        entries[tuple(parts[:5])] = parts[5]  # type: ignore[assignment]
    return entries, bad


# ── Main ──────────────────────────────────────────────────────────────────────


def sources() -> list[pathlib.Path]:
    out = []
    for pattern in ("crates/*/src/**/*.rs", "bindings/*/src/**/*.rs"):
        for p in REPO.glob(pattern):
            rel = p.relative_to(REPO).as_posix()
            if "/tests/" in rel or rel.endswith("tests.rs") or rel.endswith("_tests.rs"):
                continue
            out.append(p)
    return sorted(out)


def main(argv: list[str]) -> int:
    explain = "--explain" in argv
    if not (REPO / "Cargo.toml").exists():
        print("check_timeout_nesting: run me from a checkout with Cargo.toml at the root", file=sys.stderr)
        return 2
    paths = sources()
    if not paths:
        print(f"check_timeout_nesting: no sources under {REPO}", file=sys.stderr)
        return 2
    try:
        files = [SourceFile(p) for p in paths]
    except (OSError, UnicodeDecodeError) as e:
        print(f"check_timeout_nesting: cannot read the tree: {e}", file=sys.stderr)
        return 2

    model = Model(files)
    model.discover()
    findings, checked = model.pairs()
    push_findings, push_notes = model.push_pair()
    findings.extend(push_findings)

    by_rel = {f.rel: f for f in files}
    allow, bad_lines = load_allowlist()
    used: set[tuple[str, str, str, str, str]] = set()
    open_findings: list[Finding] = []
    justified: list[Finding] = []
    for f in findings:
        if f.kind != "push":
            r = inline_reason(by_rel, f)
            if r is None and f.key() in allow:
                r = f"allowlist: {allow[f.key()]}"
                used.add(f.key())
            if r is not None:
                f.reason = r
                justified.append(f)
                continue
        open_findings.append(f)
    stale = [k for k in allow if k not in used]

    n_timeout = sum(1 for s in model.sites if s.kind == "timeout")
    n_at = sum(1 for s in model.sites if s.kind == "timeout_at")
    n_send = sum(1 for s in model.sites if s.kind == "send_timeout")
    n_deadline = sum(1 for s in model.sites if s.kind == "deadline")
    n_via = sum(1 for s in model.sites if s.kind.startswith("via "))
    n_sleep = sum(1 for s in model.sites if s.kind == "sleep")

    if explain:
        print("check_timeout_nesting --explain\n")
        print("Sites (non-test; comments, strings and #[cfg(test)] items blanked):")
        for s in model.sites:
            fn = s.fn.name if s.fn else "-"
            tag = "timer, not paired" if s.kind == "sleep" else s.bound.show()
            extra = f"  [{s.note}]" if s.note and s.kind != "deadline" else ""
            print(f"  {s.where():<76} fn {fn:<34} {s.kind:<28} {s.expr:<44} -> {tag}{extra}")
        print(
            f"\nCounts: {n_timeout} tokio::time::timeout, {n_at} timeout_at, {n_send} send_timeout, "
            f"{n_deadline} `Instant::now() +` deadlines, {n_via} bounds passed to a helper, {n_sleep} sleeps (timers)."
        )
        print(
            "The review enumerated 24 non-test `tokio::time::timeout` sites; the first count above is\n"
            "the same census. Since then discovery, JWKS, and the REST/JSON-RPC unary paths moved\n"
            "to `timeout_at` on one deadline — those are the timeout_at count, and are the fixed shape."
        )
        if model.helpers:
            print("\nHelpers (a `Duration` parameter that becomes a bound inside):")
            for name, (hf, hfn, idx) in sorted(model.helpers.items()):
                print(f"  {name}(): {hf.rel} parameter #{idx} `{hfn.params[idx]}`")
        print("\nPairs checked:")
        for c in checked:
            print(f"  {c}")
        for c in push_notes:
            print(f"  {c}")
        print("\nVerdicts:")
        for f in justified:
            print(f"  justified  {f.rel} fn {f.fn} [{f.kind}] {f.inner} in {f.outer}\n             {f.reason}")
        for f in open_findings:
            print(f"  FINDING    {f.rel} fn {f.fn} [{f.kind}] {f.inner} in {f.outer}")
        print()

    problems = bool(open_findings or stale or bad_lines)
    if problems:
        out = sys.stderr
        if open_findings:
            print(f"check_timeout_nesting: {len(open_findings)} bound(s) preempted by an enclosing bound with no recorded reason:\n", file=out)
            for f in open_findings:
                where = f"{f.rel}:{','.join(str(n) for n in f.lines)}" if f.lines else f.rel
                print(f"  {where}  fn {f.fn}  [{f.kind}]", file=out)
                print(f"      {f.why}", file=out)
                for d in f.detail:
                    print(f"      {d}", file=out)
                print(file=out)
        for k in stale:
            print(f"  stale allowlist line (nothing matches it any more; delete it): {' | '.join(k)}", file=out)
        for b in bad_lines:
            print(f"  {b}", file=out)
        print(
            "\nAn inner bound larger than the one enclosing it never fires: the outer bound's\n"
            "cleanup and reporting path is what actually runs, and the inner number is\n"
            "configuration that cannot take effect. A budget applied to each phase of a call\n"
            "is a call that can take N x the budget.\n"
            "\nFix: give the whole call one deadline — `let deadline = Instant::now() + budget;`\n"
            "then `timeout_at(deadline, ..)` or the remainder for each phase — as\n"
            "crates/a2a-protocol-client/src/transport/jsonrpc.rs::execute_request does. If the\n"
            "shape is deliberate, say why in a `// timeout-nesting: <reason>` comment above the\n"
            "inner bound, or in scripts/timeout_nesting_allowlist.txt. The push pairing takes\n"
            "neither: its reason is the sender reporting its schedule and the deliverer\n"
            "comparing it (see HttpPushSender::max_delivery_duration and push_outcome::TIMEOUT_TRUNCATED).",
            file=out,
        )
        return 1

    print(
        f"check_timeout_nesting: {n_timeout + n_at + n_send} timeout site(s) + {n_deadline} deadline(s) + "
        f"{n_via} delegated bound(s); {len(checked) + len(push_notes)} pairing(s) checked, "
        f"{len(justified)} justified."
    )
    for f in justified:
        print(f"  justified: {f.rel} fn {f.fn} [{f.kind}] {f.inner} in {f.outer} — {f.reason}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
