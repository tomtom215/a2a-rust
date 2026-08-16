#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check that every `publish = false` workspace member is excluded from packaging.

`cargo package --workspace` refuses any crate that depends on a sibling by bare
`path` with no `version`, because the packaged manifest drops the path and would
resolve against crates.io instead. Every `publish = false` member here does
exactly that, so each one must be named in the `--exclude` list.

That list is duplicated in three places — `.github/workflows/ci.yml`,
`.github/workflows/release.yml` and `RELEASING.md`. Nothing ties it to the
workspace, so adding an example crate breaks packaging in CI and, worse, in the
release workflow, at the point where the failure costs the most.

This is a real regression, not a hypothetical: `hello-agent`, `deploy-agent` and
`a2a-book-tests` were all added to the workspace without being added to the
list, and `cargo package --workspace` failed on the first of them.

Exit 0 if every list covers exactly the publishable/non-publishable split,
non-zero with the missing names otherwise.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Each site that carries a copy of the exclude list.
SITES = [
    Path(".github/workflows/ci.yml"),
    Path(".github/workflows/release.yml"),
    Path("RELEASING.md"),
]


def workspace_members() -> list[tuple[str, bool]]:
    """Return (name, publishable) for every workspace member.

    Uses `cargo metadata` so the answer follows the real member list rather than
    a glob that could drift from `[workspace] members`.
    """
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    import json

    meta = json.loads(out)
    members = []
    for pkg in meta["packages"]:
        manifest = Path(pkg["manifest_path"])
        data = tomllib.loads(manifest.read_text())
        # cargo metadata reports `publish: null` for unrestricted, [] for false.
        publishable = data.get("package", {}).get("publish", True) is not False
        members.append((pkg["name"], publishable))
    return sorted(members)


def excludes_in(text: str) -> set[str]:
    return set(re.findall(r"--exclude\s+([A-Za-z0-9_-]+)", text))


def main() -> int:
    members = workspace_members()
    must_exclude = {name for name, publishable in members if not publishable}
    publishable = {name for name, publishable in members if publishable}

    failures: list[str] = []
    for site in SITES:
        path = ROOT / site
        if not path.exists():
            failures.append(f"{site}: missing")
            continue
        text = path.read_text()
        if "--exclude" not in text:
            failures.append(f"{site}: no --exclude list found")
            continue
        listed = excludes_in(text)

        missing = sorted(must_exclude - listed)
        if missing:
            failures.append(
                f"{site}: `publish = false` member(s) not excluded: "
                + ", ".join(missing)
            )

        # Excluding a publishable crate would silently drop it from the release.
        wrongly = sorted(listed & publishable)
        if wrongly:
            failures.append(
                f"{site}: publishable crate(s) wrongly excluded: " + ", ".join(wrongly)
            )

        unknown = sorted(listed - must_exclude - publishable)
        if unknown:
            failures.append(
                f"{site}: --exclude names non-member(s): " + ", ".join(unknown)
            )

    if failures:
        print("check_package_excludes: packaging exclude lists are out of sync\n")
        for f in failures:
            print(f"  {f}")
        print(
            "\n`cargo package --workspace` rejects any crate depending on a sibling\n"
            "by bare `path` with no `version`, which every `publish = false` member\n"
            "here does. Add the missing name(s) to the --exclude list at each site\n"
            "above, or the release workflow fails at packaging time."
        )
        return 1

    print(
        f"check_package_excludes: {len(publishable)} publishable, "
        f"{len(must_exclude)} excluded, {len(SITES)} sites agree"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
