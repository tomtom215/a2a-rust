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

Three more assertions cover where the defaults are *said* rather than tabulated
(0.16.0 turned `tls-rustls` on by default in the server, and the Installation
page, the README and the crate-map diagram went on saying it was the client's
and the SDK's alone; found 2026-11-07, docs/audits/2026-11-07-adopter-walkthrough):

  * a table with no Default column marks a default feature "on by default" in its
    Description, and marks nothing else so;
  * the Installation page's sentences `**`x` is on by default** for `crate`...`
    name, per feature, exactly the crates whose manifest default holds it;
  * `book/diagrams/gs-crate-map.mmd` lists each crate's `default:` features as
    its manifest does.

And one assertion about where a feature is *needed*: a book page whose Rust
blocks use an item that exists only under a Cargo feature (`PostgresTaskStore`,
`WebSocketDispatcher`, `tool_loop`, `auth::jwt`...) names that feature, in
backticks or in quotes, on that page. The book's own doctests turn every feature
on, so a page can compile there and fail for a reader who copied its dependency
line (`a2a-protocol-server = "0.16"`) and was never told the feature: nine pages
did, found 2026-11-07 by compiling every block against the default features
(docs/audits/2026-11-07-adopter-walkthrough, 03-book-sweep.sh). The migration
guides, ADR summaries and pitfalls pages describe what was true and are not read.

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
    if "default" not in header and "description" in header:
        col = header.index("description")
        for r in rows:
            f = plain(r[0])
            said = col < len(r) and "on by default" in r[col].lower()
            if said and f not in default:
                problems.append(f"{where}: `{f}` is described as on by default, and {crate}'s default is {sorted(default)}")
            elif not said and f in default:
                problems.append(f"{where}: `{f}` is on by default in {crate} and its row does not say so")
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


PROSE = ROOT / "book/src/getting-started/installation.md"
# `**`a` and `b` are on by default** for `a2a-protocol-x`, `a2a-protocol-y` and `a2a-protocol-z``
SAID = re.compile(
    r"\*\*((?:`[a-z-]+`(?:,?\s+and\s+|,\s+)?)+)\s+(?:is|are)\s+on\s+by\s+default\*\*\s+for\s+"
    r"((?:`a2a-protocol-[a-z]+`(?:,?\s+and\s+|,\s+)?)+)"
)
DIAGRAM = ROOT / "book/diagrams/gs-crate-map.mmd"
NODE = re.compile(r'^\s*[a-z]+\["(a2a-protocol-[a-z]+)<br/>(.*?)"\]\s*$')


def check_prose(problems: list[str]) -> None:
    """The Installation page's "on by default for ..." sentences, per feature."""
    text = PROSE.read_text()
    said: dict[str, set[str]] = {}
    for m in SAID.finditer(text):
        feats = re.findall(r"`([a-z-]+)`", m.group(1))
        crates = set(re.findall(r"`(a2a-protocol-[a-z]+)`", m.group(2)))
        for f in feats:
            said.setdefault(f, set()).update(crates)
    if not said:
        raise Unreadable(f"{PROSE.relative_to(ROOT)}: no \"on by default\" sentence found; the pattern in check_feature_tables.py needs to follow the prose")
    for f, crates in sorted(said.items()):
        actual = {c for c in CRATES if f in manifest(c)[1]}
        if crates != actual:
            problems.append(
                f"{PROSE.relative_to(ROOT)}: says `{f}` is on by default for {sorted(crates)}, "
                f"but the manifests turn it on in {sorted(actual)}"
            )


def check_diagram(problems: list[str]) -> None:
    """The crate-map diagram's `default:` lists against the manifests."""
    seen = set()
    for line in DIAGRAM.read_text().splitlines():
        m = NODE.match(line)
        if not m or "default:" not in m.group(2):
            continue
        crate = m.group(1)
        listed = m.group(2).split("default:", 1)[1].replace("<br/>", " ")
        names = {x.strip() for x in listed.split(",") if x.strip()}
        names = set() if names == {"none"} else names
        seen.add(crate)
        actual = manifest(crate)[1] if crate in CRATES else None
        if actual is not None and names != actual:
            problems.append(f"{DIAGRAM.relative_to(ROOT)}: {crate} lists default {sorted(names)}, its manifest has {sorted(actual)}")
    for crate in sorted(set(CRATES) - seen):
        problems.append(f"{DIAGRAM.relative_to(ROOT)}: no `default:` line for {crate}")


# An item that exists only under a feature, as it appears in a Rust block.
FEATURE_ITEMS = [
    (r"\bauth::jwt\b|\bJwt(?:AuthInterceptor|Validator)\b", "auth-jwt"),
    (r"\bWebSocket(?:Dispatcher|Transport)\b|\bdispatch::websocket\b", "websocket"),
    (r"\bPostgres[A-Z]\w*", "postgres"),
    (r"\b(?:TenantAware)?Sqlite[A-Z]\w*", "sqlite"),
    (r"\btool_loop\b", "tool-loop"),
    (r"\bGrpc(?:Dispatcher|Transport)\b|\bdispatch::grpc\b|\btransport::grpc\b|\bbuild_grpc\b", "grpc"),
    (r"\bRestRouter\b|\bA2aRouter\b", "axum"),
    (r"\ba2a_protocol_server::conformance\b|\bserver::conformance\b", "conformance"),
    (r"\bEncrypted[A-Z]\w*", "encryption"),
]
BOOK_SUMMARY = ROOT / "book/src/SUMMARY.md"
HISTORICAL_PAGES = (
    "reference/upgrading/", "reference/changelog", "reference/adrs.md", "reference/pitfalls",
    "deployment/dogfooding", "reference/mutation-history", "reference/conformance-history",
)
RUST_BLOCK = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,})(?P<info>[^\n`]*)\n(?P<body>.*?)^(?P=indent)(?P=fence)[ \t]*$", re.M | re.S)


def page_text_blocks(text: str) -> list[str]:
    """The Rust blocks of a page that rustdoc compiles (not `ignore`, not `text`)."""
    out = []
    for m in RUST_BLOCK.finditer(text):
        info = m.group("info").strip()
        tags = [t for t in re.split(r"[,\s]+", info) if t]
        if (not tags or tags[0] == "rust") and not any(t.startswith("ignore") for t in tags):
            out.append(m.group("body"))
    return out


def check_pages_name_their_features(problems: list[str]) -> None:
    """Each page that uses a feature-gated item names the feature."""
    base = ROOT / "book/src"
    pages = re.findall(r"\]\(\./([^)#]+\.md)\)", BOOK_SUMMARY.read_text())
    for rel in dict.fromkeys(pages):
        if rel.startswith(HISTORICAL_PAGES):
            continue
        text = (base / rel).read_text()
        # Comments name things (`// or: RestRouter (Axum)`) without using them.
        code = re.sub(r"//[^\n]*", "", "\n".join(page_text_blocks(text)))
        for pattern, feature in FEATURE_ITEMS:
            hit = re.search(pattern, code)
            if hit and f"`{feature}`" not in text and f'"{feature}"' not in text:
                problems.append(
                    f"book/src/{rel}: a Rust block uses `{hit.group(0)}`, which needs the `{feature}` "
                    f"feature, and the page never names it (a reader with the default features gets "
                    f"`unresolved import`)"
                )


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
    check_prose(problems)
    check_diagram(problems)
    check_pages_name_their_features(problems)
    return problems


def self_test() -> None:
    text = "## Features\n\n| Feature | Default |\n|---|---|\n| `a` | Yes |\n| `b` | No |\n"
    [(line, crate, header, rows)] = feature_tables(text)
    assert (line, crate, header) == (3, None, ["feature", "default"]), (line, crate, header)
    assert [plain(r[0]) for r in rows] == ["a", "b"]
    text = "### `a2a-protocol-client`\n\n| Feature | Description |\n|--|--|\n| `x` | y |\n\n## Other\n"
    assert feature_tables(text)[0][1] == "a2a-protocol-client"
    m = SAID.search("x **`a` and `b` are on by default** for `a2a-protocol-client`, `a2a-protocol-server` and `a2a-protocol-sdk`, y")
    assert m and re.findall(r"`([a-z-]+)`", m.group(1)) == ["a", "b"]
    assert re.findall(r"`(a2a-protocol-[a-z]+)`", m.group(2)) == ["a2a-protocol-client", "a2a-protocol-server", "a2a-protocol-sdk"]
    n = NODE.match('    server["a2a-protocol-server<br/>builds agents<br/>default: tracing, otel,<br/>tls-rustls"]')
    assert n and n.group(1) == "a2a-protocol-server" and "tls-rustls" in n.group(2)
    blocks = page_text_blocks("```rust\nlet a = PostgresTaskStore;\n```\n```text\nB\n```\n```rust,ignore\nC\n```\n```toml\nD\n```\n")
    assert blocks == ["let a = PostgresTaskStore;\n"], blocks
    assert re.search(FEATURE_ITEMS[2][0], "use a2a_protocol_server::store::PostgresTaskStore;")
    assert not re.search(FEATURE_ITEMS[2][0], "postgres is a database")
    print("check_feature_tables --self-test: table parsing, crate attribution, prose, diagram and page patterns pass")


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
