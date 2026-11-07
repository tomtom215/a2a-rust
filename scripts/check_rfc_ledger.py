#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Keeps docs/compliance/rfc-ledger.json honest against the RFCs it audits.

`check_spec_ledger.py` does this for the A2A specification and is bound to it:
its text, its provenance header, its proto, its summary page. The RFCs the SDK
implements (JWS, JWT, their best-practice RFC) had no such table, so what
the code does about each MUST was the author's reading, tested by the author's
tests; `docs/handoff.md` records the stale-if-error bug that got through that
way. This is the same discipline for plain-text RFCs, reusing that checker's
citation library (`scripts/lib/spec_ledger/citations.py`) unchanged, so a
citation here means what it means there: `path:line Symbol` for Rust
implementation evidence, `path:line name` for a test, the line where the thing
is.

It checks, for each document in the ledger:

0. The vendored text hashes to the ledger's `sha256`. Every line number is a line
   of that exact text, so a refreshed copy fails here before the line checks can
   pass by coincidence.
1. Every line carrying MUST, SHALL, SHOULD, REQUIRED, RECOMMENDED, MAY or
   OPTIONAL (or their negations) is in some entry's `lines`, or in
   `excluded_lines` with a reason (the RFC 2119 boilerplate, for one).
2. Every cited line exists and carries a keyword, and is cited once.
3. `status` and `severity` use the vocabulary, an open status has a severity and
   a closed one has none; a `NOT_APPLICABLE` or `APPLICATION` entry says why in
   `notes`; a `DEVIATION` (the code knowingly does not do what a MUST says, for a
   reason that is written down) says why in `notes` and names the `decision` that
   made it, so a refusal of a MUST is a recorded choice and not an unexplained gap; `TESTED` has implementation and test evidence; `IMPLEMENTED_UNTESTED`
   has implementation evidence.
4. Every citation names what it points at and the line is where that is.
5. A `security` entry that is `TESTED` carries `mutant_evidence`: each item names
   a test from the entry's `test_evidence`, so "tested" for a check an attacker
   would target means a test was seen to fail with the check removed.

It does not, and cannot, check that a test proves what the entry says, or that
a recorded mutant was ever applied: that is in the notes and in review. It
checks that the evidence exists, is where it is said to be, and is named.

`--write` regenerates docs/compliance/rfc-ledger.md, the readable summary; the
default `--check` also fails when it is stale. `--fix-lines` moves citation line
numbers to their unique target, as the other checker's does. `--self-test`
proves each rule refuses what it should, offline.

Exit 0 when all hold, 1 otherwise, 2 when an input is missing or unreadable.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))

from spec_ledger import citations  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs/compliance/rfc-ledger.json"
SUMMARY = ROOT / "docs/compliance/rfc-ledger.md"

KEYWORD = re.compile(
    r"\b(MUST NOT|MUST|SHALL NOT|SHALL|SHOULD NOT|SHOULD|NOT RECOMMENDED|RECOMMENDED|REQUIRED|MAY|OPTIONAL)\b"
)
OPEN = {"GAP", "PARTIAL", "IMPLEMENTED_UNTESTED", "UNKNOWN"}
CLOSED = {"TESTED", "APPLICATION", "NOT_APPLICABLE", "SUPPORTED", "NOT_SUPPORTED", "DEVIATION"}
SEVERITIES = {"high", "medium", "low"}
SEVERITY_ORDER = {"high": 0, "medium": 1, "low": 2}


def check_document(root: Path, doc: dict) -> list[str]:
    """Every problem with one document of the ledger."""
    ident = doc.get("id", "?")
    problems: list[str] = []
    path = root / doc.get("path", "")
    try:
        raw = path.read_bytes()
    except OSError as e:
        return [f"{ident}: cannot read {doc.get('path')}: {e}"]
    digest = hashlib.sha256(raw).hexdigest()
    if digest != doc.get("sha256"):
        problems.append(
            f"{ident}: {doc['path']} hashes to {digest}, the ledger's sha256 is {doc.get('sha256')}: "
            "re-anchor every line to the new text and update the hash"
        )
    lines = raw.decode("utf-8").split("\n")
    keyword_lines = {n for n, text in enumerate(lines, start=1) if KEYWORD.search(text)}

    cited: dict[int, int] = {}
    for i, entry in enumerate(doc.get("entries", [])):
        where = f"{ident} entry {i} (section {entry.get('section')})"
        for n in entry.get("lines", []):
            if not isinstance(n, int) or n not in keyword_lines:
                problems.append(f"{where} cites line {n}, which has no requirement keyword")
            elif n in cited:
                problems.append(f"{where} cites line {n}, which entry {cited[n]} cites too")
            else:
                cited[n] = i
        if not entry.get("lines"):
            problems.append(f"{where} cites no line")
        problems += check_entry(root, where, entry)

    excluded: set[int] = set()
    for item in doc.get("excluded_lines", []):
        n = item.get("line")
        if n not in keyword_lines:
            problems.append(f"{ident} excludes line {n}, which has no requirement keyword")
        elif not str(item.get("reason", "")).strip():
            problems.append(f"{ident} excludes line {n} without a reason")
        elif n in cited:
            problems.append(f"{ident} excludes line {n}, which an entry cites")
        excluded.add(n)

    for n in sorted(keyword_lines - set(cited) - excluded):
        problems.append(f"{ident} line {n} has no entry: {lines[n - 1].strip()[:90]}")
    return problems


