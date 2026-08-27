#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Package the SLIMRPC binding, tolerating the one state `cargo package` cannot.

The binding lives outside the workspace and depends on the SDK crates by
`version` *and* `path`. Locally the path wins, so its build, clippy and test
steps are green against the in-tree crates. `cargo package` strips the path, so
the `version` requirement resolves against the crates.io index instead — and
during a release that index does not yet carry the version being prepared.

That makes one commit in every release cycle unpassable, in both directions:

    pin      in-tree   build / clippy / test        cargo package
    ^0.10    0.10.0    pass                         fails — 0.10.0 not published
    ^0.9     0.10.0    fails — didn't match 0.10.0  fails

Reverting the pin does not rescue it; it breaks the build too. A range would
admit both and is refused where the pin is declared, because these are *public*
dependencies — a consumer resolving the binding against an older SDK is the
failure the tight pin exists to prevent. RELEASING.md bumps the pin only after
publication, but the SDK version bump that must precede the tag is the same
commit that puts the binding out of resolution, so there is no green tree in
between. See docs/v0.9.0-post-release-review.md, B23.

This wrapper teaches the step the single state it cannot otherwise verify —

    every pin names the version that is in the tree, and the version cargo
    could not resolve is absent from the index

— and nothing else. A typo'd pin naming a version that is neither in-tree nor
published still fails, because such a pin does not match the in-tree crate,
which is the condition checked here.

Three properties are deliberate:

  * **The skip is not a blind spot.** `cargo package --list` runs every check
    except registry resolution — it still rejects a missing `readme`, a bad
    `exclude`, an unreadable target (all three measured). It runs on the skip
    path, so the release window loses registry resolution alone rather than the
    whole gate.

  * **A failed index query fails the gate.** The skip requires positive proof
    that the version is absent; an unreachable index proves nothing. The query
    happens only after `cargo package` has already failed, having reached that
    same index to do so — so this cannot turn a network flake green. It can
    only leave red what was already red.

  * **Cargo's own error is not trusted for the absence proof.** It prints
    `candidate versions found which didn't match: 0.9.0, 0.8.0, 0.7.0, ...` —
    truncated, so the list cannot show that a version is missing. Only the
    crate name is taken from it; the index is asked directly.

Usage:
    scripts/package_binding.py              package the binding
    scripts/package_binding.py --self-test  check the decision table alone

The self-test runs on every invocation as well, before any judgement is made.

Exit 0 if the binding packages, or if it fails solely because the release it is
pinned to has not been published yet. Non-zero otherwise.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BINDING = ROOT / "bindings" / "a2a-protocol-slimrpc"

# Cargo normalises a bare `version = "0.10"` to `^0.10` when it reports a
# resolution failure. Only the crate name is used; see the module docstring on
# why the candidate list that follows this line is not.
UNRESOLVED = re.compile(
    r"failed to select a version for the requirement `([A-Za-z0-9_-]+) = \"([^\"]+)\"`"
)

# A bare `X`, `X.Y` or `X.Y.Z`. Anything carrying an operator, a comma or a
# wildcard is not a pin this script will certify — see `analyse`.
BARE_REQ = re.compile(r"^\d+(\.\d+){0,2}$")
EXACT_VERSION = re.compile(r"^\d+\.\d+\.\d+$")


class IndexUnavailable(Exception):
    """The registry index could not be asked. Never grounds for a skip."""


def sparse_index_path(name: str) -> str:
    """crates.io sparse-index layout: 1/, 2/, 3/f/, then first-two/next-two."""
    n = name.lower()
    if len(n) <= 2:
        return f"{len(n)}/{n}"
    if len(n) == 3:
        return f"3/{n[0]}/{n}"
    return f"{n[0:2]}/{n[2:4]}/{n}"


