#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check that a release tag publishes what its release notes describe.

`v0.13.0` did not (audit N7, `docs/adopter-audit-2026-09-22.md`). The tag
landed on `391f0df`, the merge of #138, rather than on the release
preparation that wrote the 0.13.0 section. The crates were built from that
later tree, so they shipped nineteen changes — one of them breaking — that
the tagged `CHANGELOG.md` still listed under `[Unreleased]`, and that the
GitHub release notes, extracted from the 0.13.0 section, did not mention.
Every gate `release.yml` ran was green, because none of them asked the
question. This script asks it four ways, one subcommand each:

  unreleased  The tagged tree's `## [Unreleased]` section is empty, or holds
              only the placeholder `Nothing yet.` that release preparation
              leaves behind.

  prep        The tag is the release-preparation commit. P is the newest
              non-merge commit at or before the tag that changed the text of
              the release's own `## [X.Y.Z]` section. Every file that is
              packaged into a published crate must be identical at P and at
              the tag, apart from the crates' own version strings. So nothing
              that ships changed after the notes were last written: a change
              that lands later has to touch the notes, which puts it in front
              of whoever writes them. The provenance manifest, which cannot pin
              the commit that adds it, is outside the packaged paths anyway.

  cadence     `STABILITY.md` section 3: a patch release carries no breaking
              change, and at most one breaking minor ships per calendar month.
              "Breaking" is what the CHANGELOG says — a `### Breaking Changes`
              heading in the release's section — which is also where the
              release notes come from.

  vcs         Every packaged `.crate` records, in `.cargo_vcs_info.json`, that
              it was built from the tagged commit, from a clean tree. That is
              the file the N7 investigation had to read by hand to learn what
              0.13.0 was built from.

Measured against this repository's own tags on 2026-09-23 (see the
`--history` mode and `docs/adopter-audit-2026-09-22.md`, N10): `prep` fails
every tag from `v0.10.0` to `v0.13.0`, and `cadence` fails `v0.13.0`.

Exit 0 when the check holds, 1 when it does not, 2 on a usage or repository
error — never 0 on an error, so a check that could not run cannot pass.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tarfile
from pathlib import Path

PUBLISHED = (
    "crates/a2a-protocol-types",
    "crates/a2a-protocol-client",
    "crates/a2a-protocol-server",
    "crates/a2a-protocol-sdk",
)
# The root manifest is packaged in effect: each crate inherits
# `rust-version`, `edition` and `license` from it, and cargo writes the
# inherited values into the published `Cargo.toml`.
PACKAGED = (*PUBLISHED, "Cargo.toml")

PLACEHOLDER = "Nothing yet."
# `**Cadence exception:** <reason>` in a release's CHANGELOG section. The
# reason must say something: twenty characters is enough for "critical fix
# for ..." and too many for a bare "yes" or "n/a".
CADENCE_EXCEPTION = re.compile(r"(?m)^\*\*Cadence exception:\*\*[ \t]*(?P<reason>\S.{19,})$")
HEADING = re.compile(r"^## \[(?P<ver>[^\]]+)\](?: - (?P<date>\d{4}-\d{2}-\d{2}))?", re.M)
# A crate's own version and its pins on sibling crates — the strings release
# preparation is expected to change, and nothing else.
VERSION_STRINGS = (
    re.compile(r'(?m)^(version\s*=\s*)"[^"]*"'),
    re.compile(r'(a2a-protocol-[a-z]+\s*=\s*\{\s*version\s*=\s*)"[^"]*"'),
)


class Usage(Exception):
    """A problem with the invocation or the repository, not with the release."""


