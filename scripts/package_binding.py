#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Package the SLIMRPC binding and build its tarball against the in-tree SDK.

The binding lives outside the workspace and depends on the SDK crates by
`version` *and* `path`. Locally the path wins, so its build, clippy and test
steps are green against the in-tree crates. `cargo package` strips the path, so
the `version` requirement resolves against the crates.io index instead — the
*last released* SDK, not the tree — and during a release that index does not
yet carry the version being prepared.

That had two costs. One commit in every release cycle was unpassable, in both
directions:

    pin      in-tree   build / clippy / test        cargo package
    ^0.11    0.11.0    pass                         fails — 0.11.0 not published
    ^0.10    0.11.0    fails — didn't match 0.11.0  fails

And, until 2026-09-10, the gate ran `cargo package --no-verify` the rest of the
time, so the tarball was listed and its manifest checked but it was never
built: verification would have compiled the binding against the published SDK,
which is not what Build and Test compile it against, and any API the binding
used from the same change would have failed until that release shipped.

Both have one cause — registry resolution of the SDK pins — and one fix: Cargo's
`[patch]`, applied at the config level so the manifest is untouched:

    cargo package --allow-dirty \\
        --config 'patch.crates-io.a2a-protocol-types.path="/abs/crates/a2a-protocol-types"' \\
        --config 'patch.crates-io.a2a-protocol-client.path="…"' \\
        --config 'patch.crates-io.a2a-protocol-server.path="…"'

A patch supplies a version of a crates.io crate from a path, including a version
the index does not have — Cargo's own "prepublishing a breaking change" case. So
verification builds the tarball against the in-tree SDK, exactly what Build and
Test do, and the release window stops being a state at all: the pin names the
in-tree version, the patch supplies it, and the index is not asked for it.

Measured 2026-09-10. On a synthetic consumer pinned `version = "0.99", path`
to an unpublished `a2a-protocol-types 0.99.0`, plain `cargo package` fails with
`failed to select a version for the requirement`, and the same command with the
patch packages and verifies. On the binding itself the patched verification
compiled the packaged crate against the tree in 17.5s. The one visible
difference from an unpatched tarball is the regenerated `Cargo.lock` inside it,
which records the SDK crates without a registry `source` line; nothing consumes
that file — the gate's tarball is never the published one — but it is why this
script must not be mistaken for the release step.

What the patch does not do, and this script still must:

  * **Refuse a pin that does not name the in-tree version, before cargo runs.**
    A patch is used only when its version satisfies the requirement. A stale
    `0.10` against an in-tree 0.11.0 leaves the patch unused; cargo *warns*,
    resolves 0.10.0 from crates.io, and verification builds against the
    published SDK — passing or failing on the wrong crate either way
    (measured: the synthetic consumer, re-pinned to a published version,
    verified against the registry crate and failed on an API only the patch
    had). The pin check is the one the previous decision table had, kept and
    self-tested.

  * **Treat an unused patch as a failure, not a warning.** Belt and braces on
    the above, so a mismatch this script's semver arithmetic did not predict
    is still red rather than a line in the log.

Removed the same day: the `cargo package --list --no-verify` fallback and the
crates.io index query behind it. They existed to tell a release window from a
broken manifest when the window could not be verified; with the patch it can,
so there was nothing left for them to cover. See
docs/v0.9.0-post-release-review.md, B23 and §2.5.

Usage:
    scripts/package_binding.py              package and verify the binding
    scripts/package_binding.py --self-test  check the pin rules alone

The self-test runs on every invocation as well, before cargo is called.

Exit 0 if the binding packages and its tarball builds against the in-tree SDK.
Non-zero otherwise.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import NamedTuple

ROOT = Path(__file__).resolve().parent.parent
BINDING = ROOT / "bindings" / "a2a-protocol-slimrpc"

# A bare `X`, `X.Y` or `X.Y.Z`. Anything carrying an operator, a comma or a
# wildcard is not a pin this script will certify — see `check_pins`.
BARE_REQ = re.compile(r"^\d+(\.\d+){0,2}$")
EXACT_VERSION = re.compile(r"^\d+\.\d+\.\d+$")