def published_versions(name: str) -> dict[str, bool]:
    """Map version -> yanked, as the index reports it. Raises IndexUnavailable."""
    url = f"https://index.crates.io/{sparse_index_path(name)}"
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            body = resp.read().decode("utf-8")
    except urllib.error.HTTPError as exc:
        # 404 is the index's answer for "no such crate", which is a fact, not a
        # failure: an unpublished crate has no versions.
        if exc.code == 404:
            return {}
        raise IndexUnavailable(f"{url}: HTTP {exc.code}") from exc
    except Exception as exc:  # noqa: BLE001 — any transport failure is the same answer
        raise IndexUnavailable(f"{url}: {type(exc).__name__}: {exc}") from exc

    out: dict[str, bool] = {}
    for line in body.splitlines():
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError as exc:
            raise IndexUnavailable(f"{url}: malformed index line") from exc
        out[entry["vers"]] = bool(entry.get("yanked", False))
    return out


def caret_bounds(req: str) -> tuple[tuple[int, int, int], tuple[int, int, int]]:
    """Cargo's default (caret) range for a bare requirement.

    The upper bound is set by the leftmost non-zero component *of the parts
    actually written*, which is why `^0.0` and `^0` differ from `^0.0.3`.
    """
    parts = [int(p) for p in req.split(".")]
    major = parts[0]
    minor = parts[1] if len(parts) > 1 else 0
    patch = parts[2] if len(parts) > 2 else 0
    lower = (major, minor, patch)
    if major != 0:
        upper = (major + 1, 0, 0)
    elif minor != 0:
        upper = (0, minor + 1, 0)
    elif len(parts) >= 3 and patch != 0:
        upper = (0, 0, patch + 1)
    elif len(parts) >= 2:
        upper = (0, 1, 0)
    else:
        upper = (1, 0, 0)
    return lower, upper


def req_matches(req: str, version: str) -> bool:
    lower, upper = caret_bounds(req)
    v = tuple(int(p) for p in version.split("."))
    return lower <= v < upper


def analyse(
    stderr: str,
    pins: dict[str, tuple[str, str]],
    published: dict[str, dict[str, bool]],
) -> tuple[bool, str]:
    """Decide whether a failed `cargo package` is the release window.

    `pins` maps crate name -> (requirement as written, version found in tree).
    `published` maps crate name -> the index's version table, and is consulted
    only for the crate cargo actually named.

    Returns (skip_is_justified, human-readable reason). Kept free of I/O so
    `--self-test` can walk every branch without a network or a cargo run.
    """
    named = UNRESOLVED.findall(stderr)
    if not named:
        return False, (
            "the failure is not an unresolved version requirement, so it is a "
            "real packaging error"
        )

    # Cargo stops at the first unresolved requirement, so the crate it names is
    # necessarily one of the pins; a name from anywhere else means this error
    # is about some other dependency and is not the release window.
    unknown = [n for n, _ in named if n not in pins]
    if unknown:
        return False, (
            "cargo could not resolve " + ", ".join(sorted(set(unknown)))
            + ", which is not one of the in-tree SDK pins"
        )

    # Every pin is checked, not just the one cargo stopped on: a release window
    # that also carries a typo'd pin elsewhere must not be waved through on the
    # strength of the one requirement cargo happened to report first.
    for crate, (req, in_tree) in sorted(pins.items()):
        if not BARE_REQ.match(req):
            return False, (
                f"{crate} is pinned as `{req}`, not a bare version; this script "
                "certifies only exact caret pins"
            )
        if not EXACT_VERSION.match(in_tree):
            return False, (
                f"{crate} is version `{in_tree}` in tree, which is not a plain "
                "major.minor.patch"
            )
        if not req_matches(req, in_tree):
            return False, (
                f"{crate} is pinned `{req}` but is {in_tree} in tree — the pin "
                "does not name the version being built, so this is a broken "
                "manifest, not a release window"
            )

    # The absence proof, for the crate cargo actually failed on.
    crate = named[0][0]
    _, in_tree = pins[crate]
    table = published[crate]
    if in_tree in table:
        state = "yanked" if table[in_tree] else "published"
        return False, (
            f"{crate} {in_tree} is already {state} on crates.io, so the pin "
            "should have resolved — this failure is not the release window"
        )

    return True, (
        f"{crate} {in_tree} is pinned by the binding, built from the tree, and "
        "absent from crates.io"
    )