def git(repo: Path, *args: str, check: bool = True) -> str:
    r = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True)
    if check and r.returncode != 0:
        raise Usage(f"git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def show(repo: Path, rev: str, path: str) -> str | None:
    r = subprocess.run(
        ["git", "-C", str(repo), "show", f"{rev}:{path}"], capture_output=True, text=True
    )
    return r.stdout if r.returncode == 0 else None


def section(changelog: str, version: str) -> str | None:
    """The body of `## [version]`, heading included; None if absent."""
    heads = list(HEADING.finditer(changelog))
    for i, m in enumerate(heads):
        if m.group("ver") == version:
            end = heads[i + 1].start() if i + 1 < len(heads) else len(changelog)
            return changelog[m.start():end]
    return None


def version_of(tag: str) -> str:
    v = tag[1:] if tag.startswith("v") else tag
    if not re.fullmatch(r"\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?", v):
        raise Usage(f"{tag!r} is not a vX.Y.Z tag")
    return v


def check_unreleased(repo: Path, rev: str) -> list[str]:
    text = show(repo, rev, "CHANGELOG.md")
    if text is None:
        raise Usage(f"CHANGELOG.md is not in {rev}")
    body = section(text, "Unreleased")
    if body is None:
        return []
    lines = [ln.strip() for ln in body.splitlines()[1:] if ln.strip()]
    if not lines or lines == [PLACEHOLDER]:
        return []
    entries = sum(1 for ln in lines if ln.startswith("- "))
    return [
        f"the tagged CHANGELOG.md has {entries} entr{'y' if entries == 1 else 'ies'} "
        f"under ## [Unreleased] ({len(lines)} non-blank lines). They ship in this "
        "release and its notes will not mention them: move them into the release's "
        f"section, or tag the commit that does not contain them. First line: {lines[0][:100]}"
    ]


def normalise(path: str, text: str) -> str:
    if path.endswith("Cargo.toml"):
        for rx in VERSION_STRINGS:
            text = rx.sub(r'\1"*"', text)
    return text


def notes_commit(repo: Path, rev: str, version: str) -> str:
    """The newest non-merge commit at or before `rev` that changed the section."""
    log = git(repo, "log", "--no-merges", "--format=%H %P", rev, "--", "CHANGELOG.md")
    for line in log.splitlines():
        sha, *parents = line.split()
        now = section(show(repo, sha, "CHANGELOG.md") or "", version)
        before = section(show(repo, parents[0], "CHANGELOG.md") or "", version) if parents else None
        if now is not None and now != before:
            return sha
    raise Usage(f"no commit at or before {rev} writes a ## [{version}] section")


def check_prep(repo: Path, tag: str, rev: str) -> list[str]:
    version = version_of(tag)
    p = notes_commit(repo, rev, version)
    changed = git(repo, "diff", "--name-only", p, rev, "--", *PACKAGED).split()
    drift = [
        f for f in changed
        if normalise(f, show(repo, p, f) or "") != normalise(f, show(repo, rev, f) or "")
    ]
    if not drift:
        return []
    later = git(repo, "log", "--no-merges", "--format=%h %s", f"{p}..{rev}", "--", *drift)
    return [
        f"{tag} is not the release-preparation commit. The {version} notes were last "
        f"written at {p[:10]}, and {len(drift)} packaged file(s) changed after that, "
        "so the crates ship code the notes were not written against:",
        *(f"  {f}" for f in drift[:20]),
        *([f"  … and {len(drift) - 20} more"] if len(drift) > 20 else []),
        "  Commits that changed them after the notes:",
        *(f"    {c}" for c in later.splitlines()[:20]),
    ]


def releases(changelog: str) -> list[tuple[str, str | None, bool]]:
    """(version, date, has breaking heading) for every release section."""
    out = []
    for m in HEADING.finditer(changelog):
        if m.group("ver") == "Unreleased":
            continue
        body = section(changelog, m.group("ver")) or ""
        out.append((m.group("ver"), m.group("date"), "\n### Breaking Changes" in body))
    return out


def check_cadence(repo: Path, tag: str, rev: str) -> list[str]:
    version = version_of(tag)
    text = show(repo, rev, "CHANGELOG.md")
    if text is None:
        raise Usage(f"CHANGELOG.md is not in {rev}")
    rows = releases(text)
    mine = [r for r in rows if r[0] == version]
    if not mine:
        raise Usage(f"no ## [{version}] section in the tagged CHANGELOG.md")
    _, date, breaking = mine[0]
    if not breaking:
        return []
    major, minor, patch = (int(x) for x in version.split("-")[0].split("."))
    problems = []
    if patch != 0 or (major >= 1 and minor != 0):
        kind = "patch" if patch != 0 else "minor"
        problems.append(
            f"{version} is a {kind} release with a '### Breaking Changes' section. "
            "STABILITY.md section 2 allows breaking changes only in a "
            f"{'minor (0.x) or major' if major == 0 else 'major'} release."
        )
    if date is None:
        raise Usage(f"## [{version}] is undated")
    # A pre-release and the final of the same version are one breaking
    # release, not two.
    base = version.split("-")[0]
    clash = [v for v, d, b in rows if b and v.split("-")[0] != base and d and d[:7] == date[:7]]
    # STABILITY.md section 3's exception: the release's own notes may declare
    # that it cannot wait for the next month (a critical fix that needs a
    # break), with the reason, so the exception is deliberate and visible to
    # adopters in the release notes rather than a gate someone switched off.
    exception = CADENCE_EXCEPTION.search(section(text, version) or "")
    if clash and exception:
        print(f"cadence exception declared for {version} (same month as "
              f"{', '.join(clash)}): {exception.group('reason').strip()}")
    elif clash:
        problems.append(
            f"{version} ({date}) is a breaking release, and so is {', '.join(clash)} in "
            f"the same calendar month. STABILITY.md section 3 allows at most one "
            "breaking minor per calendar month: batch the breaks, wait for the "
            "next month, or declare an exception in the release's notes with a "
            "line '**Cadence exception:** <why this cannot wait>'."
        )
    return problems


def check_vcs(rev: str, crates: list[Path]) -> list[str]:
    if not crates:
        raise Usage("no .crate files given")
    problems = []
    for crate in crates:
        with tarfile.open(crate, "r:gz") as tar:
            info = next((m for m in tar.getmembers() if m.name.endswith("/.cargo_vcs_info.json")), None)
            if info is None:
                problems.append(f"{crate.name}: no .cargo_vcs_info.json (packaged outside git, or with --allow-dirty?)")
                continue
            data = json.load(tar.extractfile(info))
        sha = data.get("git", {}).get("sha1", "")
        before = len(problems)
        if sha != rev:
            problems.append(f"{crate.name}: built from {sha or '(none)'}, not the tagged commit {rev}")
        if data.get("git", {}).get("dirty"):
            problems.append(f"{crate.name}: built from a dirty working tree")
        if len(problems) == before:
            print(f"ok  {crate.name}: built from {sha[:10]}")
    return problems


def history(repo: Path) -> int:
    tags = git(repo, "tag", "--list", "v*", "--sort=v:refname").split()
    for tag in tags:
        rev = git(repo, "rev-parse", f"{tag}^{{commit}}").strip()
        verdicts = []
        for name, fn in (("unreleased", lambda: check_unreleased(repo, rev)),
                         ("prep", lambda: check_prep(repo, tag, rev)),
                         ("cadence", lambda: check_cadence(repo, tag, rev))):
            try:
                verdicts.append(f"{name}={'FAIL' if fn() else 'ok'}")
            except Usage as e:
                verdicts.append(f"{name}=error({str(e)[:60]})")
        print(f"{tag:10} {rev[:10]}  " + "  ".join(verdicts))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", type=Path, default=Path.cwd())
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("unreleased", "prep", "cadence"):
        s = sub.add_parser(name)
        s.add_argument("tag")
        s.add_argument("rev")
    v = sub.add_parser("vcs")
    v.add_argument("rev")
    v.add_argument("crates", nargs="*", type=Path)
    sub.add_parser("history", help="run the tree checks over every v* tag and report")
    a = ap.parse_args()
    try:
        if a.cmd == "history":
            return history(a.repo)
        if a.cmd == "vcs":
            problems = check_vcs(a.rev, a.crates)
        else:
            rev = git(a.repo, "rev-parse", f"{a.rev}^{{commit}}").strip()
            fn = {"unreleased": check_unreleased, "prep": check_prep, "cadence": check_cadence}[a.cmd]
            problems = fn(a.repo, rev) if a.cmd == "unreleased" else fn(a.repo, a.tag, rev)
    except Usage as e:
        print(f"::error::check_release_tree {a.cmd}: {e}")
        return 2
    for p in problems:
        print(f"::error::{p}" if not p.startswith("  ") else p)
    if not problems:
        print(f"check_release_tree {a.cmd}: ok")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
