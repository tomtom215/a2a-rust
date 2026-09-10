# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Three of the four pairings, all within one function of one file:

  nested     a bound lexically inside another bound's future. Inner larger
             than outer is a finding.
  deadline   a bound in a function that earlier set `Instant::now() + X` and
             is not derived from that deadline. Inner larger than X is a
             finding; two distinct knobs are a finding unless a comment block
             or a `build`/`validate` fn mentions both.
  twice      the same knob, const or literal applied to two sequential
             phases of one function (one level of same-file calls is
             followed). That is the 2x shape; a finding.

The fourth, push, is two functions apart and lives in `push`. Every finding
produced here can be justified by a `// timeout-nesting:` comment or an
allowlist line; `report` applies both.
"""

from __future__ import annotations

import dataclasses
import re

from .entities import Bound, Finding, Fn, Site, SourceFile
from .scrub import matching_brace


class PairsMixin:
    """The pairing half of `model.Model`; expects `self.files`, `self.by_crate`
    and `self.sites`."""

    files: list[SourceFile]
    by_crate: dict[str, list[SourceFile]]
    sites: list[Site]

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
