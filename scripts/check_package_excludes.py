#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check that every `publish = false` workspace member is excluded from packaging.

`cargo package --workspace` refuses any crate that depends on a sibling by bare
`path` with no `version`, because the packaged manifest drops the path and would
resolve against crates.io instead. Every `publish = false` member here does
exactly that, so each one must be named in the `--exclude` list.

That list is duplicated in three files — `.github/workflows/ci.yml`,
`.github/workflows/release.yml` and `RELEASING.md` — and *five* times in total,
because `release.yml` carries three separate `cargo package --workspace`
invocations. Nothing ties any of them to the workspace, so adding an example
crate breaks packaging in CI and, worse, in the release workflow, at the point
where the failure costs the most.

This is a real regression, not a hypothetical: `hello-agent`, `deploy-agent` and
`a2a-book-tests` were all added to the workspace without being added to the
list, and `cargo package --workspace` failed on the first of them.

Per invocation, not per file
----------------------------
Each `cargo package --workspace` command is checked against the workspace on
its own. It used to be one `re.findall` over the whole file text compared as a
set-union, which meant a name dropped from *one* of `release.yml`'s three
invocations still passed — the other two supplied it to the union. The command
that actually runs is one of the three, so the union is not the thing that has
to be right; each list is. And the failure that gets through lands in the
release workflow, after the tag is pushed, which is the exact cost the
paragraph above says this file exists to prevent.

A file with no `cargo package --workspace` invocation at all is a failure too,
not a pass with nothing to check: the command was renamed or removed, and a
checker that finds no work to do and reports success is the defect class this
repository keeps rediscovering.

Exit 0 if every invocation's list covers exactly the publishable/non-publishable
split, non-zero with the missing names and the invocation they are missing from
otherwise.

The licence files
-----------------
The same exit code also requires that every publishable crate's directory holds
a `LICENSE` and a `NOTICE` that are byte-for-byte the repository's. Cargo
packages the crate's own directory, so a file that sits only at the repository
root is in no `.crate`: for 0.16.0's preparation the four tarballs carried the
manifest's `license = "Apache-2.0"` and no licence text (Apache-2.0 §4(a) and
§4(d)). Found on 2026-11-07 by unpacking them (docs/audits/2026-11-07-adopter-walkthrough).
Copy, do not symlink: a copy is what the tarball holds, and this check says when
one has drifted.
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