# Cargo's exact wording for a patch the resolver did not select, e.g.
#   warning: patch `a2a-protocol-types v0.11.0 (/path)` was not used in the crate graph
UNUSED_PATCH = re.compile(r"patch `([A-Za-z0-9_-]+) v[^`]*` was not used in the crate graph")


class Pin(NamedTuple):
    """One dependency declared with both `version` and `path`."""

    req: str
    """The requirement as written, e.g. `0.11`."""
    in_tree: str
    """The version in the crate's own manifest at `path`, e.g. `0.11.0`."""
    path: str
    """Absolute path to that crate, for the patch."""


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


def check_pins(pins: dict[str, tuple[str, str]]) -> tuple[bool, str]:
    """Decide whether the pins are ones a patch will actually be used for.

    `pins` maps crate name -> (requirement as written, version found in tree).
    Returns (pins_are_sound, human-readable reason). Kept free of I/O so
    `--self-test` can walk every branch without a cargo run.
    """
    if not pins:
        return False, (
            "the manifest declares no dependency with both `version` and `path`, "
            "so there is nothing to patch — the shape this gate exists for has "
            "changed and the gate needs rethinking, not skipping"
        )
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
                f"{crate} is pinned `{req}` but is {in_tree} in tree — the patch "
                "would go unused and verification would build against the "
                "published crate instead of this one"
            )
    return True, "every pin names the version that is in the tree"


def patch_args(paths: dict[str, str]) -> list[str]:
    """The `--config` arguments that supply each pinned crate from its path.

    `json.dumps` quotes the path as a TOML basic string; the two formats agree
    on every escape a filesystem path can need.
    """
    args: list[str] = []
    for crate, path in sorted(paths.items()):
        args += ["--config", f"patch.crates-io.{crate}.path={json.dumps(path)}"]
    return args


def read_pins() -> dict[str, Pin]:
    """Every dependency declared with both `version` and `path`, and its in-tree version."""
    manifest = tomllib.loads((BINDING / "Cargo.toml").read_text())
    pins: dict[str, Pin] = {}
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, spec in (manifest.get(section) or {}).items():
            if not isinstance(spec, dict) or "path" not in spec or "version" not in spec:
                continue
            dep_dir = (BINDING / spec["path"]).resolve()
            in_tree = tomllib.loads((dep_dir / "Cargo.toml").read_text())["package"]["version"]
            pins[name] = Pin(spec["version"], in_tree, str(dep_dir))
    return pins


def annotate(level: str, message: str) -> None:
    """A failure has to be visible in the checks summary, not just the log."""
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level}::{message}")
    print(message, file=sys.stderr)


def run(args: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args, cwd=BINDING, capture_output=True, text=True, check=False
    )


def main() -> int:
    # The pin rules are checked before they are used. This gate's value is
    # building the tarball against the tree, and a rule that had drifted would
    # let a stale pin through to a build against the registry — silently, in
    # the direction of passing.
    if self_test() != 0:
        return 1

    pins = read_pins()
    sound, reason = check_pins({name: (p.req, p.in_tree) for name, p in pins.items()})
    if not sound:
        annotate("error", f"package_binding: refusing to package — {reason}.")
        return 1

    args = ["cargo", "package", "--allow-dirty"]
    args += patch_args({name: p.path for name, p in pins.items()})
    print("package_binding: " + " ".join(args))

    packaged = run(args)
    sys.stdout.write(packaged.stdout)
    sys.stderr.write(packaged.stderr)

    if packaged.returncode != 0:
        annotate(
            "error",
            "package_binding: `cargo package` failed. The in-tree SDK was supplied "
            "by patch, so this is not the release window; it is a packaging error.",
        )
        return 1

    unused = sorted(set(UNUSED_PATCH.findall(packaged.stderr)))
    if unused:
        annotate(
            "error",
            "package_binding: cargo did not use the patch for "
            + ", ".join(unused)
            + ", so verification built against the published crate rather than "
            "the tree. The pin check should have caught this; the arithmetic "
            "and cargo disagree, and cargo is right.",
        )
        return 1

    against = ", ".join(f"{name} {p.in_tree}" for name, p in sorted(pins.items()))
    print(
        "package_binding: the binding packages and its tarball builds against "
        f"the in-tree SDK ({against})"
    )
    return 0