def read_pins() -> dict[str, tuple[str, str]]:
    """Every dependency declared with both `version` and `path`, and its in-tree version."""
    manifest = tomllib.loads((BINDING / "Cargo.toml").read_text())
    pins: dict[str, tuple[str, str]] = {}
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, spec in (manifest.get(section) or {}).items():
            if not isinstance(spec, dict) or "path" not in spec or "version" not in spec:
                continue
            dep_manifest = (BINDING / spec["path"] / "Cargo.toml").resolve()
            in_tree = tomllib.loads(dep_manifest.read_text())["package"]["version"]
            pins[name] = (spec["version"], in_tree)
    return pins


def annotate(level: str, message: str) -> None:
    """A skip has to be visible in the checks summary, not just the log."""
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level}::{message}")
    print(message)


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args, cwd=BINDING, capture_output=True, text=True, check=False
    )


def main() -> int:
    # The decision table is checked before it is used. This gate's whole value
    # is telling a release window from a broken manifest, and a table that had
    # drifted would get that wrong silently — in the direction of passing.
    if self_test() != 0:
        return 1

    packaged = run(["cargo", "package", "--no-verify", "--allow-dirty"])
    sys.stdout.write(packaged.stdout)
    sys.stderr.write(packaged.stderr)
    if packaged.returncode == 0:
        print("package_binding: the binding packages against the published SDK")
        return 0

    pins = read_pins()
    named = UNRESOLVED.findall(packaged.stderr)
    published: dict[str, dict[str, bool]] = {}
    if named and named[0][0] in pins:
        crate = named[0][0]
        try:
            published[crate] = published_versions(crate)
        except IndexUnavailable as exc:
            print(
                f"\npackage_binding: cargo package failed, and the crates.io index "
                f"could not be asked whether this is a release window ({exc}).\n"
                "A skip needs positive proof that the pinned version is absent, so "
                "this fails rather than guessing.",
                file=sys.stderr,
            )
            return 1

    skip, reason = analyse(packaged.stderr, pins, published)
    if not skip:
        print(
            f"\npackage_binding: `cargo package` failed and this is not the "
            f"release window — {reason}.",
            file=sys.stderr,
        )
        return 1

    # Registry resolution is the only thing that cannot be checked now. Prove
    # the rest of packaging still holds rather than skipping the gate whole.
    listed = run(["cargo", "package", "--list", "--no-verify", "--allow-dirty"])
    if listed.returncode != 0:
        sys.stderr.write(listed.stderr)
        print(
            "\npackage_binding: the SDK release is unpublished, but `cargo package "
            "--list` fails for a second, unrelated reason. Fix that first.",
            file=sys.stderr,
        )
        return 1

    annotate(
        "warning",
        "package_binding: registry resolution SKIPPED — " + reason + ". "
        "Everything `cargo package --list` covers still passed "
        f"({len(listed.stdout.splitlines())} files). Re-run this gate after the "
        "SDK is published; RELEASING.md step 4 is what closes the window.",
    )
    return 0


# ── --self-test ──────────────────────────────────────────────────────────────
# `analyse` is the whole of the new judgement, and the states it has to tell
# apart are exactly the ones that are expensive to stage for real: a release
# window needs an unpublished version, and a typo'd pin needs a broken tree.
# The table below walks all of them in milliseconds and without a network.
RESOLVE_ERR = (
    "error: failed to prepare local package for uploading\n"
    "\nCaused by:\n"
    '  failed to select a version for the requirement `a2a-protocol-client = "^0.10"`\n'
    "  candidate versions found which didn't match: 0.9.0, 0.8.0, 0.7.0, ...\n"
    "  location searched: crates.io index\n"
)
README_ERR = "error: readme `NO_SUCH_README.md` does not appear to exist\n"

