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

The deploy-agent manifest
-------------------------
`examples/deploy-agent/deployment.yaml` names the container image the release
workflow publishes, `ghcr.io/<owner>/<repo>/deploy-agent:<version>`, by the
workspace's exact version (the image is tagged with the release version only;
there is no `latest`). A manifest that names another version is a first run
that cannot pull its image (audit A13), so this gate requires the tag in the
manifest to be the workspace version, and requires exactly one such image.

The README's conformance figure
-------------------------------
The README says how the official TCK graded this SDK twice: in the badge at the
top and in the Project Status paragraph. The badge is a URL, nobody reads it
while editing the prose, and on 2026-11-07 it still said 83/114 with 5 failing
while the paragraph, updated on 2026-11-06, said 85 of 115 with 2 failing. This
gate requires the badge's passed, total and failing counts to equal the
paragraph's.

Deprecated entry points
-----------------------
`serve()` and `serve_with_addr()` have been deprecated since 0.16.0 (neither can
be stopped). Until 2026-11-07 the introduction's feature list, the README's
feature table and the server crate's key-types table still offered them as the
way to start a listener. A paragraph, list item or table row of current prose
that names one of them in backticks, with parentheses, must say "deprecated";
the migration guides, the ADR summaries and the pitfalls pages describe what was
true and are not read.

The crates a page's program uses
--------------------------------
A page that shows a `[dependencies]` block naming an a2a crate and, beside it, a
complete program (one with a `fn main`, as rustdoc compiles it, hidden lines
included) gives a reader everything to type. The tool-loop page's minimal agent
used `tokio` and `serde_json` and its block named neither: a reader with that
page's dependency line alone got four compile errors (found 2026-11-07,
docs/audits/2026-11-07-adopter-walkthrough). For such a page, every one of
{tokio, serde_json, serde, uuid, futures_util, tracing_subscriber, hyper,
axum, tonic} that the program uses is named in a `toml` block of that page.

`--self-test` runs the three assertions above on small inputs with the values on each side
of every boundary (a count one off, a row that says "deprecated" next to one that does
not, a crate declared and not declared), so the checks are themselves checked; CI runs it
before the gate.

Exit codes: 0 every snippet names the current line or is allowlisted with a
reason, every entry matches a snippet, the manifest names this release's image,
the README's two TCK figures agree, no current prose offers a deprecated
entry point and every page's dependency block names the crates its program
uses; 1 a snippet is stale, an entry is stale,
the manifest's image is not this release's or the figures differ; 2 not run from
the repository root, or the allowlist is malformed.
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
MANIFEST = ROOT / "examples" / "deploy-agent" / "deployment.yaml"
MANIFEST_IMAGE = re.compile(r"image:\s*ghcr\.io/[\w./-]+/deploy-agent:(\S+)")

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


def full_version() -> str:
    """The workspace version, from the types crate's own manifest."""
    data = tomllib.loads(SOURCE_OF_TRUTH.read_text(encoding="utf-8"))
    return data["package"]["version"]


def current_line() -> str:
    """MAJOR.MINOR of the workspace, from the types crate's own manifest."""
    major, minor, *_ = full_version().split(".")
    return f"{major}.{minor}"


def manifest_problem() -> str | None:
    """Why the deploy-agent manifest does not name this release's image."""
    if not MANIFEST.exists():
        return f"{MANIFEST.relative_to(ROOT)} does not exist"
    tags = MANIFEST_IMAGE.findall(MANIFEST.read_text(encoding="utf-8"))
    version = full_version()
    if tags != [version]:
        return (
            f"{MANIFEST.relative_to(ROOT)} names the image tag(s) {tags or 'none'}; "
            f"it must name exactly one, ghcr.io/.../deploy-agent:{version}, the "
            "image the release workflow publishes for this version"
        )
    return None


README = ROOT / "README.md"
# `official%20TCK-83%2F114%20MUST%2C%205%20failing` (a shields.io path, URL-encoded)
BADGE = re.compile(r"official%20TCK-(\d+)%2F(\d+)%20MUST%2C%20(\d+)%20failing")
# `**85 of 115 MUST requirements pass and 2 fail**`
PROSE = re.compile(r"\*\*(\d+) of (\d+) MUST requirements pass and (\d+) fail\*\*")


def readme_figure_problem(text: str | None = None) -> str | None:
    """Why the README's badge and Project Status paragraph disagree, if they do."""
    text = README.read_text(encoding="utf-8") if text is None else text
    badge, prose = BADGE.findall(text), PROSE.findall(text)
    if len(badge) != 1 or len(prose) != 1:
        return (
            f"README.md: expected one TCK badge and one 'N of M MUST requirements pass and F "
            f"fail' sentence, found {len(badge)} and {len(prose)}; this gate's patterns "
            "(BADGE, PROSE) must follow the wording"
        )
    if badge[0] != prose[0]:
        b, p = badge[0], prose[0]
        return (
            f"README.md: the badge says {b[0]}/{b[1]} MUST, {b[2]} failing; the Project Status "
            f"paragraph says {p[0]} of {p[1]} pass and {p[2]} fail"
        )
    return None


