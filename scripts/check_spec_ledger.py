#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Keeps docs/compliance/spec-ledger.json honest against the specification.

The ledger records, for every RFC 2119 statement in A2A's specification, what
this repository does about it and which test says so. A ledger like that rots
in two ways nobody notices: the specification gains a requirement and the
ledger never hears of it, or a test is renamed and the ledger goes on citing
evidence that no longer exists. This check fails on both.

It checks, against the vendored specification
(`docs/implementation/a2a-v1.0.1-specification.md`, the v1.0.1 release) and
the vendored proto:

0. The vendored specification's text hashes to the ledger's `spec.sha256`.
   Every `spec_line` is a line number in that exact text, so a refreshed
   copy without re-anchored entries fails here, before the line checks
   below can pass by coincidence on moved text.

1. Every specification line carrying MUST, SHOULD, REQUIRED, SHALL,
   RECOMMENDED or MAY has at least one entry. A line that only introduces a
   list ("Clients MUST follow these rules:") is covered by entries on the list
   items beneath it, which is where the requirements are.
2. Every entry's `spec_line` still names a line with a keyword on it, so a
   refreshed specification that moves text shows up as stale entries rather
   than as entries quietly pointing at the wrong sentence.
3. Every citation names what it points at, and the line it gives is where
   that thing is (since 2026-10-23; scripts/lib/spec_ledger/citations.py has
   the forms in full). Rust implementation evidence is `path:line Symbol`,
   the line of the item's definition; anything else is `path:line
   "fragment"`, the fragment on that line; test evidence is `path:line
   name`, the line of `fn name`. Before, a bare `path:line` was checked only
   to be inside its file and a symbol only to appear somewhere in it, so 419
   citations sat on comments, blank lines and braces, and any edit above a
   cited line moved it silently (2026-10-20: one had pointed past the end of
   a 444-line file for six days). `--fix-lines` moves line numbers whose
   target is found exactly once and lists the rest.
4. `status` and `severity` use the ledger's vocabulary, an open status has a
   severity, and a closed one does not.
5. Entries under `after_release` (requirements upstream added after the
   release the ledger audits) meet 3 and 4, and cite no spec line: their text
   is not in the vendored copy.

It does not, and cannot, check that a test *proves* what the entry says. That
judgement is in the entry's notes and in review.

`--write` regenerates docs/compliance/spec-ledger.md, the readable summary,
from the JSON; the default `--check` also fails when that file is stale, so
the page a reviewer reads cannot drift from the data it summarises.
`--self-test` proves each citation rule refuses what it should, offline.

Exit 0 when all hold, 1 otherwise, 2 when an input is missing or unreadable.
"""

from __future__ import annotations

import hashlib
import json
import re
import sys
import argparse
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))

from spec_ledger import citations  # noqa: E402
from spec_ledger.selftest import self_test  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs/compliance/spec-ledger.json"
SPEC = ROOT / "docs/implementation/a2a-v1.0.1-specification.md"
PROTO = ROOT / "proto/a2a_v1/a2a.proto"
SUMMARY = ROOT / "docs/compliance/spec-ledger.md"
SEVERITY_ORDER = {"high": 0, "medium": 1, "low": 2}

KEYWORD = re.compile(r"\b(MUST|SHOULD|REQUIRED|SHALL|RECOMMENDED|MAY)\b")
OPEN = {"GAP", "PARTIAL", "IMPLEMENTED_UNTESTED", "UNKNOWN"}
CLOSED = {"TESTED", "APPLICATION", "NOT_APPLICABLE", "SUPPORTED", "NOT_SUPPORTED"}
SEVERITIES = {"high", "medium", "low"}


def spec_body(path: Path) -> list[str]:
    """The upstream text, without this repository's provenance header."""
    lines = path.read_text(encoding="utf-8").split("\n")
    end = next(i for i, line in enumerate(lines) if line.strip() == "-->")
    # The header is followed by one blank separator line.
    return lines[end + 2 :]


LIST_OR_ROW = re.compile(r"\s*([-*]|\d+\.|\|)\s?")


def normative_context(lines: list[str], n: int) -> bool:
    """Whether line `n` (1-based) sits in a list or table that a keyword
    sentence introduces, so its requirement is stated by that sentence:
    "Clients MUST follow these rules:" then the rules, or §5.4's
    "...MUST..." paragraph then its table."""
    if not LIST_OR_ROW.match(lines[n - 1]):
        return False
    i = n - 2
    while i >= 0 and (not lines[i].strip() or LIST_OR_ROW.match(lines[i])):
        if KEYWORD.search(lines[i]) and not lines[i].lstrip().startswith("|"):
            return True
        i -= 1
    return i >= 0 and bool(KEYWORD.search(lines[i]))


def is_lead_in(line: str) -> bool:
    return line.rstrip().endswith(":")


def indent(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def covered_by_list(lines: list[str], n: int, cited: set[int]) -> bool:
    """A lead-in at line `n` (1-based) is covered by entries on its own list.

    Its own list is the items directly after it; when the lead-in is itself a
    list item ("2. **RFC 8785 Compliance**: ... MUST ...:"), only the items
    nested under it, so a sibling item's entry cannot cover it.
    """
    lead = lines[n - 1]
    nested_only = bool(LIST_OR_ROW.match(lead))
    i = n  # index of the line after the lead-in
    seen_item = False
    while i < len(lines):
        text = lines[i]
        if not text.strip():
            i += 1
            continue
        if nested_only and indent(text) <= indent(lead):
            break
        if LIST_OR_ROW.match(text):
            seen_item = True
            if i + 1 in cited:
                return True
        elif seen_item and indent(text) == 0:
            break
        i += 1
    return False


def check_citations(entry: dict, problems: list[str]) -> None:
    where = f"entry at {entry.get('source', 'after_release')}:{entry.get('spec_line', entry.get('section'))}"
    for key, check in (("impl_evidence", citations.check_impl), ("test_evidence", citations.check_test)):
        for ev in entry.get(key, []):
            why = check(ROOT, ev)
            if why:
                problems.append(f"{where} cites {ev!r}: {why}")


def chapter(entry: dict) -> str:
    if entry.get("source") == "proto":
        return "proto"
    m = re.match(r"\D*(\d+|[A-Z]\b)", str(entry.get("section", "")))
    return m.group(1) if m else "?"


def render(ledger: dict) -> str:
    entries = ledger["entries"]
    statuses = ["TESTED", "APPLICATION", "SUPPORTED", "NOT_SUPPORTED", "NOT_APPLICABLE",
                "IMPLEMENTED_UNTESTED", "PARTIAL", "GAP", "UNKNOWN"]
    by_chapter: dict[str, dict[str, int]] = {}
    for e in entries:
        row = by_chapter.setdefault(chapter(e), {})
        row[e["status"]] = row.get(e["status"], 0) + 1
    used = [st for st in statuses if any(st in r for r in by_chapter.values())]

    def key(c: str) -> tuple[int, str]:
        return (0, f"{int(c):04d}") if c.isdigit() else (1, c)

    out = [
        "<!-- SPDX-License-Identifier: Apache-2.0 -->",
        "<!-- GENERATED by scripts/check_spec_ledger.py --write from spec-ledger.json. Do not edit. -->",
        "",
        "# A2A specification ledger",
        "",
        "Every RFC 2119 statement in the A2A specification, and every `REQUIRED` field",
        "in its proto, with what this SDK does about it. The data is",
        "[`spec-ledger.json`](spec-ledger.json); this page is generated from it, and",
        "`scripts/check_spec_ledger.py` (CI) fails when either drifts from the",
        "vendored specification or cites a test that no longer exists.",
        "",
        f"Specification: `{ledger['spec']['repository']}` `{ledger['spec']['path']}` at tag",
        f"`{ledger['spec']['tag']}` (`{ledger['spec']['commit'][:12]}`), vendored as",
        f"`{ledger['spec']['vendored_as']}` (SHA-256 `{ledger['spec']['sha256'][:16]}…`).",
        f"Proto: `{ledger['proto']['vendored_as']}`, A2A {ledger['proto']['release']}.",
        f"Audited at commit `{ledger['audited_commit']}`.",
        "",
        "**How the entries were made, and how far to trust them.** " + ledger["method"],
        "",
        "Statuses: `TESTED` (implemented, and a test asserts it); `IMPLEMENTED_UNTESTED`;",
        "`PARTIAL` (some bindings or paths); `GAP` (not implemented or violated);",
        "`APPLICATION` (the agent author's to do; the entry says whether the SDK gives",
        "a hook); `SUPPORTED`/`NOT_SUPPORTED` (for MAY); `NOT_APPLICABLE`.",
        "",
        "## By chapter",
        "",
        "| Chapter | " + " | ".join(used) + " | Total |",
        "|---|" + "---:|" * (len(used) + 1),
    ]
    for c in sorted(by_chapter, key=key):
        row = by_chapter[c]
        label = "proto `REQUIRED` fields" if c == "proto" else f"§{c}"
        out.append(f"| {label} | " + " | ".join(str(row.get(st, "")) for st in used)
                   + f" | {sum(row.values())} |")
    totals = {st: sum(r.get(st, 0) for r in by_chapter.values()) for st in used}
    out.append("| **All** | " + " | ".join(f"**{totals[st]}**" for st in used)
               + f" | **{len(entries)}** |")
    out += ["", "## Open items, most severe first", "",
            "| Severity | Status | Where | Requirement | Notes |", "|---|---|---|---|---|"]
    open_items = [e for e in entries if e["status"] in OPEN]
    open_items.sort(key=lambda e: (SEVERITY_ORDER.get(e.get("severity"), 3), e["status"],
                                   e.get("source", ""), e.get("spec_line", 0)))
    def cell(text: str, limit: int) -> str:
        text = " ".join(str(text).split()).replace("|", "\\|")
        return text if len(text) <= limit else text[: limit - 1] + "…"
    for e in open_items:
        where = f"{e['section']} (line {e['spec_line']})" if e["source"] == "spec" else f"{e['section']} (proto line {e['spec_line']})"
        out.append(f"| {e.get('severity')} | {e['status']} | {cell(where, 60)} | "
                   f"{cell(e['requirement'], 140)} | {cell(e.get('notes', ''), 260)} |")
    after = ledger.get("after_release")
    if after:
        out += ["", "## After the release", "", after["note"], "",
                "| Section | Keyword | Status | Requirement | Notes |", "|---|---|---|---|---|"]
        for e in after["entries"]:
            out.append(f"| {e['section']} | {e['keyword']} | {e['status']} | "
                       f"{cell(e['requirement'], 140)} | {cell(e.get('notes', ''), 260)} |")
    out.append("")
    return "\n".join(out)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--check", action="store_true", help="validate (default)")
    group.add_argument("--write", action="store_true", help="regenerate spec-ledger.md")
    group.add_argument("--fix-lines", action="store_true",
                       help="move citation line numbers to their unique target; list the rest")
    group.add_argument("--self-test", action="store_true", help="prove the citation rules, offline")
    parser.add_argument("--ledger", type=Path, default=LEDGER,
                        help="check this copy of the ledger instead (spec-ledger.md is not compared)")
    args = parser.parse_args()
    # The citation rules are proven on every run, so a change that makes one
    # accept everything fails this gate instead of passing the ledger.
    failed = self_test()
    if failed or args.self_test:
        return failed
    args.ledger = args.ledger.resolve()

    try:
        ledger = json.loads(args.ledger.read_text(encoding="utf-8"))
        spec = spec_body(SPEC)
        proto = PROTO.read_text(encoding="utf-8").split("\n")
    except (OSError, ValueError, StopIteration) as e:
        print(f"check_spec_ledger: cannot read inputs: {e}", file=sys.stderr)
        return 2

    entries = ledger.get("entries", [])
    problems: list[str] = []

    if args.ledger != LEDGER and args.write:
        print("check_spec_ledger: --write regenerates the real ledger's summary only", file=sys.stderr)
        return 2
    if args.fix_lines:
        fixed, left = citations.fix_lines(ROOT, entries + ledger.get("after_release", {}).get("entries", []))
        args.ledger.write_text(json.dumps(ledger, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"check_spec_ledger --fix-lines: moved {fixed} line number(s); {len(left)} left for a person")
        for p in left:
            print(f"  {p}")
        return 1 if left else 0

    # 0. The text the line numbers refer to.
    digest = hashlib.sha256("\n".join(spec).encode("utf-8")).hexdigest()
    pinned = ledger.get("spec", {}).get("sha256")
    if digest != pinned:
        problems.append(
            f"{SPEC.relative_to(ROOT)} hashes to {digest}, the ledger's spec.sha256 is "
            f"{pinned}: re-anchor every spec_line to the new text and update spec"
        )
    cited = {e["spec_line"] for e in entries if e.get("source") == "spec"}
    cited_proto = {e["spec_line"] for e in entries if e.get("source") == "proto"}

    # 1. Coverage of the specification.
    for n, line in enumerate(spec, start=1):
        if not KEYWORD.search(line) or n in cited:
            continue
        if is_lead_in(line) and covered_by_list(spec, n, cited):
            continue
        problems.append(f"spec line {n} has no entry: {line.strip()[:100]}")

    # The proto's REQUIRED fields are part of the specification too (§1.4).
    for n, line in enumerate(proto, start=1):
        if "field_behavior) = REQUIRED" in line and n not in cited_proto:
            problems.append(f"proto line {n} (REQUIRED) has no entry: {line.strip()[:100]}")

    for e in entries:
        source, n = e.get("source"), e.get("spec_line")
        # 2. Entries still point at normative text.
        if source == "spec":
            implied = "implied" in str(e.get("keyword", ""))
            if not isinstance(n, int) or not 1 <= n <= len(spec):
                problems.append(f"entry cites spec line {n}, which does not exist")
            elif not (
                KEYWORD.search(spec[n - 1])
                or normative_context(spec, n)
                # An implied requirement (an operation's Errors list, a
                # binding rule stated as a heading) has no keyword of its own.
                or implied
            ):
                problems.append(
                    f"entry cites spec line {n}, which is neither normative text nor "
                    f"a list item or table row under it: {spec[n - 1].strip()[:80]}"
                )
        elif source != "proto":
            problems.append(f"entry at line {n} has source {source!r}, not spec or proto")
        # 4. Vocabulary.
        status, severity = e.get("status"), e.get("severity")
        if status in OPEN:
            if severity not in SEVERITIES:
                problems.append(f"{source}:{n} is {status} with severity {severity!r}")
        elif status in CLOSED:
            if severity is not None:
                problems.append(f"{source}:{n} is {status} but carries severity {severity!r}")
        else:
            problems.append(f"{source}:{n} has unknown status {status!r}")
        # SUPPORTED / NOT_SUPPORTED say whether an optional feature is
        # offered, so they answer only a MAY or OPTIONAL. 23 entries had
        # used SUPPORTED for a MUST or SHOULD the SDK leaves to the
        # application or meets with a test (corrected 2026-10-18).
        if status in {"SUPPORTED", "NOT_SUPPORTED"}:
            first = (e.get("keyword") or "").split()[:1]
            if not first or first[0].upper() not in {"MAY", "OPTIONAL"}:
                problems.append(
                    f"{source}:{n} is {status}, which answers only a MAY or OPTIONAL, "
                    f"but its keyword is {e.get('keyword')!r}: use TESTED or APPLICATION"
                )
        # 3. Evidence exists and is where it is said to be.
        check_citations(e, problems)

    # 5. Requirements added upstream after the audited release.
    for e in ledger.get("after_release", {}).get("entries", []):
        where = f"after_release {e.get('section')}"
        if "spec_line" in e:
            problems.append(f"{where} cites spec_line {e['spec_line']}, which is not in the vendored text")
        status, severity = e.get("status"), e.get("severity")
        if status in OPEN and severity not in SEVERITIES:
            problems.append(f"{where} is {status} with severity {severity!r}")
        elif status in CLOSED and severity is not None:
            problems.append(f"{where} is {status} but carries severity {severity!r}")
        elif status not in OPEN | CLOSED:
            problems.append(f"{where} has unknown status {status!r}")
        check_citations(e, problems)

    summary_text = render(ledger)
    if args.write:
        SUMMARY.write_text(summary_text, encoding="utf-8")
        print(f"check_spec_ledger: wrote {SUMMARY.relative_to(ROOT)}")
    elif args.ledger != LEDGER:
        pass  # a copy under test: the summary describes the real ledger
    elif not SUMMARY.is_file() or SUMMARY.read_text(encoding="utf-8") != summary_text:
        problems.append(
            f"{SUMMARY.relative_to(ROOT)} is stale; run scripts/check_spec_ledger.py --write"
        )

    if problems:
        print(f"check_spec_ledger: {len(problems)} problem(s)", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1

    counts: dict[str, int] = {}
    for e in entries:
        counts[e["status"]] = counts.get(e["status"], 0) + 1
    summary = ", ".join(f"{k} {v}" for k, v in sorted(counts.items()))
    print(f"check_spec_ledger: {len(entries)} entries cover every keyword line; {summary}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