WINDOW_PINS = {
    "a2a-protocol-types": ("0.10", "0.10.0"),
    "a2a-protocol-client": ("0.10", "0.10.0"),
    "a2a-protocol-server": ("0.10", "0.10.0"),
}
PUBLISHED_09 = {"a2a-protocol-client": {"0.9.0": False, "0.8.0": False}}

SELF_TEST_CASES: list[tuple[str, str, dict, dict, bool]] = [
    (
        "release window: pins name the in-tree version, which is unpublished",
        RESOLVE_ERR, WINDOW_PINS, PUBLISHED_09, True,
    ),
    (
        "typo'd pin: names a version neither in-tree nor published",
        RESOLVE_ERR,
        {**WINDOW_PINS, "a2a-protocol-types": ("0.42", "0.10.0")},
        PUBLISHED_09, False,
    ),
    (
        "typo'd pin on the crate cargo named",
        RESOLVE_ERR,
        {**WINDOW_PINS, "a2a-protocol-client": ("0.11", "0.10.0")},
        PUBLISHED_09, False,
    ),
    (
        "stale pin: in-tree bumped, pin left behind",
        RESOLVE_ERR,
        {**WINDOW_PINS, "a2a-protocol-client": ("0.9", "0.10.0")},
        PUBLISHED_09, False,
    ),
    (
        "broken manifest: not a resolution failure at all",
        README_ERR, WINDOW_PINS, PUBLISHED_09, False,
    ),
    (
        "already published: the pin should have resolved",
        RESOLVE_ERR, WINDOW_PINS,
        {"a2a-protocol-client": {"0.10.0": False, "0.9.0": False}}, False,
    ),
    (
        "published but yanked is not a release window",
        RESOLVE_ERR, WINDOW_PINS,
        {"a2a-protocol-client": {"0.10.0": True, "0.9.0": False}}, False,
    ),
    (
        "a range is not a pin this script will certify",
        RESOLVE_ERR,
        {**WINDOW_PINS, "a2a-protocol-server": (">=0.9, <0.11", "0.10.0")},
        PUBLISHED_09, False,
    ),
    (
        "unresolved crate that is not one of the pins",
        RESOLVE_ERR.replace("a2a-protocol-client", "some-other-crate"),
        WINDOW_PINS, PUBLISHED_09, False,
    ),
]

# Caret semantics, which the pin check leans on entirely.
CARET_CASES = [
    ("0.10", "0.10.0", True), ("0.10", "0.10.7", True), ("0.10", "0.11.0", False),
    ("0.9", "0.10.0", False), ("0.1", "0.10.0", False), ("1.2", "1.9.0", True),
    ("1.2", "2.0.0", False), ("0.0.3", "0.0.3", True), ("0.0.3", "0.0.4", False),
    ("0", "0.42.0", True), ("0.42", "0.10.0", False),
]


def self_test() -> int:
    failures = []
    for req, version, want in CARET_CASES:
        got = req_matches(req, version)
        if got != want:
            failures.append(f"caret: ^{req} vs {version} -> {got}, want {want}")

    for name, stderr, pins, published, want in SELF_TEST_CASES:
        got, reason = analyse(stderr, pins, published)
        if got != want:
            failures.append(f"{name}: skip={got}, want {want} ({reason})")

    if failures:
        print("package_binding --self-test: FAILED\n")
        for f in failures:
            print(f"  {f}")
        return 1
    print(
        f"package_binding --self-test: {len(CARET_CASES)} caret cases and "
        f"{len(SELF_TEST_CASES)} decision cases pass"
    )
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(main())
