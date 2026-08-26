#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Holds the book's uncompiled Rust blocks as a shrink-only ratchet.
#
# Until 2026-08-16 the book carried 158 Rust code blocks and nothing compiled
# any of them — no `mdbook test`, no skeptic, nothing in any workflow. The cost
# was real: the README's Quick Start stopped compiling before v0.8.0 and no one
# noticed for three minor versions, because `agent_executor!` was never
# exported from the prelude and the macro named a crate the snippet's own
# dependency list did not include.
#
# Two mechanisms replace that, and this script is the second:
#
#   1. `a2a-book-tests` includes every page with `#[doc = include_str!(..)]`,
#      so each non-ignored block is compiled and linked by `cargo test`.
#   2. This ratchet, which stops the first mechanism being defeated by simply
#      marking new blocks `ignore`.
#
# Mechanism 1's page list is written by hand, and until 2026-08-26 nothing
# checked it. A page nobody remembered to register was not compiled and not
# reported — the ratchet below would still pass, because a page with no
# `ignore`d blocks is indistinguishable from a page that is being compiled.
# Four pages were in that state. None of them happened to carry a live Rust
# block, so nothing was broken; that was luck, not a guard. The registration
# check below closes it, with one documented exclusion.
#
# Semantics match `check_file_lengths.sh` deliberately: the baseline must equal
# reality exactly. A count that grew is a failure (write a compiling block, or
# argue for the exemption in review). A count that shrank is also a failure,
# because a baseline entry that overstates reality reads as a live exemption
# while exempting nothing — fix it by updating the number, which is the good
# kind of failure.
#
# Untagged blocks are checked too. rustdoc treats a bare ``` fence as Rust, so
# 24 ASCII diagrams and terminal transcripts were being compiled as Rust before
# they were tagged `text`.
#
# Usage:
#   ./scripts/check_book_code.sh
#
# Exit codes:
#   0  baseline matches reality, and every page is registered for compilation
#   1  drift (ignore count changed, an untagged block appeared, or a page is
#      not registered in book-tests/src/lib.rs)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

python3 - <<'PY'
import pathlib
import re
import sys

BASELINE = pathlib.Path(".book-ignore-baseline")
BOOK = pathlib.Path("book/src")
LIB = pathlib.Path("book-tests/src/lib.rs")

# Pages deliberately not compiled, each with the reason. Anything else missing
# from `book-tests/src/lib.rs` is an omission, not a decision.
NOT_REGISTERED = {
    "SUMMARY.md": "mdBook's table of contents — a list of links, not a page",
}

if not BASELINE.exists():
    print("check_book_code: .book-ignore-baseline is missing", file=sys.stderr)
    sys.exit(1)

expected = {}
for line in BASELINE.read_text(encoding="utf-8").splitlines():
    if not line.strip() or line.startswith("#"):
        continue
    count, path = line.split("\t", 1)
    expected[path] = int(count)

actual = {}
untagged = []
for p in sorted(BOOK.rglob("*.md")):
    text = p.read_text(encoding="utf-8")
    n = len(re.findall(r"^```rust[^\n]*\bignore\b", text, re.M))
    if n:
        actual[str(p)] = n
    # An opening fence with no info string is compiled as Rust by rustdoc.
    inside = False
    for i, ln in enumerate(text.split("\n"), start=1):
        if ln.startswith("```"):
            if not inside:
                inside = True
                if ln.strip() == "```":
                    untagged.append(f"{p}:{i}")
            else:
                inside = False

grew, shrank, added, removed = [], [], [], []
for path, want in expected.items():
    have = actual.get(path, 0)
    if have > want:
        grew.append(f"{path}: baseline {want}, found {have}")
    elif have < want:
        shrank.append(f"{path}: baseline {want}, found {have}")
for path, have in actual.items():
    if path not in expected:
        added.append(f"{path}: {have} ignored block(s), not in the baseline")
removed = [p for p in expected if p not in actual]

# ── Every page must be compiled by something ─────────────────────────────────
unregistered = []
if not LIB.exists():
    unregistered.append(f"{LIB} is missing; nothing compiles the book's Rust")
else:
    registered = set(re.findall(r'include_str!\("\.\./\.\./book/src/([^"]+)"\)',
                                LIB.read_text(encoding="utf-8")))
    for page in sorted(str(q.relative_to(BOOK)) for q in BOOK.rglob("*.md")):
        if page in registered or page in NOT_REGISTERED:
            continue
        unregistered.append(page)
    # An exclusion for a page that no longer exists reads as a live decision
    # while excluding nothing — the same failure the baseline's `shrank` arm
    # exists for.
    on_disk = {str(q.relative_to(BOOK)) for q in BOOK.rglob("*.md")}
    for page in sorted(NOT_REGISTERED):
        if page not in on_disk:
            unregistered.append(f"{page} is excluded but no longer exists")

failed = False

if unregistered:
    failed = True
    print("check_book_code: book pages that nothing compiles")
    for u in unregistered:
        print(f"  UNREGISTERED  {u}")
    print("  Add `#[doc = include_str!(\"../../book/src/<page>\")]` to")
    print("  book-tests/src/lib.rs, or record the page in NOT_REGISTERED here")
    print("  with the reason. An unregistered page's Rust is never compiled,")
    print("  and nothing else would have said so.")
    print()

if untagged:
    failed = True
    print("check_book_code: code fences with no language tag "
          "(rustdoc compiles these as Rust)")
    for u in untagged:
        print(f"  UNTAGGED  {u}")
    print("  Tag them: ```text, ```bash, ```json, ```toml, or ```rust.")
    print()

if grew or added:
    failed = True
    print("check_book_code: more `ignore`d Rust blocks than the baseline allows")
    for m in grew + added:
        print(f"  GREW  {m}")
    print("  `ignore` means nothing compiles this block. Prefer making it")
    print("  compile — hidden `# ` preamble lines supply imports without")
    print("  showing them — or use ```text if it is not Rust.")
    print()

if shrank:
    failed = True
    print("check_book_code: fewer `ignore`d blocks than the baseline claims "
          "(good news, but the baseline has to say so)")
    for m in shrank:
        print(f"  STALE  {m}")
    print()

if removed:
    failed = True
    print("check_book_code: baseline lists files that no longer have "
          "`ignore`d blocks")
    for m in removed:
        print(f"  STALE  {m}")
    print()

if failed:
    print("  Update .book-ignore-baseline to match, keeping the total in its")
    print("  header in sync.")
    sys.exit(1)

total = sum(actual.values())
print(f"check_book_code: {total} ignored block(s) across {len(actual)} file(s), "
      f"matching the baseline; every other Rust block in the book is compiled")
PY
