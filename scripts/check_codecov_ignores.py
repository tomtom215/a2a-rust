#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when a path `codecov.yml` says it ignores is still in the coverage report.

# What problem this solves

Until 2026-09-09 `codecov.yml` tried to exclude the PostgreSQL stores, because
they only execute against a live server and would otherwise report 0% forever.
The exclusion never worked, and the file recorded a fix that was applied and
never checked. Since 2026-09-09 those files are measured instead — the coverage
workflow runs the live-database suites under instrumentation — and the
remaining ignores are `tck/**`, `examples/**` and `itk/**`, none of them
library code. This script stays as the instrument for whatever is listed: an
entry Codecov is still counting is a failure. Since 2026-09-10 it runs weekly
in `coverage.yml` (`ignores-applied`); until then nothing ran it, so "the
exclusion is applied" was, again, a claim with no check behind it. Measured
that day: 3 patterns, 146 files in the report, no match. The history:

    # These five were previously listed as bare paths, and Codecov did not
    # apply them. Verified 2026-08-06 against Codecov's own API for `615d01f8`:
    # all five appear in the report (0.00%-8.06%) ... The three patterns that
    # *do* work here all contain a glob token, so these now carry one too.

The "before" was measured. The "after" was not. Measured 2026-08-19 against the
same API, all five are still in the report — and `postgres_config_store.rs` is
still at exactly the 8.06% that comment quotes. Two further PostgreSQL files
have appeared since and were never listed at all, and `store/postgres_store.rs`
became `store/postgres_store/mod.rs`, which the pattern naming the old path
could not match even if the pattern shape were right.

The cost is not cosmetic. Those seven files are 957 of 35,287 counted lines and
**881 of 2,096 missed lines — 42% of every uncovered line in the repository**.
The badge reads 94.06%; without them it is 96.46%. Every conversation about
where the coverage gaps are has been starting from the wrong number.

This is the same failure the file it checks already names: a number nothing
recomputes is a number that decays. So this recomputes it. (Those seven files
are no longer excluded; see codecov.yml for how the gap was closed.)

# What this checks, and what it does not

Every entry under `ignore:` in `codecov.yml` is matched against every file in
the current Codecov report. A match is a hard failure: the entry claims to
exclude something that is still counted.

It does **not** check the reverse — that everything absent from the report is
listed here. Codecov omits files for its own reasons and a missing file is not
evidence of a config error.

The glob-to-regex translation mirrors what Codecov's own validator returns for
these patterns (`POST https://codecov.io/validate`): `**` becomes `.*`, `*`
becomes `[^/]*`, and the whole pattern is anchored at both ends. If Codecov
changes that translation this check drifts, which is why the validator call is
part of `--explain`.

# Usage

    check_codecov_ignores.py [--repo owner/name] [--explain]

    CODECOV_REPORT_FILE=<tree.json>  read a saved report tree instead of the
                                     API (for the workflow-gate harness only)

Exit codes:
    0  no ignored path appears in the report
    1  an ignored path is still being counted
    2  the report or the config could not be read (never treated as agreement)
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

import yaml

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPO = "tomtom215/a2a-rust"
API = "https://api.codecov.io/api/v2/github/{owner}/repos/{repo}/report/tree?depth=12"
VALIDATE = "https://codecov.io/validate"


def glob_to_regex(pattern: str) -> re.Pattern[str]:
    """Translate a Codecov ignore glob the way Codecov's validator does.

    Checked against the validator's own output for this repository's patterns:
    `tck/**` -> `(?s:tck/.*)\\Z`, `**/store/pg_migration.rs` ->
    `(?s:.*/store/pg_migration\\.rs)\\Z`.
    """
    out: list[str] = []
    i = 0
    while i < len(pattern):
        if pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    return re.compile(f"(?s:{''.join(out)})\\Z")


