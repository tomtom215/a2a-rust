#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fail when a published crate's manifest admits a version with a RustSec advisory.

`cargo deny check` and `cargo audit` read *this repository's* lockfile. A
consumer never sees that lockfile: cargo resolves our crates against the
consumer's own, and keeps whatever version of a dependency it already holds as
long as our manifest's requirement admits it. So a dependency fixed here by a
lockfile bump is still vulnerable for every consumer who does not bump it
themselves, and bumping an a2a crate does not help them — the requirement
never moved.

That was measured, not supposed. An adopter on 0.12.1 found that
`cargo update -p a2a-protocol-client` left rustls at 0.23.43, inside
RUSTSEC-2026-0285, because our requirement was `>=0.23, <0.24`. On
2026-09-24 the manifests admitted versions under seven advisories across six
dependencies (bytes, ring, rustls, sqlx, time, tokio), while both deny jobs
were green (N24 in `docs/adopter-audit-2026-09-22.md`).

What this checks
----------------
For each normal and build dependency of the four published crates — the
dependencies a consumer compiles — every published, non-yanked version the
requirement admits is tested against every advisory for that crate in the
RustSec database. Any admitted version that is neither patched nor unaffected
fails the check, with the advisory, the affected range and the patched
floor. Informational advisories (`unsound`, `unmaintained`) count too: an
unsound release is worth a floor as much as a vulnerable one.

Dev-dependencies are out of scope, since no consumer compiles them, and so
is `bindings/a2a-protocol-slimrpc`, which has never been published.

Inputs: the RustSec database (cloned fresh unless `--db` names a checkout),
and the crates.io API for the list of published versions. Both are live, so
a new advisory can turn this red with no change here — as it turns
`cargo deny check` red. That is the point of both.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import tomllib
import urllib.request

PUBLISHED = ["a2a-protocol-types", "a2a-protocol-client", "a2a-protocol-server", "a2a-protocol-sdk"]
SECTIONS = ["dependencies", "build-dependencies"]
USER_AGENT = "a2a-rust check_advisory_floors (https://github.com/tomtom215/a2a-rust)"

Version = tuple[int, int, int]


def parse_version(text: str) -> Version:
    core = text.split("-")[0].split("+")[0]
    parts = [int(x) for x in core.split(".")]
    return tuple((parts + [0, 0, 0])[:3])  # type: ignore[return-value]


def matches(version: Version, requirement: str) -> bool:
    """Cargo's requirement grammar, for the comparators these manifests and
    the advisory database use: `>=`, `>`, `<`, `<=`, `=`, `^` and a bare
    version (which is `^`), comma-separated."""
    for clause in requirement.split(","):
        clause = clause.strip()
        m = re.fullmatch(r"(>=|<=|>|<|=|\^|~)?\s*([0-9][0-9.]*)(?:-[0-9A-Za-z.\-]+)?", clause)
        if not m:
            sys.exit(f"check_advisory_floors: cannot parse requirement clause {clause!r}")
        op, bound = m.group(1) or "^", parse_version(m.group(2))
        given = m.group(2).count(".") + 1
        if op == ">=" and not version >= bound:
            return False
        if op == ">" and not version > bound:
            return False
        if op == "<" and not version < bound:
            return False
        if op == "<=" and not version <= bound:
            return False
        if op == "=" and version[:given] != bound[:given]:
            return False
        if op == "~":
            if version < bound or version[: min(given, 2)] != bound[: min(given, 2)]:
                return False
        if op == "^":
            if version < bound:
                return False
            if bound[0] > 0 or given == 1:
                if version[0] != bound[0]:
                    return False
            elif bound[1] > 0 or given == 2:
                if version[:2] != bound[:2]:
                    return False
            elif version != bound:
                return False
    return True


def requirements(root: pathlib.Path) -> dict[str, set[tuple[str, str]]]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["dependencies"]
    found: dict[str, set[tuple[str, str]]] = {}
    for crate in PUBLISHED:
        manifest = tomllib.loads((root / "crates" / crate / "Cargo.toml").read_text())
        for section in SECTIONS:
            for name, spec in manifest.get(section, {}).items():
                if isinstance(spec, dict) and spec.get("path"):
                    continue  # a sibling crate, versioned by the release
                if isinstance(spec, dict) and spec.get("workspace"):
                    spec = workspace[name]
                req = spec if isinstance(spec, str) else spec.get("version")
                if not req:
                    sys.exit(f"check_advisory_floors: {crate} {section}.{name} has no version")
                package = spec.get("package", name) if isinstance(spec, dict) else name
                found.setdefault(package, set()).add((crate, req))
    return found


def advisories(db: pathlib.Path) -> dict[str, list[dict]]:
    by_crate: dict[str, list[dict]] = {}
    files = list((db / "crates").glob("*/*.md"))
    if not files:
        sys.exit(f"check_advisory_floors: no advisories under {db}/crates — wrong path?")
    for path in files:
        front = path.read_text().split("```toml", 1)[1].split("```", 1)[0]
        advisory = tomllib.loads(front)
        if advisory["advisory"].get("withdrawn"):
            continue
        by_crate.setdefault(advisory["advisory"]["package"], []).append(advisory)
    return by_crate


def published_versions(name: str) -> list[str]:
    url = f"https://crates.io/api/v1/crates/{name}/versions?per_page=100"
    out: list[str] = []
    while url:
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=60) as response:
            page = json.load(response)
        out += [v["num"] for v in page["versions"] if not v["yanked"] and "-" not in v["num"]]
        nxt = page.get("meta", {}).get("next_page")
        url = f"https://crates.io/api/v1/crates/{name}/versions{nxt}" if nxt else ""
    if not out:
        sys.exit(f"check_advisory_floors: crates.io lists no versions of {name}")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path("."))
    parser.add_argument("--db", type=pathlib.Path, help="a RustSec advisory-db checkout")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as scratch:
        db = args.db
        if db is None:
            db = pathlib.Path(scratch) / "advisory-db"
            subprocess.run(
                ["git", "clone", "--quiet", "--depth", "1",
                 "https://github.com/rustsec/advisory-db", str(db)],
                check=True,
            )
        known = advisories(db)
        reqs = requirements(args.root)
        if not reqs:
            sys.exit("check_advisory_floors: found no dependencies to check")

        failures = 0
        checked = 0
        for name in sorted(reqs):
            if name not in known:
                continue
            versions = published_versions(name)
            for crate, req in sorted(reqs[name]):
                admitted = [v for v in versions if matches(parse_version(v), req)]
                checked += 1
                for advisory in known[name]:
                    ranges = advisory.get("versions", {})
                    safe = ranges.get("patched", []) + ranges.get("unaffected", [])
                    bad = sorted(
                        (v for v in admitted if not any(matches(parse_version(v), r) for r in safe)),
                        key=parse_version,
                    )
                    if bad:
                        failures += 1
                        kind = advisory["advisory"].get("informational", "vulnerability")
                        print(
                            f"{crate}: `{name} = {req!r}` admits {len(bad)} version(s) under "
                            f"{advisory['advisory']['id']} ({kind}), {bad[0]} to {bad[-1]}; "
                            f"patched: {', '.join(ranges.get('patched', [])) or 'none'}"
                        )
        print(
            f"{sum(len(u) for u in reqs.values())} requirement(s) on {len(reqs)} crate(s); "
            f"{checked} have advisories on file; {failures} requirement/advisory pair(s) admit an affected version"
        )
        return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