def check_entry(root: Path, where: str, e: dict) -> list[str]:
    problems: list[str] = []
    status, severity = e.get("status"), e.get("severity")
    if status in OPEN:
        if severity not in SEVERITIES:
            problems.append(f"{where} is {status} with severity {severity!r}")
    elif status in CLOSED:
        if severity is not None:
            problems.append(f"{where} is {status} but carries severity {severity!r}")
    else:
        problems.append(f"{where} has unknown status {status!r}")
    if status in {"SUPPORTED", "NOT_SUPPORTED"}:
        first = (e.get("keyword") or "").split()[:1]
        if not first or first[0].upper() not in {"MAY", "OPTIONAL"}:
            problems.append(f"{where} is {status}, which answers only a MAY or OPTIONAL")
    if not str(e.get("requirement", "")).strip():
        problems.append(f"{where} does not say what the requirement is")
    if status in {"NOT_APPLICABLE", "APPLICATION", "DEVIATION"} and not str(e.get("notes", "")).strip():
        problems.append(f"{where} is {status} without notes saying why")
    if status == "DEVIATION" and not str(e.get("decision", "")).strip():
        problems.append(f"{where} is a DEVIATION without the decision that made it")
    impl, tests = e.get("impl_evidence", []), e.get("test_evidence", [])
    if status == "TESTED" and not (impl and tests):
        problems.append(f"{where} is TESTED without implementation and test evidence")
    if status == "IMPLEMENTED_UNTESTED" and not impl:
        problems.append(f"{where} is IMPLEMENTED_UNTESTED without implementation evidence")
    if e.get("security") and status == "TESTED":
        names = [re.sub(r"^\S+:\d+\s+", "", t).split(" (")[0].strip() for t in tests]
        mutants = e.get("mutant_evidence", [])
        if not mutants:
            problems.append(f"{where} is a TESTED security entry without mutant_evidence")
        for m in mutants:
            if not any(name and name in m for name in names):
                problems.append(f"{where} mutant_evidence {m!r} names none of its tests")
    for key, check in (("impl_evidence", citations.check_impl), ("test_evidence", citations.check_test)):
        for ev in e.get(key, []):
            why = check(root, ev)
            if why:
                problems.append(f"{where} cites {ev!r}: {why}")
    return problems


def render(ledger: dict) -> str:
    out = [
        "<!-- SPDX-License-Identifier: Apache-2.0 -->",
        "<!-- Generated by scripts/check_rfc_ledger.py --write from rfc-ledger.json; do not edit. -->",
        "",
        "# RFC requirements ledger",
        "",
        "What this repository does about each MUST, SHOULD and MAY of the RFCs it implements, "
        "and which test says so. The rules are in `scripts/check_rfc_ledger.py`.",
        "",
    ]
    for doc in ledger.get("documents", []):
        entries = doc.get("entries", [])
        counts: dict[str, int] = {}
        for e in entries:
            counts[e.get("status", "?")] = counts.get(e.get("status", "?"), 0) + 1
        out += [f"## {doc.get('title', doc.get('id'))} (`{doc.get('id')}`)", ""]
        out.append(", ".join(f"{k} {v}" for k, v in sorted(counts.items())) or "no entries")
        out.append("")
        opened = [e for e in entries if e.get("status") in OPEN | {"DEVIATION"}]
        opened.sort(key=lambda e: (SEVERITY_ORDER.get(e.get("severity"), 9), e.get("lines", [0])[0]))
        if opened:
            out += ["| Section | Keyword | Status | Severity | Requirement |", "|---|---|---|---|---|"]
            for e in opened:
                out.append(
                    f"| {e.get('section')} | {e.get('keyword')} | {e['status']} | {e.get('severity') or 'decided'} "
                    f"| {str(e.get('requirement', '')).replace('|', '/')} |"
                )
            out.append("")
    return "\n".join(out) + "\n"