def report_files(repo: str, report_file: str | None) -> list[tuple[str, int, int]]:
    """Every leaf file in the current Codecov report: (path, lines, misses).

    `report_file` (the `CODECOV_REPORT_FILE` knob) reads a saved copy of the
    same tree instead of the API. It exists so `prove_workflow_gates_fail.py`
    can hand this script a synthetic report and watch it fail; it is not for
    CI, where the live report is the only one that means anything.
    """
    if report_file:
        source = report_file
        try:
            data = json.loads(Path(report_file).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            print(f"check_codecov_ignores: could not read {source}: {exc}", file=sys.stderr)
            raise SystemExit(2) from exc
    else:
        owner, name = repo.split("/", 1)
        source = API.format(owner=owner, repo=name)
        try:
            with urllib.request.urlopen(source, timeout=60) as resp:  # noqa: S310
                data = json.load(resp)
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            print(f"check_codecov_ignores: could not read {source}: {exc}", file=sys.stderr)
            raise SystemExit(2) from exc

    leaves: list[tuple[str, int, int]] = []

    # A node carrying a `children` key is a directory even when the list is
    # empty; only a node without one is a file. Until 2026-09-10 an empty
    # directory was counted as a file named "" — so a report with no files
    # at all produced one phantom leaf and the "no files" check below never
    # fired. Found by `prove_workflow_gates_fail.py` the day the gate was
    # registered, which is the reason that harness exists.
    def walk(node: dict, prefix: str = "") -> None:
        path = f"{prefix}/{node['name']}".lstrip("/")
        children = node.get("children")
        if children is not None:
            for child in children:
                walk(child, path)
        elif path:
            leaves.append((path, node.get("lines") or 0, node.get("misses") or 0))

    for node in data if isinstance(data, list) else [data]:
        walk(node)
    if not leaves:
        print("check_codecov_ignores: the report has no files at all", file=sys.stderr)
        raise SystemExit(2)
    return leaves


def tracked_files() -> list[str]:
    """Every file git tracks, so a stale pattern can be told from a working one."""
    import subprocess

    try:
        out = subprocess.run(
            ["git", "ls-files"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"check_codecov_ignores: could not list tracked files: {exc}", file=sys.stderr)
        raise SystemExit(2) from exc
    return out.stdout.split()


def ignores() -> list[str]:
    path = REPO_ROOT / "codecov.yml"
    try:
        config = yaml.safe_load(path.read_text())
    except (OSError, yaml.YAMLError) as exc:
        print(f"check_codecov_ignores: could not read {path}: {exc}", file=sys.stderr)
        raise SystemExit(2) from exc
    entries = config.get("ignore") or []
    if not entries:
        print("check_codecov_ignores: codecov.yml has no ignore list", file=sys.stderr)
        raise SystemExit(2)
    return entries


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", default=DEFAULT_REPO)
    parser.add_argument(
        "--explain",
        action="store_true",
        help="also print Codecov's own translation of the ignore patterns",
    )
    args = parser.parse_args()

    patterns = [(entry, glob_to_regex(entry)) for entry in ignores()]
    leaves = report_files(args.repo, os.environ.get("CODECOV_REPORT_FILE"))

    violations: list[tuple[str, str, int, int]] = []
    for path, lines, misses in leaves:
        for entry, rx in patterns:
            if rx.match(path):
                violations.append((entry, path, lines, misses))
                break

    if args.explain:
        try:
            body = (REPO_ROOT / "codecov.yml").read_bytes()
            req = urllib.request.Request(VALIDATE, data=body, method="POST")
            with urllib.request.urlopen(req, timeout=60) as resp:  # noqa: S310
                print(resp.read().decode(), file=sys.stderr)
        except (urllib.error.URLError, TimeoutError) as exc:
            print(f"(validator unreachable: {exc})", file=sys.stderr)

    # A pattern that matches nothing in the report is either working or stale.
    # Tell them apart by asking the working tree: a pattern that matches files
    # on disk but none in the report is doing its job; one that matches nothing
    # anywhere names a path that no longer exists. `store/postgres_store.rs`
    # became `store/postgres_store/mod.rs` on 2026-08-14 and the pattern naming
    # the old path went stale in the same commit — a live-looking exemption that
    # exempts nothing, which is the failure `check_file_lengths.sh` treats as an
    # error for the same reason.
    tracked = tracked_files()
    stale = [
        entry
        for entry, rx in patterns
        if not any(rx.match(path) for path, _, _ in leaves)
        and not any(rx.match(path) for path in tracked)
    ]

    if violations:
        counted = sum(lines for _, _, lines, _ in violations)
        missed = sum(misses for _, _, _, misses in violations)
        total_lines = sum(l for _, l, _ in leaves)
        total_missed = sum(m for _, _, m in leaves)
        print(
            "\ncodecov.yml claims to ignore paths that are still being counted:\n",
            file=sys.stderr,
        )
        for entry, path, lines, misses in sorted(violations, key=lambda v: -v[3]):
            print(f"    {misses:>5} missed of {lines:>5}  {path}", file=sys.stderr)
            print(f"          matched by  {entry}", file=sys.stderr)
        without = 100.0 * (total_lines - counted - (total_missed - missed)) / (
            total_lines - counted
        )
        print(
            f"\n  {missed} of {total_missed} missed lines ({100 * missed / total_missed:.0f}%) "
            f"come from paths this file says are excluded.\n"
            f"  Reported coverage {100 * (total_lines - total_missed) / total_lines:.2f}%; "
            f"without them {without:.2f}%.\n\n"
            "  Codecov accepts these patterns — `POST https://codecov.io/validate`\n"
            "  compiles them without complaint — and does not apply them. Do not\n"
            "  assume a new pattern shape works because it validates; re-run this\n"
            "  after the next upload, which is the only thing that settles it.\n",
            file=sys.stderr,
        )
        return 1

    if stale:
        print(
            "\ncodecov.yml ignores paths that no longer exist:\n", file=sys.stderr
        )
        for entry in stale:
            print(f"    {entry}", file=sys.stderr)
        print(
            "\n  Nothing in the report matches these, and nothing in the working\n"
            "  tree does either — so they are not working exclusions, they are\n"
            "  exclusions for files that have moved or gone. Point them at the\n"
            "  current path or delete them; an entry that exempts nothing reads\n"
            "  as a live decision and is not one.\n",
            file=sys.stderr,
        )
        return 1

    print(
        f"check_codecov_ignores: {len(patterns)} ignore pattern(s), none of them "
        f"matches any of the {len(leaves)} files in the report, and each still "
        f"names a path that exists"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
