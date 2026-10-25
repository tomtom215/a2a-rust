#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Every example crate has a page in the book, or a recorded reason it has none.

Until 2026-10-25 the book had pages for ten of the sixteen crates under
examples/. The overview said so in a sentence that nothing kept true: five
examples had a README and no page, and a seventeenth would have been missed the
same way. The book's own code gate (check_book_code.sh) asks whether a page
that exists is compiled; this asks whether the page exists.

Three assertions:

  1. Every directory examples/<name>/ that holds a Cargo.toml has
     book/src/examples/<name>.md, a link to it in SUMMARY.md and a link to it
     in the overview table, unless scripts/book_examples_allowlist.txt names it
     with a reason.
  2. Every examples page SUMMARY.md lists (the overview apart) belongs to an
     example directory that exists, so a removed example leaves no orphan page.
  3. Every allowlist entry still names an example that exists and has no page,
     so an exemption cannot outlive its reason.

Exit: 0 clean; 1 a finding; 2 not run from a repository checkout.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXAMPLES = ROOT / "examples"
BOOK = ROOT / "book" / "src"
PAGES = BOOK / "examples"
SUMMARY = BOOK / "SUMMARY.md"
OVERVIEW = PAGES / "overview.md"
ALLOWLIST = ROOT / "scripts" / "book_examples_allowlist.txt"

# `](./examples/<name>.md)` in SUMMARY.md, and `](./<name>.md)` in the overview.
SUMMARY_LINK = re.compile(r"\]\(\./examples/([^/)#]+)\.md\)")
OVERVIEW_LINK = re.compile(r"\]\(\./([^/)#]+)\.md\)")


def read_allowlist(problems):
    """`<example>  # <reason>` per line; the reason is required."""
    entries = {}
    if not ALLOWLIST.is_file():
        return entries
    for n, raw in enumerate(ALLOWLIST.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        name, _, reason = line.partition("#")
        if not name.strip() or not reason.strip():
            problems.append(f"{ALLOWLIST.name}:{n}: expected `<example>  # <reason>`")
            continue
        entries[name.strip()] = n
    return entries


def main():
    if not EXAMPLES.is_dir() or not SUMMARY.is_file() or not OVERVIEW.is_file():
        print("check_book_examples: run from a repository checkout", file=sys.stderr)
        return 2

    problems = []
    dirs = sorted(p.name for p in EXAMPLES.iterdir() if (p / "Cargo.toml").is_file())
    in_summary = set(SUMMARY_LINK.findall(SUMMARY.read_text(encoding="utf-8")))
    in_overview = set(OVERVIEW_LINK.findall(OVERVIEW.read_text(encoding="utf-8")))
    allowed = read_allowlist(problems)

    for name in dirs:
        has_page = (PAGES / f"{name}.md").is_file()
        if name in allowed:
            if has_page or name in in_summary:
                problems.append(
                    f"{ALLOWLIST.name}:{allowed[name]}: `{name}` has a page now. "
                    "An exemption that matches nothing is stale; delete the entry."
                )
            continue
        missing = []
        if not has_page:
            missing.append(f"book/src/examples/{name}.md does not exist")
        if name not in in_summary:
            missing.append("SUMMARY.md does not link it")
        if name not in in_overview:
            missing.append("the overview table does not link it")
        if missing:
            problems.append(f"examples/{name}: " + "; ".join(missing))

    for name in sorted(in_summary - {"overview"}):
        if name not in dirs:
            problems.append(
                f"SUMMARY.md links examples/{name}.md, but there is no "
                f"examples/{name}/Cargo.toml. Remove the page or restore the example."
            )
        elif not (PAGES / f"{name}.md").is_file():
            problems.append(f"SUMMARY.md links examples/{name}.md, which does not exist")

    for name, n in sorted(allowed.items()):
        if name not in dirs:
            problems.append(
                f"{ALLOWLIST.name}:{n}: `{name}` is not an example directory. "
                "An exemption that matches nothing is stale; delete the entry."
            )

    if problems:
        print(f"check_book_examples: {len(problems)} finding(s)", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        print(
            "\nAdd the page, list it in SUMMARY.md and the overview table, or record\n"
            f"why an example needs none in {ALLOWLIST.relative_to(ROOT)}.",
            file=sys.stderr,
        )
        return 1

    print(
        f"check_book_examples: {len(dirs)} example crates, "
        f"{len(dirs) - len(allowed)} with a page, {len(allowed)} exempt"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