def self_test() -> int:
    """Each rule, shown to refuse; the real ledger is not read."""
    rfc = "Preface.\nA server MUST verify it.\nA server SHOULD NOT cache it.\nPlain line.\nIt MAY retry.\n"
    code = "fn verify() {}\n\n#[test]\nfn rejects() {}\n"
    problems: list[str] = []
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "rfc.txt").write_text(rfc, encoding="utf-8")
        (root / "lib.rs").write_text(code, encoding="utf-8")
        digest = hashlib.sha256(rfc.encode()).hexdigest()

        def doc(entries, excluded=(), sha=digest):
            return {"id": "t", "path": "rfc.txt", "sha256": sha, "entries": list(entries), "excluded_lines": list(excluded)}

        def entry(lines, **kw):
            base = {"lines": lines, "section": "1", "keyword": "MUST", "requirement": "r", "status": "NOT_APPLICABLE",
                    "severity": None, "notes": "why"}
            base.update(kw)
            return base

        full = [entry([2]), entry([3]), entry([5], keyword="MAY")]

        def expect(name: str, d: dict, fragment: str | None) -> None:
            got = check_document(root, d)
            if fragment is None and got:
                problems.append(f"{name}: should pass, got {got}")
            elif fragment is not None and not any(fragment in g for g in got):
                problems.append(f"{name}: should fail with {fragment!r}, got {got}")

        expect("a complete ledger passes", doc(full), None)
        expect("a changed text", doc(full, sha="0" * 64), "hashes to")
        expect("an uncovered keyword line", doc(full[:2]), "line 5 has no entry")
        expect("an excluded line passes", doc(full[:2], [{"line": 5, "reason": "boilerplate"}]), None)
        expect("an exclusion needs a reason", doc(full[:2], [{"line": 5, "reason": ""}]), "without a reason")
        expect("a line with no keyword", doc(full + [entry([4])]), "no requirement keyword")
        expect("a line cited twice", doc(full + [entry([2])]), "cites too")
        expect("an unknown status", doc([entry([2], status="DONE")] + full[1:]), "unknown status")
        expect("an open status needs a severity", doc([entry([2], status="GAP")] + full[1:]), "with severity None")
        expect("a closed status has no severity", doc([entry([2], severity="low")] + full[1:]), "carries severity")
        expect("not applicable says why", doc([entry([2], notes="")] + full[1:]), "without notes")
        expect("a deviation names its decision", doc([entry([2], status="DEVIATION")] + full[1:]), "without the decision")
        expect("a deviation with a decision passes", doc([entry([2], status="DEVIATION", decision="docs/handoff.md")] + full[1:]), None)
        expect("TESTED needs both evidences", doc([entry([2], status="TESTED")] + full[1:]), "without implementation and test")
        good_impl, good_test = ["lib.rs:1 verify"], ["lib.rs:4 rejects"]
        tested = entry([2], status="TESTED", impl_evidence=good_impl, test_evidence=good_test)
        expect("TESTED with evidence passes", doc([tested] + full[1:]), None)
        expect("a citation off by a line", doc([dict(tested, impl_evidence=["lib.rs:2 verify"])] + full[1:]), "cites")
        secure = dict(tested, security=True)
        expect("a security entry needs mutant evidence", doc([secure] + full[1:]), "without mutant_evidence")
        expect("mutant evidence names its test", doc([dict(secure, mutant_evidence=["removed the check"])] + full[1:]), "names none of its tests")
        expect("mutant evidence naming its test passes",
               doc([dict(secure, mutant_evidence=["lib.rs:1: check removed -> rejects FAILS"])] + full[1:]), None)
    for p in problems:
        print(f"check_rfc_ledger self-test: {p}", file=sys.stderr)
    if not problems:
        print("check_rfc_ledger self-test: every rule refuses what it should")
    return 1 if problems else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--check", action="store_true", help="validate (default)")
    group.add_argument("--write", action="store_true", help="regenerate rfc-ledger.md")
    group.add_argument("--fix-lines", action="store_true", help="move citation line numbers to their unique target")
    group.add_argument("--self-test", action="store_true", help="prove each rule, offline")
    parser.add_argument("--ledger", type=Path, default=LEDGER, help="check this copy instead")
    args = parser.parse_args()
    failed = self_test()
    if failed or args.self_test:
        return failed
    try:
        ledger = json.loads(args.ledger.read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        print(f"check_rfc_ledger: cannot read {args.ledger}: {e}", file=sys.stderr)
        return 2
    docs = ledger.get("documents", [])
    if args.fix_lines:
        entries = [e for d in docs for e in d.get("entries", [])]
        fixed, left = citations.fix_lines(ROOT, entries)
        args.ledger.write_text(json.dumps(ledger, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"check_rfc_ledger --fix-lines: moved {fixed} line number(s); {len(left)} left for a person")
        for p in left:
            print(f"  {p}")
        return 1 if left else 0
    problems = [p for d in docs for p in check_document(ROOT, d)]
    text = render(ledger)
    if args.write:
        SUMMARY.write_text(text, encoding="utf-8")
        print(f"check_rfc_ledger: wrote {SUMMARY.relative_to(ROOT)}")
    elif args.ledger.resolve() == LEDGER and (not SUMMARY.is_file() or SUMMARY.read_text(encoding="utf-8") != text):
        problems.append(f"{SUMMARY.relative_to(ROOT)} is stale; run scripts/check_rfc_ledger.py --write")
    if problems:
        print(f"check_rfc_ledger: {len(problems)} problem(s)", file=sys.stderr)
        for p in problems[:200]:
            print(f"  {p}", file=sys.stderr)
        if len(problems) > 200:
            print(f"  ... and {len(problems) - 200} more", file=sys.stderr)
        return 1
    total = sum(len(d.get("entries", [])) for d in docs)
    print(f"check_rfc_ledger: {total} entries in {len(docs)} document(s) cover every keyword line")
    return 0


if __name__ == "__main__":
    sys.exit(main())
