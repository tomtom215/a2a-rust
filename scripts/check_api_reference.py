#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when `book/src/reference/api-reference.md` names something that no
longer exists.

# What problem this solves

That page is a hand-written listing of every public type, trait and function
across the four crates. Nothing generated it and nothing checked it, so it could
only ever get less true: rename a type and the page still names the old one,
delete a trait and the page still documents it. It is the same failure the
benchmark-prose ratchet was built for — a number, or here a name, that nothing
recomputes is one that decays — and it sat in the document a reader consults
precisely when they do not yet know the API well enough to notice.

Generated rustdoc now ships alongside the book at `/api/`, which covers the
"what exists" question exhaustively. This page survives because a curated
overview is genuinely more useful as a starting point than an exhaustive index.
Keeping it means keeping it true.

# What this checks, and what it does not

Every backticked identifier in the page's tables is looked up in the four
crates' sources. A name the sources never define is a hard failure.

Deliberately one-directional: it catches names on the page that do not exist in
the code, not public items missing from the page. Exhaustiveness is rustdoc's
job and this page is explicitly a *selection*, so flagging every unlisted item
would produce noise the page's whole purpose is to avoid.

Also deliberately shallow: it verifies a definition exists somewhere in the
crates, not that the item is public, nor that it lives in the module the page
implies. A stricter check needs the compiler's view — `rustdoc --output-format
json` is the eventual answer, but it is nightly-only, and a gate that cannot run
on the toolchain the project pins is a gate that gets skipped. What is here
catches the failure that actually happens: a rename or deletion leaving a stale
name behind.

# Usage

    check_api_reference.py [--page book/src/reference/api-reference.md]

Exit codes:
    0  every name on the page exists in the crates
    1  the page names something the crates do not define
    2  the page or the crate sources could not be read
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRATES = REPO / "crates"

# Rust keywords, primitives and prose that legitimately appear in backticks but
# are not items to look up. Kept explicit rather than inferred: a heuristic that
# silently skipped real names would report agreement it had not established.
IGNORE = {
    # primitives and std shorthands
    "bool", "u8", "u16", "u32", "u64", "usize", "i32", "i64", "f32", "f64",
    "str", "String", "Vec", "Option", "Result", "HashMap", "BTreeMap", "Arc",
    "Box", "Duration", "Instant", "Pin", "Future", "Send", "Sync", "Clone",
    "Debug", "Default", "Iterator", "Stream", "PathBuf", "SocketAddr",
    "serde_json", "Value", "Self", "impl", "dyn", "async", "await", "true",
    "false", "None", "Some", "Ok", "Err",
}

# Anything that looks like a path, call, snippet or prose fragment rather than a
# bare item name.
NOT_AN_ITEM = re.compile(r"[\s()\[\]{}<>:=,.\"'/\\|+*&^%$#@!?;-]")


def rust_sources() -> str:
    """Every line of Rust in the four published crates, concatenated."""
    if not CRATES.is_dir():
        print(f"check_api_reference: {CRATES} not found", file=sys.stderr)
        raise SystemExit(2)
    chunks = []
    for path in sorted(CRATES.rglob("*.rs")):
        try:
            chunks.append(path.read_text(encoding="utf-8"))
        except OSError as e:
            print(f"check_api_reference: cannot read {path}: {e}", file=sys.stderr)
            raise SystemExit(2) from e
    if not chunks:
        print("check_api_reference: no Rust sources found", file=sys.stderr)
        raise SystemExit(2)
    return "\n".join(chunks)


def defines(sources: str, name: str) -> bool:
    """Whether the crates define an item with this name."""
    # Types, traits, enums, unions, modules, consts, statics, functions, macros
    # and type aliases — plus enum variants and struct fields, which the page
    # also names.
    patterns = (
        rf"\b(?:struct|enum|trait|union|mod|type|const|static|fn|macro_rules!)\s+{re.escape(name)}\b",
        rf"\b{re.escape(name)}\s*(?:\{{|\(|,|=|:)",  # variant / field / builder method
    )
    return any(re.search(p, sources) for p in patterns)


def names_on_page(text: str) -> list[str]:
    """Backticked identifiers from the page's tables."""
    found: list[str] = []
    seen: set[str] = set()
    for line in text.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        for raw in re.findall(r"`([^`]+)`", line):
            token = raw.strip()
            # Strip a trailing `()` so `validate()` checks as `validate`.
            token = re.sub(r"\(\)$", "", token)
            if not token or token in IGNORE or token in seen:
                continue
            if NOT_AN_ITEM.search(token):
                continue
            seen.add(token)
            found.append(token)
    return found


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--page", type=Path, default=REPO / "book" / "src" / "reference" / "api-reference.md"
    )
    args = ap.parse_args()

    try:
        text = args.page.read_text(encoding="utf-8")
    except OSError as e:
        print(f"check_api_reference: cannot read {args.page}: {e}", file=sys.stderr)
        return 2

    names = names_on_page(text)
    if not names:
        print("check_api_reference: parsed zero names from the page", file=sys.stderr)
        print("  refusing to report agreement over an empty check", file=sys.stderr)
        return 2

    sources = rust_sources()
    missing = [n for n in names if not defines(sources, n)]

    print(f"check_api_reference: {len(names)} name(s) listed in {args.page.name}")

    if missing:
        print(f"\nFAIL — {len(missing)} name(s) on the page are not defined in crates/:\n")
        for n in missing:
            print(f"    {n}")
        print(
            "\nThe API Quick Reference is hand-maintained. Something was renamed"
            "\nor removed and the page still advertises it. Update the page, or"
            "\nadd the name to IGNORE if it is prose rather than an item."
        )
        return 1

    print("OK — every name on the page is defined in the crates.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
