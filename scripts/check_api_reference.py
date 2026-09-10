#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when `book/src/reference/api-reference.md` and the crates disagree
about what exists at the crate roots.

# What problem this solves

That page is a hand-written listing of the public types, traits and functions
across the four crates. Nothing generated it and nothing checked it, so it could
only ever get less true: rename a type and the page still names the old one,
delete a trait and the page still documents it, add a re-export and the page
never hears of it. It is the same failure the benchmark-prose ratchet was built
for — a number, or here a name, that nothing recomputes is one that decays —
and it sat in the document a reader consults precisely when they do not yet
know the API well enough to notice.

Generated rustdoc now ships alongside the book at `/api/`, which covers the
"what exists" question exhaustively. This page survives because a curated
overview is genuinely more useful as a starting point than an exhaustive index.
Keeping it means keeping it true.

# What this checks, and what it does not

Two directions, both run on every invocation.

**Forward** (page → code): every backticked identifier in the page's tables is
looked up in the four crates' sources. A name the sources never define is a
hard failure. This is deliberately shallow: it verifies a definition exists
somewhere in the crates, not that the item is public, nor that it lives in the
module the page implies.

**Reverse** (code → page): every name a crate exports *at its root* must appear
in the first column of some table on the page. What counts as a root export —
`pub use` groups and globs, `pub` items, `#[macro_export]` macros, the sdk
facade's `prelude` — is spelled out in `scripts/lib/api_reference_roots.py`,
which implements this direction.

Still **not** checked in either direction: items that live only at module
depth (`a2a_protocol_types::security::...` without a root re-export),
visibility of a forward-checked name, and whether the page places an item in
the module it actually lives in. A stricter check needs the compiler's view —
`rustdoc --output-format json` is the eventual answer, but it is nightly-only,
and a gate that cannot run on the toolchain the project pins is a gate that
gets skipped. What is here catches the two failures that actually happen: a
rename or deletion leaving a stale name behind, and a new root export the
curated page never learns about.

There is no allowlist. Every root export is on the page or the check fails.

# Usage

    check_api_reference.py [--page book/src/reference/api-reference.md] [--explain]

`--explain` prints, per crate, every root name found and where it came from,
plus every glob the reverse pass could not resolve.

Exit codes:
    0  every name on the page exists, and every root export is on the page
    1  the page names something the crates do not define, or a root export
       is missing from the page
    2  the page or the crate sources could not be read
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# The reverse-direction pass lives beside this script, not on the interpreter's
# default path, so it resolves the same way from any working directory.
sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))

from api_reference_roots import collect_roots, names_in_first_column, report_roots  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
CRATES = REPO / "crates"

# The four published crates, in the order the page presents them.
CRATE_DIRS = (
    "a2a-protocol-types",
    "a2a-protocol-client",
    "a2a-protocol-server",
    "a2a-protocol-sdk",
)

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


# ── Forward direction: page → code ───────────────────────────────────────────


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


# ── Main ─────────────────────────────────────────────────────────────────────


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--page", type=Path, default=REPO / "book" / "src" / "reference" / "api-reference.md"
    )
    ap.add_argument(
        "--explain",
        action="store_true",
        help="list every root name per crate, its origin, and unresolved globs",
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

    # Forward: page -> code.
    sources = rust_sources()
    missing = [n for n in names if not defines(sources, n)]

    print(f"check_api_reference: {len(names)} name(s) listed in {args.page.name}")

    # Reverse: code -> page.
    listed = names_in_first_column(text)
    roots = collect_roots(CRATES, CRATE_DIRS)
    unlisted = report_roots(roots, listed, args.explain)

    if not roots or not any(r.names for r in roots):
        print("check_api_reference: parsed zero root exports from crates/", file=sys.stderr)
        print("  refusing to report agreement over an empty check", file=sys.stderr)
        return 2

    failed = False
    if missing:
        failed = True
        print(f"\nFAIL — {len(missing)} name(s) on the page are not defined in crates/:\n")
        for n in missing:
            print(f"    {n}")
        print(
            "\nThe API Quick Reference is hand-maintained. Something was renamed"
            "\nor removed and the page still advertises it. Update the page, or"
            "\nadd the name to IGNORE if it is prose rather than an item."
        )

    total_unlisted = sum(len(v) for v in unlisted.values())
    if unlisted:
        failed = True
        print(f"\nFAIL — {total_unlisted} root-level export(s) are not named on the page:\n")
        for crate, absent in unlisted.items():
            print(f"  {crate}:")
            for n in absent:
                print(f"    {n}")
        print(
            "\nEvery name a crate re-exports at its root must have a row on the"
            "\nAPI Quick Reference (first column of a table). Add a row with a"
            "\none-line description from the item's doc comment, or drop the"
            "\nroot re-export if it was not meant to be public API."
        )

    if failed:
        print(f"\ncheck_api_reference: FAIL (stale={len(missing)}, unlisted={total_unlisted})")
        return 1

    print(
        "OK — every name on the page is defined in the crates, and every"
        " root-level export is on the page."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