DEPRECATED_ENTRY = re.compile(r"`(?:serve|serve_with_addr)\(\)`")
HISTORICAL = (
    "book/src/reference/upgrading/",
    "book/src/reference/adrs.md",
    "book/src/reference/pitfalls",
)


def deprecated_entry_problems(files: dict[str, str]) -> list[str]:
    """`path:line` of each block of prose that names a deprecated entry point without saying so.

    A block is a table row, a list item or a paragraph (a run of non-blank lines);
    a block that says "deprecated" anywhere is fine.
    """
    problems = []
    for path, text in sorted(files.items()):
        if not path.endswith(".md") or path.startswith(HISTORICAL):
            continue
        block, start = [], 0

        def flush() -> None:
            joined = "\n".join(block)
            hit = DEPRECATED_ENTRY.search(joined)
            if hit and "deprecated" not in joined.lower():
                problems.append(f"{path}:{start}: names {hit.group(0)} and does not say it is deprecated")

        for n, line in enumerate(text.splitlines() + [""], 1):
            # A table row and a list item each start a block of their own.
            if block and (not line.strip() or line.startswith("|") or re.match(r"\s*(?:[-*]|\d+\.)\s", line)):
                flush()
                block = []
            if line.strip():
                if not block:
                    start = n
                block.append(line)
    return problems