# ── --self-test ──────────────────────────────────────────────────────────────
# `check_pins` is the whole of the judgement made before cargo runs, and the
# state it exists to refuse — a pin that leaves the patch unused — is one cargo
# reports as a warning and then builds past. The table below walks every branch
# in milliseconds and without a cargo run.
SOUND_PINS = {
    "a2a-protocol-types": ("0.11", "0.11.0"),
    "a2a-protocol-client": ("0.11", "0.11.0"),
    "a2a-protocol-server": ("0.11", "0.11.0"),
}

PIN_CASES: list[tuple[str, dict[str, tuple[str, str]], bool]] = [
    ("every pin names the in-tree version", SOUND_PINS, True),
    (
        "release window is not special: the pin names an in-tree version "
        "whether or not the index has it",
        {**SOUND_PINS, "a2a-protocol-types": ("0.12", "0.12.0")},
        True,
    ),
    (
        "stale pin: in-tree bumped, pin left behind — the patch would go unused",
        {**SOUND_PINS, "a2a-protocol-client": ("0.10", "0.11.0")},
        False,
    ),
    (
        "typo'd pin: names a version that is not in the tree",
        {**SOUND_PINS, "a2a-protocol-types": ("0.42", "0.11.0")},
        False,
    ),
    (
        "a range is not a pin this script will certify",
        {**SOUND_PINS, "a2a-protocol-server": (">=0.10, <0.12", "0.11.0")},
        False,
    ),
    (
        "a pre-release in tree is not a plain version",
        {**SOUND_PINS, "a2a-protocol-server": ("0.11", "0.11.0-rc.1")},
        False,
    ),
    ("no pins at all is a changed shape, not a pass", {}, False),
]

# Caret semantics, which the pin check leans on entirely.
CARET_CASES = [
    ("0.10", "0.10.0", True), ("0.10", "0.10.7", True), ("0.10", "0.11.0", False),
    ("0.9", "0.10.0", False), ("0.1", "0.10.0", False), ("1.2", "1.9.0", True),
    ("1.2", "2.0.0", False), ("0.0.3", "0.0.3", True), ("0.0.3", "0.0.4", False),
    ("0", "0.42.0", True), ("0.42", "0.10.0", False),
]

# The patch arguments, and the warning that means cargo ignored one. The
# warning text is the one cargo 1.94 prints, captured from a run rather than
# transcribed from documentation.
UNUSED_WARNING = (
    "warning: patch `a2a-protocol-types v0.99.0 (/tmp/x/a2a-protocol-types)` "
    "was not used in the crate graph\n"
    "help: Check that the patched package version and available features are compatible\n"
)


def self_test() -> int:
    failures = []
    for req, version, want in CARET_CASES:
        got = req_matches(req, version)
        if got != want:
            failures.append(f"caret: ^{req} vs {version} -> {got}, want {want}")

    for name, pins, want in PIN_CASES:
        got, reason = check_pins(pins)
        if got != want:
            failures.append(f"{name}: sound={got}, want {want} ({reason})")

    want_args = [
        "--config", 'patch.crates-io.a2a-protocol-client.path="/r/crates/a2a-protocol-client"',
        "--config", 'patch.crates-io.a2a-protocol-types.path="/r/crates/a2a-protocol-types"',
    ]
    got_args = patch_args({
        "a2a-protocol-types": "/r/crates/a2a-protocol-types",
        "a2a-protocol-client": "/r/crates/a2a-protocol-client",
    })
    if got_args != want_args:
        failures.append(f"patch args: {got_args}")

    if UNUSED_PATCH.findall(UNUSED_WARNING) != ["a2a-protocol-types"]:
        failures.append("unused-patch warning not recognised")
    if UNUSED_PATCH.findall("    Finished `dev` profile\n"):
        failures.append("unused-patch regex matches a clean run")

    if failures:
        print("package_binding --self-test: FAILED\n")
        for f in failures:
            print(f"  {f}")
        return 1
    print(
        f"package_binding --self-test: {len(CARET_CASES)} caret cases, "
        f"{len(PIN_CASES)} pin cases, the patch arguments and the unused-patch "
        "warning all pass"
    )
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    sys.exit(main())
