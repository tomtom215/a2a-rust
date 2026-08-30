#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Assert that hand-written version claims still match the manifests.

Why this exists. The release-prep commit bumps version *numbers* — the four
SDK crates, the binding's own version, and the binding's `a2a-protocol-*`
requirements. It does not touch the prose that explains those numbers, and
several sentences quote them verbatim. Nothing recomputed them, so they decayed.

At v0.10.0 all four such sentences were updated by hand and were correct. At
v0.11.0 none of them were, and the tree simultaneously said:

    manifest : a2a-protocol-types = { version = "0.11", path = ... }
    comment  : "the reason the SDK requirement below is a tight `0.10`"

    manifest : a2a-protocol-slimrpc version = "0.3.0", SDK version = "0.11.0"
    RELEASING: "versioned independently — `0.2.0` against the SDK's `0.10.0`"

The binding's comment is the costly one. `cargo package` writes the original
manifest into the `.crate` as `Cargo.toml.orig`, comments intact, so a stale
sentence there is published to crates.io permanently — a release cannot be
amended, only yanked and superseded.

`scripts/package_binding.py` already checks the *pins* against the in-tree
versions. This is the ratchet for the gap that check cannot close: it compares
the *prose* against the same manifests, so a bump that forgets a sentence goes
red instead of shipping.

Deliberately narrow, for the reason `check_benchmark_prose.sh` gives: it checks
named claims, not every number. A generic "find every version-shaped string"
sweep would flag the changelog, the historical tag list, and the worked example
in the packaging-window table — all legitimately naming past versions — and a
gate that cries wolf gets routed around. New quoted versions are added here
explicitly.

The structural rule, the same one that gate learned by mutation: an anchor that
matches nothing is a FAILURE, never a pass. Otherwise rewording a sentence
silently disarms the check that guards it, and the gate reports green while
measuring no claim at all.

Usage:
  ./scripts/check_release_version_prose.py

Exit codes:
  0  every checked claim agrees with the manifests
  1  a claim has drifted, or an anchor no longer matches
"""

from __future__ import annotations

import pathlib
import re
import sys
import tomllib

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent

SDK_MANIFEST = REPO_ROOT / "crates" / "a2a-protocol-types" / "Cargo.toml"
BINDING_MANIFEST = REPO_ROOT / "bindings" / "a2a-protocol-slimrpc" / "Cargo.toml"
RELEASING = REPO_ROOT / "RELEASING.md"

# Spelled-out minor numbers, for "would claim <word> minor versions".
NUMBER_WORDS = {
    1: "one", 2: "two", 3: "three", 4: "four", 5: "five", 6: "six", 7: "seven",
    8: "eight", 9: "nine", 10: "ten", 11: "eleven", 12: "twelve",
    13: "thirteen", 14: "fourteen", 15: "fifteen", 16: "sixteen",
    17: "seventeen", 18: "eighteen", 19: "nineteen", 20: "twenty",
}

SDK_CRATES = ("a2a-protocol-types", "a2a-protocol-client", "a2a-protocol-server")

failures: list[str] = []


def fail(msg: str) -> None:
    failures.append(msg)


def ground_truth() -> tuple[str, str, str]:
    """(sdk version, binding version, the binding's SDK requirement)."""
    sdk_doc = tomllib.loads(SDK_MANIFEST.read_text(encoding="utf-8"))
    binding_doc = tomllib.loads(BINDING_MANIFEST.read_text(encoding="utf-8"))

    sdk = sdk_doc["package"]["version"]
    binding = binding_doc["package"]["version"]

    pins = {c: binding_doc["dependencies"][c]["version"] for c in SDK_CRATES}
    distinct = set(pins.values())
    if len(distinct) != 1:
        # Not this gate's job to say which is right — package_binding.py checks
        # pins against the tree — but the prose quotes a single requirement, so
        # there has to be one to quote.
        fail(
            "the binding's three SDK requirements disagree, so the single "
            f"requirement its prose quotes is ambiguous: {pins}"
        )
    return sdk, binding, sorted(distinct)[0]


def check(path: pathlib.Path, label: str, pattern: str, expected: tuple[str, ...]) -> None:
    """Assert `pattern` matches exactly once in `path` and captures `expected`.

    Zero matches fails. That is the point: a reworded sentence must break this
    gate loudly rather than quietly leave its claim unguarded.
    """
    text = path.read_text(encoding="utf-8")
    matches = re.findall(pattern, text)
    rel = path.relative_to(REPO_ROOT)

    if not matches:
        fail(
            f"{rel}: the {label} anchor matched nothing.\n"
            f"      pattern: {pattern}\n"
            "      The sentence was reworded or removed. Re-point the anchor — "
            "leaving it unmatched would leave the claim unchecked."
        )
        return
    if len(matches) > 1:
        fail(
            f"{rel}: the {label} anchor matched {len(matches)} times; it must "
            f"identify one claim. Narrow it.\n      pattern: {pattern}"
        )
        return

    found = matches[0]
    found = (found,) if isinstance(found, str) else tuple(found)
    if found != expected:
        fail(
            f"{rel}: the {label} claim has drifted from the manifests.\n"
            f"      prose says     : {found}\n"
            f"      manifests say  : {expected}"
        )


def main() -> int:
    sdk, binding, pin = ground_truth()
    minor = int(sdk.split(".")[1])
    word = NUMBER_WORDS.get(minor)
    if word is None:
        fail(
            f"SDK minor version {minor} has no entry in NUMBER_WORDS, so the "
            '"would claim <word> minor versions" claim cannot be checked. '
            "Extend the table."
        )
        word = ""

    # --- the binding's own manifest: these comments ship to crates.io ---
    check(
        BINDING_MANIFEST,
        "binding-would-match-SDK",
        r"# This crate's public API is new\. Numbering it (\S+) to match the SDK would\n"
        r"# claim (\w+) minor versions",
        (sdk, word),
    )
    check(
        BINDING_MANIFEST,
        "binding-tight-requirement",
        r"# tight `([^`]+)` rather than a range",
        (pin,),
    )

    # --- RELEASING.md ---
    check(
        RELEASING,
        "releasing-independent-versioning",
        r"It is versioned independently — `([^`]+)` against the SDK's `([^`]+)`",
        (binding, sdk),
    )
    check(
        RELEASING,
        "releasing-tight-requirement",
        r"therefore a tight `([^`]+)`, not a range",
        (pin,),
    )
    check(
        RELEASING,
        "releasing-current-sdk-version",
        r"all four are at\s+(\S+) as of this writing",
        (sdk,),
    )

    if failures:
        print("check_release_version_prose: FAILED", file=sys.stderr)
        print(
            f"  manifests: SDK {sdk}, binding {binding}, "
            f"binding's SDK requirement {pin}\n",
            file=sys.stderr,
        )
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        print(
            "\n  A version nothing recomputes is a version that decays. Update "
            "the prose to match the manifests.",
            file=sys.stderr,
        )
        return 1

    print(
        f"check_release_version_prose: 5 version claims agree with the manifests "
        f"(SDK {sdk}, binding {binding}, requirement {pin})"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