THIRD_PARTY = ("tokio", "serde_json", "serde", "uuid", "futures_util", "tracing_subscriber", "hyper", "axum", "tonic")
FENCE = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,})(?P<info>[^\n`]*)\n(?P<body>.*?)^(?P=indent)(?P=fence)[ \t]*$", re.M | re.S)


def dependency_block_problems(files: dict[str, str]) -> list[str]:
    """`path`: crates a page's complete program uses and no `toml` block of the page names."""
    problems = []
    for path, text in sorted(files.items()):
        if not path.endswith(".md") or path.startswith(HISTORICAL):
            continue
        toml = [m.group("body") for m in FENCE.finditer(text) if m.group("info").strip().startswith("toml")]
        shown = [t for t in toml if "[dependencies]" in t and re.search(rf"^{CRATES}\s*=", t, re.M)]
        if not shown:
            continue
        declared = {n.replace("-", "_") for t in toml for n in re.findall(r"^([A-Za-z0-9_-]+)\s*=", t, re.M)}
        for m in FENCE.finditer(text):
            tags = [x for x in re.split(r"[,\s]+", m.group("info").strip()) if x]
            if (tags and tags[0] != "rust") or any(x.startswith("ignore") for x in tags):
                continue
            # What rustdoc compiles includes the hidden `# ` lines; matching on the raw text sees
            # them (`# async fn main()` contains `fn main`), so nothing needs unhiding.
            body = m.group("body")
            if not re.search(r"\bfn main\b", body):
                continue
            code = re.sub(r"//[^\n]*", "", body)
            for crate in THIRD_PARTY:
                if re.search(rf"\b{crate}::", code) and crate not in declared:
                    line = text.count("\n", 0, m.start()) + 2
                    problems.append(f"{path}:{line}: the program uses `{crate}` and no `toml` block on the page names it")
    return problems


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


def self_test() -> None:
    """The three prose assertions, on inputs with a value either side of each boundary."""
    badge = "official%20TCK-{}%2F{}%20MUST%2C%20{}%20failing"

    def readme(b, p):
        return f"[x](https://img/{badge.format(*b)}) ... **{p[0]} of {p[1]} MUST requirements pass and {p[2]} fail**"

    assert readme_figure_problem(readme((85, 115, 2), (85, 115, 2))) is None
    for off in ((84, 115, 2), (85, 114, 2), (85, 115, 1), (85, 115, 3), (86, 115, 2)):
        assert "badge says" in readme_figure_problem(readme(off, (85, 115, 2))), off
    assert "expected one TCK badge" in readme_figure_problem("no figures here")
    only_badge = readme((85, 115, 2), (85, 115, 2)).split(" ... ")[0]
    only_prose = readme((85, 115, 2), (85, 115, 2)).split(" ... ")[1]
    assert "found 1 and 0" in readme_figure_problem(only_badge)
    assert "found 0 and 1" in readme_figure_problem(only_prose)
    assert "found 2 and 1" in readme_figure_problem(only_badge * 2 + " " + only_prose)
    assert "expected one TCK badge" in readme_figure_problem(readme((1, 2, 3), (1, 2, 3)) * 2)

    md = lambda text: {"a.md": text}  # noqa: E731
    assert deprecated_entry_problems(md("Use `serve()` to start.")) == [
        "a.md:1: names `serve()` and does not say it is deprecated"
    ]
    assert deprecated_entry_problems(md("`serve_with_addr()` is deprecated since 0.16.0.")) == []
    assert deprecated_entry_problems(md("| `serve()` | Deprecated: no shutdown |")) == []
    # a table row and a list item are blocks of their own: one row saying so does not excuse another
    two_rows = "| `serve()` | fine |\n| `serve_with_addr()` | Deprecated |"
    assert deprecated_entry_problems(md(two_rows)) == ["a.md:1: names `serve()` and does not say it is deprecated"]
    two_items = "- `serve()` starts it\n- the old `serve()` is deprecated"
    assert deprecated_entry_problems(md(two_items)) == ["a.md:1: names `serve()` and does not say it is deprecated"]
    assert deprecated_entry_problems(md("Use `serve` here.")) == []  # no parentheses: not the call
    assert deprecated_entry_problems({"book/src/reference/upgrading/x.md": "`serve()`"}) == []  # history
    assert deprecated_entry_problems({"x.rs": "`serve()`"}) == []

    deps = '```toml\n[dependencies]\na2a-protocol-sdk = "0.16"\n{}\n```\n'
    main = "```rust,no_run\n# #[tokio::main]\n# async fn main() {\nlet v = serde_json::json!(1);\n# }\n```\n"
    # the line is the block's first code line: the fence is line 6, its body starts on 7
    assert dependency_block_problems({"a.md": deps.format("") + main}) == [
        "a.md:7: the program uses `tokio` and no `toml` block on the page names it",
        "a.md:7: the program uses `serde_json` and no `toml` block on the page names it",
    ]
    assert dependency_block_problems({"a.md": deps.format('tokio = "1"\nserde_json = "1"') + main}) == []
    one = dependency_block_problems({"a.md": deps.format('tokio = "1"') + main})
    assert len(one) == 1 and "`serde_json`" in one[0], one
    assert dependency_block_problems({"a.md": main}) == []  # no dependency block shown: nothing to hold it to
    held = deps.format("") + main
    assert dependency_block_problems({"book/src/reference/upgrading/x.md": held}) == []  # history
    assert dependency_block_problems({"x.rs": held}) == []  # not Markdown
    hidden_only = "```rust,no_run\n# fn main() {\n# let v = serde_json::json!(1);\n# }\n```\n"
    assert len(dependency_block_problems({"a.md": deps.format("") + hidden_only})) == 1  # a hidden line is code too
    fragment = "```rust,no_run\nlet v = serde_json::json!(1);\n```\n"
    assert dependency_block_problems({"a.md": deps.format("") + fragment}) == []  # no fn main: a fragment
    ignored = "```rust,ignore\n# fn main() {\nlet v = serde_json::json!(1);\n# }\n```\n"
    assert dependency_block_problems({"a.md": deps.format("") + ignored}) == []
    commented = "```rust,no_run\nfn main() {\n    // serde_json::json!(1)\n}\n```\n"
    assert dependency_block_problems({"a.md": deps.format("") + commented}) == []
    print("check_doc_versions --self-test: the README figure, deprecated-entry and dependency-block assertions pass")


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        self_test()
        return 0
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

    problem = manifest_problem() or readme_figure_problem()
    if problem:
        print(f"check_doc_versions: {problem}")
        return 1

    pages = {p: (ROOT / p).read_text(encoding="utf-8") for p in tracked() if p.endswith(".md")}
    missing = dependency_block_problems(pages)
    if missing:
        print("check_doc_versions: a page's dependency block is missing crates its program uses\n")
        for m in missing:
            print(f"  {m}")
        print("\nA reader who types the page's [dependencies] and its program gets `unresolved import`.")
        return 1

    offered = deprecated_entry_problems(pages)
    if offered:
        print("check_doc_versions: current prose offers a deprecated entry point\n")
        for o in offered:
            print(f"  {o}")
        print("\nSay it is deprecated since 0.16.0 and point at Server::bind and serve_with_shutdown.")
        return 1

    print(
        f"check_doc_versions: {len(found)} dependency snippet(s) across "
        f"{len({s[0] for s in found})} file(s) name the current {line} line; "
        f"{len(entries)} allowlisted as historical; the deploy-agent manifest "
        f"names image {full_version()}; the README's two TCK figures agree; no current prose offers a deprecated entry point; every page's dependency block names the crates its program uses"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