def workspace_members() -> list[tuple[str, bool, Path]]:
    """Return (name, publishable, directory) for every workspace member.

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
        members.append((pkg["name"], publishable, manifest.parent))
    return sorted(members)


# One `cargo package`/`cargo publish` command, including any backslash
# continuations.
#
# Anchored on the command rather than on `--exclude`, so that an invocation
# which has *lost* its exclude list is still found — an invocation nothing
# matches is an invocation nothing checks.
#
# The command must start its line, optionally after a YAML `run:` key. All
# three files talk *about* `cargo package --workspace` in prose as well as
# running it, and an unanchored pattern matched the sentences too: two ci.yml
# comments and a RELEASING.md paragraph were each reported as an invocation
# missing all seventeen excludes. A backtick, a `#` or any other prose
# character before the command is what tells the two apart.
INVOCATION = re.compile(
    r"^[ \t]*(?:-[ \t]+)?(?:run:[ \t]*)?"
    r"(cargo[ \t]+(?:package|publish)\b[^\n]*(?:\\\n[^\n]*)*)",
    re.M,
)


def excludes_in(text: str) -> set[str]:
    return set(re.findall(r"--exclude\s+([A-Za-z0-9_-]+)", text))


def invocations(text: str) -> list[tuple[int, str]]:
    """Every workspace-wide packaging command, as (line number, command).

    `--workspace` is the filter: `cargo publish -p "$crate"` in release.yml
    packages one named crate and has no exclude list to get wrong, so requiring
    one of it would be a false failure that teaches people to widen the regex.
    """
    found = []
    for m in INVOCATION.finditer(text):
        cmd = m.group(1)
        if "--workspace" not in cmd:
            continue
        found.append((text.count("\n", 0, m.start()) + 1, cmd))
    return found


# What the Apache-2.0 licence asks a redistributor to pass on: its text (§4(a))
# and the project's NOTICE (§4(d)). Cargo packages a crate's own directory, and
# the repository's copies sit above every crate, so a crate that does not carry
# its own is published without either: the manifest says `Apache-2.0` and the
# `.crate` holds no licence text. Found on 2026-11-07 by unpacking the four
# tarballs of 0.16.0 (docs/audits/2026-11-07-adopter-walkthrough).
LICENCE_FILES = ("LICENSE", "NOTICE")


def licence_failures(members: list[tuple[str, bool, Path]]) -> list[str]:
    """One line per publishable crate whose LICENSE or NOTICE is absent or differs."""
    failures = []
    for name, publishable, directory in members:
        if not publishable:
            continue
        for fname in LICENCE_FILES:
            root_copy = ROOT / fname
            crate_copy = directory / fname
            where = crate_copy.relative_to(ROOT)
            if not crate_copy.is_file():
                failures.append(
                    f"{where}: missing, so the `{name}` tarball carries no {fname}; "
                    f"copy the repository's {fname}"
                )
            elif crate_copy.read_bytes() != root_copy.read_bytes():
                failures.append(
                    f"{where}: differs from the repository's {fname}; "
                    f"copy it again (cp {fname} {where})"
                )
    return failures


def main() -> int:
    members = workspace_members()
    must_exclude = {name for name, publishable, _ in members if not publishable}
    publishable = {name for name, publishable, _ in members if publishable}

    failures: list[str] = []
    checked = 0
    for site in SITES:
        path = ROOT / site
        if not path.exists():
            failures.append(f"{site}: missing")
            continue
        text = path.read_text()
        sites_invocations = invocations(text)
        if not sites_invocations:
            failures.append(
                f"{site}: no `cargo package --workspace` invocation found — "
                "the command was renamed or removed, and this file is being "
                "reported clean without anything being checked"
            )
            continue

        for line, cmd in sites_invocations:
            checked += 1
            where = f"{site}:{line}"
            listed = excludes_in(cmd)

            missing = sorted(must_exclude - listed)
            if missing:
                failures.append(
                    f"{where}: `publish = false` member(s) not excluded: "
                    + ", ".join(missing)
                )

            # Excluding a publishable crate would silently drop it from the
            # release.
            wrongly = sorted(listed & publishable)
            if wrongly:
                failures.append(
                    f"{where}: publishable crate(s) wrongly excluded: "
                    + ", ".join(wrongly)
                )

            unknown = sorted(listed - must_exclude - publishable)
            if unknown:
                failures.append(
                    f"{where}: --exclude names non-member(s): " + ", ".join(unknown)
                )

    licence = licence_failures(members)

    if failures or licence:
        print("check_package_excludes: packaging is out of sync\n")
        for f in failures + licence:
            print(f"  {f}")
        if failures:
            print(
                "\n`cargo package --workspace` rejects any crate depending on a sibling\n"
                "by bare `path` with no `version`, which every `publish = false` member\n"
                "here does. Add the missing name(s) to the --exclude list of each\n"
                "invocation named above — each one is a command that runs on its own,\n"
                "so fixing one of release.yml's three does not fix the other two — or\n"
                "the release workflow fails at packaging time, after the tag is pushed."
            )
        if licence:
            print(
                "\nCargo packages a crate's own directory: a LICENSE or NOTICE that is\n"
                "only at the repository root is in no published tarball. Copy them into\n"
                "each crate named above (Apache-2.0 §4(a), §4(d))."
            )
        return 1

    print(
        f"check_package_excludes: {len(publishable)} publishable, "
        f"{len(must_exclude)} excluded, {checked} invocation(s) across "
        f"{len(SITES)} file(s) agree; each publishable crate carries "
        f"{' and '.join(LICENCE_FILES)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
