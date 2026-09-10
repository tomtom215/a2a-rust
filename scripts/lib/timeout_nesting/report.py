# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Justifications and the verdict: `main` for the entry point.

A finding is silenced by a `// timeout-nesting: <reason>` comment within six
lines above the inner bound, or by a line in scripts/timeout_nesting_allowlist.txt
(`path | fn | kind | inner | outer | reason`). An allowlist line nothing matches
is itself a finding, so a fixed defect cannot keep its exemption. The push
pairing takes neither (see `push`).

Usage
-----
    python3 scripts/check_timeout_nesting.py            # the gate
    python3 scripts/check_timeout_nesting.py --explain  # every site, resolved,
                                                        # every pair, its verdict

Exit codes: 0 clean, 1 at least one unjustified pair (or a stale allowlist
line), 2 the tree could not be read.
"""

from __future__ import annotations

import re
import sys

from . import ALLOWLIST, REPO
from .entities import Finding, SourceFile
from .model import Model
from .sites import sources

JUSTIFY = re.compile(r"timeout-nesting:\s*(\S.*)")


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
