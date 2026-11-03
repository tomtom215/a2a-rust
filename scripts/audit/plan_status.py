#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Count how many of the triage plan's items have a fix commit.

`docs/audits/2026-11-01-triage/PLAN.md` lists every item that needs work, once,
under exactly one commit group ("- **Items (N):** A, B, C"). Section O of
`docs/audits/2026-10-29/README.md` lists, per commit, the rows that commit
fixed (wholly or in part). An item "has a commit" when its ID appears in a
section O row, except where the row says that item "is not done".

Why a script: the handoff quoted "97 of 191" and "94 with none" from a count
made by hand. The count is a set intersection, and a hand count of one is how
an ID that section O mentions only to say it is NOT done (NEW-g-docs-8) got
counted as done.

Usage:
    scripts/audit/plan_status.py            # print the counts and the open IDs
    scripts/audit/plan_status.py --json     # the same, as JSON
    scripts/audit/plan_status.py --self-test

Exit codes: 0 counted; 1 the files disagree with each other (an ID in two
groups, a group whose declared count differs from its list); 2 a file could not
be read or parsed.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PLAN = ROOT / "docs/audits/2026-11-01-triage/PLAN.md"
AUDIT = ROOT / "docs/audits/2026-10-29/README.md"

# An ID is a token that is not a piece of a longer one: `D-05` must not match
# inside `G/D-05`, and `E-F1` must not match inside `E-F13`.
_BOUND_BEFORE = r"(?<![A-Za-z0-9/_-])"
_BOUND_AFTER = r"(?![A-Za-z0-9_-])"


def parse_plan(text: str) -> tuple[dict[str, list[str]], list[str]]:
    """Return ({group: [ids]}, [problems]) from PLAN.md's group sections."""
    groups: dict[str, list[str]] = {}
    problems: list[str] = []
    group = None
    for line in text.splitlines():
        heading = re.match(r"^### ([A-Z][0-9]?)\. ", line)
        if heading:
            group = heading.group(1)
            continue
        items = re.match(r"^- \*\*Items \((\d+)\):\*\* (.+)$", line)
        if items and group:
            ids = [i.strip() for i in items.group(2).split(",") if i.strip()]
            if len(ids) != int(items.group(1)):
                problems.append(
                    f"group {group}: declares {items.group(1)} items, lists {len(ids)}"
                )
            groups[group] = ids
    return groups, problems


def section_o_rows(text: str) -> list[str]:
    """The `Rows` cell of every table row under `## O.` in the audit README."""
    out: list[str] = []
    in_o = False
    for line in text.splitlines():
        if line.startswith("## "):
            in_o = line.startswith("## O.")
            continue
        if in_o and line.startswith("| `"):
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) >= 3:
                out.append(cells[2])
    return out


def _expand_ranges(cell: str, known: set[str]) -> str:
    """Replace `X-02 to X-11` with every known ID of that prefix in between."""

    def repl(m: re.Match[str]) -> str:
        prefix, lo, hi = m.group(1), int(m.group(2)), int(m.group(4))
        found = []
        for i in known:
            k = re.fullmatch(re.escape(prefix) + r"(\d+)", i)
            if k and lo <= int(k.group(1)) <= hi:
                found.append(i)
        return " ".join(found)

    return re.sub(r"([A-Za-z/]+-)(\d+) to ([A-Za-z/]+-)(\d+)", repl, cell)


def ids_in_cell(cell: str, known: set[str]) -> set[str]:
    """Known IDs named in a section O cell, minus those it says are not done."""
    cell = cell.replace("`", "")
    not_done = set()
    for m in re.finditer(r"\(([^()]*\bnot done\b[^()]*)\)", cell):
        for i in known:
            if re.search(_BOUND_BEFORE + re.escape(i) + _BOUND_AFTER, m.group(1)):
                not_done.add(i)
    cell = re.sub(r"\([^()]*\bnot done\b[^()]*\)", " ", cell)
    cell = _expand_ranges(cell, known)
    found = set()
    for i in known:
        if re.search(_BOUND_BEFORE + re.escape(i) + _BOUND_AFTER, cell):
            found.add(i)
    return found - not_done if not_done else found


def count(plan_text: str, audit_text: str) -> dict:
    groups, problems = parse_plan(plan_text)
    seen: dict[str, str] = {}
    for g, ids in groups.items():
        for i in ids:
            if i in seen:
                problems.append(f"{i} is in group {seen[i]} and group {g}")
            seen[i] = g
    known = set(seen)
    done: set[str] = set()
    for cell in section_o_rows(audit_text):
        done |= ids_in_cell(cell, known)
    per_group = {
        g: {"items": len(ids), "with_commit": sum(1 for i in ids if i in done),
            "without": [i for i in ids if i not in done]}
        for g, ids in groups.items()
    }
    return {
        "items": len(known),
        "with_commit": len(done),
        "without_commit": len(known - done),
        "groups": per_group,
        "problems": problems,
    }


def _self_test() -> int:
    plan = (
        "### A. one\n- **Items (3):** X-1, G/D-05, X-10\n"
        "### B. two\n- **Items (3):** D-05, X-2, X-3\n"
    )
    audit = (
        "## N. other\n| `zzz` | fixed | X-2 |\n"
        "## O. status\n| Commit | Status now | Rows |\n|---|---|---|\n"
        "| `a1` | fixed | G/D-05 |\n"
        "| `a2` | fixed (part) | X-1 to X-3 (X-2 is not done) |\n"
    )
    r = count(plan, audit)
    cases = [
        ("items counted once each", r["items"] == 6),
        # G/D-05 is named; D-05 must not match inside it.
        ("D-05 is not matched inside G/D-05", "D-05" in r["groups"]["B"]["without"]),
        ("G/D-05 counted", "G/D-05" not in r["groups"]["A"]["without"]),
        # X-1 to X-3 expands to the known IDs between: X-1, X-2, X-3 (X-10 is out).
        ("a range expands to the known IDs inside it", "X-1" not in r["groups"]["A"]["without"]),
        ("X-10 is outside the range X-1 to X-3", "X-10" in r["groups"]["A"]["without"]),
        ("'is not done' removes the ID", "X-2" in r["groups"]["B"]["without"]),
        ("a section N row is not section O", r["with_commit"] == 3),
        ("no problems on consistent input", r["problems"] == []),
    ]
    dup = count("### A. a\n- **Items (1):** X-1\n### B. b\n- **Items (2):** X-1, X-9\n", audit)
    cases.append(("an ID in two groups is a problem", any("X-1 is in group A" in p for p in dup["problems"])))
    miscount = count("### A. a\n- **Items (2):** X-1\n", audit)
    cases.append(("a declared count that differs is a problem", any("declares 2" in p for p in miscount["problems"])))
    bad = [name for name, ok in cases if not ok]
    for name, ok in cases:
        print(f"{'ok  ' if ok else 'FAIL'} {name}")
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return _self_test()
    try:
        result = count(PLAN.read_text(encoding="utf-8"), AUDIT.read_text(encoding="utf-8"))
    except OSError as e:
        print(f"plan_status: {e}", file=sys.stderr)
        return 2
    if "--json" in argv:
        print(json.dumps(result, indent=2))
    else:
        print(f"{result['with_commit']} of {result['items']} plan items have at least one commit; "
              f"{result['without_commit']} do not")
        for g, v in result["groups"].items():
            print(f"  {g:>3}: {v['with_commit']:>2} of {v['items']:>2}   without: {', '.join(v['without']) or '-'}")
    for p in result["problems"]:
        print(f"plan_status: {p}", file=sys.stderr)
    return 1 if result["problems"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
