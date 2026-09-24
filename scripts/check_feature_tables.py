#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fails when a documented Cargo feature table disagrees with the manifest.

Each published crate's features are written down in six places: its own
README (its crates.io page), the crates overview, and two book pages, one of
which carries a Default column. None of them was checked, and the 2026-09-22
adopter audit found them wrong in both directions — `auth-jwt`, `grpc-tls`,
`tls-rustls` and `conformance` missing from the server's README, `proto` from
the types crate's, `auth-jwt` from the SDK's (escape class 1; S12, K3).

For every table whose first column is `Feature`, found in the places below,
this checks:

  * the set of features named equals the manifest's `[features]` keys, minus
    `default` — a missing row and an invented one both fail;
  * where a `Default` column exists, a feature marked on (`Yes`, `On`) is in
    the manifest's `default` list and one marked off (`No`, `Off`) is not;
  * in `crates/README.md`'s table, the `Crate(s)` column names exactly the
    crates (other than the SDK, which forwards) that define the feature.

Which crate a table describes comes from where it sits: a crate's own README,
or the nearest preceding heading naming a crate in backticks. A table this
cannot attribute is an error, not a skip.

Usage:
    python3 scripts/check_feature_tables.py
    python3 scripts/check_feature_tables.py --self-test

Exit codes: 0 every table agrees; 1 a table disagrees; 2 a table or manifest
could not be read.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATES = ("a2a-protocol-types", "a2a-protocol-client", "a2a-protocol-server", "a2a-protocol-sdk")
SHORT = {c.removeprefix("a2a-protocol-"): c for c in CRATES}
# Pages whose tables sit under `### \`a2a-protocol-…\`` headings.
BOOK_PAGES = ("book/src/getting-started/installation.md", "book/src/reference/configuration.md")
OVERVIEW = "crates/README.md"
ON, OFF = {"yes", "on"}, {"no", "off"}
CRATE_HEADING = re.compile(r"^#+ .*`(a2a-protocol-[a-z]+)`")


class Unreadable(Exception):
    pass


def manifest(crate: str) -> tuple[set[str], set[str]]:
    data = tomllib.loads((ROOT / "crates" / crate / "Cargo.toml").read_text())
    features = data.get("features", {})
    return set(features) - {"default"}, set(features.get("default", []))


def cells(line: str) -> list[str]:
    return [c.strip() for c in line.strip().strip("|").split("|")]


def plain(cell: str) -> str:
    return re.sub(r"[`*]", "", cell).strip()


def feature_tables(text: str) -> list[tuple[int, str | None, list[str], list[list[str]]]]:
    """(line, crate named by the nearest heading, header, rows) per Feature table."""
    out, heading_crate = [], None
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        m = CRATE_HEADING.match(lines[i])
        if m:
            heading_crate = m.group(1)
        elif lines[i].startswith("#"):
            heading_crate = None if lines[i].startswith("## ") else heading_crate
        if lines[i].startswith("|") and plain(cells(lines[i])[0]).lower() == "feature":
            header = [plain(c).lower() for c in cells(lines[i])]
            rows, j = [], i + 2
            while j < len(lines) and lines[j].startswith("|"):
                rows.append(cells(lines[j]))
                j += 1
            out.append((i + 1, heading_crate, header, rows))
            i = j
            continue
        i += 1
    return out


def check_table(where: str, crate: str, header: list[str], rows: list[list[str]], problems: list[str]) -> None:
    features, default = manifest(crate)
    named = {plain(r[0]) for r in rows}
    for f in sorted(features - named):
        problems.append(f"{where}: `{f}` is a feature of {crate} and is missing from the table")
    for f in sorted(named - features):
        problems.append(f"{where}: `{f}` is in the table but {crate} has no such feature")
    if "default" in header:
        col = header.index("default")
        for r in rows:
            f, mark = plain(r[0]), plain(r[col]).lower() if col < len(r) else ""
            if mark in ON and f not in default:
                problems.append(f"{where}: `{f}` is marked default, and {crate}'s default is {sorted(default)}")
            elif mark in OFF and f in default:
                problems.append(f"{where}: `{f}` is marked not default, but {crate} enables it by default")
            elif mark not in ON | OFF:
                problems.append(f"{where}: `{f}` has Default {r[col]!r}, which is neither on nor off")


def check_overview(problems: list[str]) -> None:
    path = ROOT / OVERVIEW
    tables = feature_tables(path.read_text())
    if len(tables) != 1:
        raise Unreadable(f"{OVERVIEW}: expected one Feature table, found {len(tables)}")
    line, _, header, rows = tables[0]
    if "crate(s)" not in header:
        raise Unreadable(f"{OVERVIEW}:{line}: the table has no Crate(s) column")
    col = header.index("crate(s)")
    defining = {c: manifest(c)[0] for c in CRATES if c != "a2a-protocol-sdk"}
    everything = set().union(*defining.values())
    named = set()
    for r in rows:
        f = plain(r[0])
        named.add(f)
        listed = {SHORT.get(x.strip(), x.strip()) for x in plain(r[col]).split(",")}
        actual = {c for c, fs in defining.items() if f in fs}
        if listed != actual:
            problems.append(
                f"{OVERVIEW}:{line}: `{f}` lists {sorted(listed)}, but it is defined by {sorted(actual)}"
            )
    for f in sorted(everything - named):
        problems.append(f"{OVERVIEW}:{line}: `{f}` is a feature of a published crate and is missing")


def run() -> list[str]:
    problems: list[str] = []
    for crate in CRATES:
        rel = f"crates/{crate}/README.md"
        tables = feature_tables((ROOT / rel).read_text())
        if not tables:
            raise Unreadable(f"{rel}: no Feature table")
        for line, _, header, rows in tables:
            check_table(f"{rel}:{line}", crate, header, rows, problems)
    for rel in BOOK_PAGES:
        tables = feature_tables((ROOT / rel).read_text())
        seen = set()
        for line, crate, header, rows in tables:
            if crate is None:
                raise Unreadable(f"{rel}:{line}: a Feature table under no crate heading")
            seen.add(crate)
            check_table(f"{rel}:{line}", crate, header, rows, problems)
        for crate in sorted(set(CRATES) - seen):
            problems.append(f"{rel}: no feature table for {crate}")
    check_overview(problems)
    return problems


def self_test() -> None:
    text = "## Features\n\n| Feature | Default |\n|---|---|\n| `a` | Yes |\n| `b` | No |\n"
    [(line, crate, header, rows)] = feature_tables(text)
    assert (line, crate, header) == (3, None, ["feature", "default"]), (line, crate, header)
    assert [plain(r[0]) for r in rows] == ["a", "b"]
    text = "### `a2a-protocol-client`\n\n| Feature | Description |\n|--|--|\n| `x` | y |\n\n## Other\n"
    assert feature_tables(text)[0][1] == "a2a-protocol-client"
    print("check_feature_tables --self-test: table parsing and crate attribution pass")


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        self_test()
        return 0
    try:
        problems = run()
    except (Unreadable, OSError, tomllib.TOMLDecodeError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    for p in problems:
        print(p)
    if problems:
        print(f"check_feature_tables: {len(problems)} disagreement(s) with the manifests")
        return 1
    print(f"check_feature_tables: every feature table agrees with {len(CRATES)} manifests")
    return 0


if __name__ == "__main__":
    sys.exit(main())
