#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check that every dependency snippet in prose names the current release line.

Prose that tells a reader what to put in their `Cargo.toml` is the one place a
stale version is actively harmful rather than merely untidy: `a2a-protocol-sdk
= "0.7"` means `^0.7`, which does not resolve to anything in the 0.12 line, so
a reader who copies it gets a five-minor-old SDK and none of the fixes since.

Nothing checked these, and they rotted exactly as you would expect. Measured
2026-09-17 at `e057c8e`, the tag of 0.12.1: 28 snippets across 14 files named
0.7, 0.8 or 0.11, including the root `README.md`, every page under
`book/src/getting-started/`, and the two `websocket.rs` module docs. One
snippet in the whole repository was current. This is the same decay class as
the benchmark prose and the API Quick Reference, and it is worse than either,
because the reader acting on it is by definition the one who does not yet know
which version is current.

`release.yml` does not catch this. It verifies the four crate `Cargo.toml`
versions against the tag and nothing else; prose is not a manifest.

What counts as current
----------------------
The MAJOR.MINOR of the workspace, not the full version: `= "0.12"` means
`^0.12` and resolves to the newest patch, which is what a reader wants, and it
means a patch release does not have to touch any of these files. A snippet
naming the exact patch is therefore reported too — not because it is harmful,
but because allowing both spellings makes the gate ambiguous and the fix
non-mechanical.

Scope
-----
Tracked markdown outside the historical records, plus Rust doc comments
(`//!`, `///`) under `crates/*/src`. `CHANGELOG.md` and the book's changelog
page are excluded because their whole content is what past versions said;
`docs/` is excluded because it holds dated reviews, assessments and handoffs,
which are records of a moment rather than instructions to a reader.

Deliberately historical snippets — a migration guide's before/after pair —
are named in `scripts/doc_versions_allowlist.txt` with a reason. An allowlist
entry that matches nothing fails the gate, so the file cannot rot either.

Exit codes: 0 every snippet names the current line or is allowlisted with a
reason, and every entry matches a snippet; 1 a snippet is stale or an entry is
stale; 2 not run from the repository root, or the allowlist is malformed.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / "scripts" / "doc_versions_allowlist.txt"
SOURCE_OF_TRUTH = ROOT / "crates" / "a2a-protocol-types" / "Cargo.toml"

# Records of what past versions said, not instructions to a reader.
EXCLUDED = (
    "CHANGELOG.md",
    "book/src/reference/changelog.md",
    "docs/",
)

CRATES = "a2a-protocol-(?:types|client|server|sdk)"
# `name = "X.Y"` or `name = { ... version = "X.Y" ... }`, the two spellings a
# reader is ever shown. A `path =` dependency is a workspace-internal manifest
# line, which release.yml's own gate and RELEASING.md §1 cover.
SNIPPET = re.compile(
    rf'\b{CRATES}\s*=\s*(?:"(?P<bare>\d+\.\d+(?:\.\d+)?)"'
    rf'|\{{(?P<inline>[^}}]*)\}})'
)
INLINE_VERSION = re.compile(r'version\s*=\s*"(\d+\.\d+(?:\.\d+)?)"')
DOC_COMMENT = re.compile(r"^\s*//[!/]")


def current_line() -> str:
    """MAJOR.MINOR of the workspace, from the types crate's own manifest."""
    data = tomllib.loads(SOURCE_OF_TRUTH.read_text(encoding="utf-8"))
    version = data["package"]["version"]
    major, minor, *_ = version.split(".")
    return f"{major}.{minor}"


def tracked() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "*.md", "*.rs"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout
    return [
        p for p in out.splitlines()
        if p and not p.startswith(EXCLUDED)
        and (p.endswith(".md") or p.startswith("crates/"))
    ]


def snippets() -> list[tuple[str, int, str, str]]:
    """Every dependency snippet as (path, line number, version, line text)."""
    found = []
    for rel in tracked():
        path = ROOT / rel
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        rust = rel.endswith(".rs")
        for n, line in enumerate(text.splitlines(), 1):
            if rust and not DOC_COMMENT.match(line):
                continue
            for m in SNIPPET.finditer(line):
                version = m.group("bare")
                if version is None:
                    inline = m.group("inline")
                    if "path" in inline:
                        continue  # workspace-internal, covered elsewhere
                    hit = INLINE_VERSION.search(inline)
                    if hit is None:
                        continue  # a bare path dep, or a feature-only table
                    version = hit.group(1)
                found.append((rel, n, version, line.strip()))
    return found


def allowlist() -> list[tuple[str, str, str]]:
    """Entries as (path, version, reason). Malformed input exits 2."""
    if not ALLOWLIST.exists():
        return []
    entries = []
    for n, raw in enumerate(ALLOWLIST.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.split("#", 1)
        body, reason = line[0].strip(), (line[1].strip() if len(line) > 1 else "")
        if not body:
            continue  # blank, or a comment-only header line
        parts = body.split()
        if len(parts) != 2:
            print(
                f"check_doc_versions: {ALLOWLIST.name}:{n}: expected "
                f"`FILE VERSION  # reason`, got {raw!r}",
                file=sys.stderr,
            )
            sys.exit(2)
        if not reason:
            print(
                f"check_doc_versions: {ALLOWLIST.name}:{n}: entry has no reason",
                file=sys.stderr,
            )
            sys.exit(2)
        entries.append((parts[0], parts[1], reason))
    return entries


def main() -> int:
    if not SOURCE_OF_TRUTH.exists():
        print("check_doc_versions: run from the repository root", file=sys.stderr)
        return 2

    line = current_line()
    entries = allowlist()
    allowed = {(path, version) for path, version, _ in entries}

    found = snippets()
    stale = [s for s in found if s[2] != line and (s[0], s[2]) not in allowed]
    used = {(s[0], s[2]) for s in found}
    orphans = [e for e in entries if (e[0], e[1]) not in used]

    if stale or orphans:
        print("check_doc_versions: prose names a version that is not the "
              f"current {line} line\n")
        for path, n, version, text in stale:
            print(f"  {path}:{n}: names {version}")
            print(f"      {text}")
        for path, version, reason in orphans:
            print(f"  {ALLOWLIST.name}: no snippet in {path} names {version}")
            print(f"      recorded reason: {reason}")
        if stale:
            print(
                f'\n`= "X.Y"` means `^X.Y`, so a snippet naming an older line '
                f"resolves to\nthat line and never to {line}. A reader who "
                "copies it gets an SDK without\nany fix released since. Update "
                f'each to "{line}", or — if the snippet is\ndeliberately '
                f"historical, as a migration guide's before/after pair is — add "
                f"it\nto {ALLOWLIST.name} with the reason."
            )
        if orphans:
            print(
                f"\nAn allowlist entry that matches nothing is stale: the "
                "snippet it excused\nwas edited or removed. Delete the entry."
            )
        return 1

    print(
        f"check_doc_versions: {len(found)} dependency snippet(s) across "
        f"{len({s[0] for s in found})} file(s) name the current {line} line; "
        f"{len(entries)} allowlisted as historical"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
