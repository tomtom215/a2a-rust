#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Cross-checks the A2A method set against sources this project does not own.

# What problem this solves

Several claims in this repository are of the form "every A2A method is
exercised". A claim like that is only as good as its denominator, and a
denominator the measured project wrote about itself is not evidence: it can be
trimmed until the numerator looks complete, and a reviewer has no way to tell
from the outside.

So the denominator is triangulated across three sources, two of which this
project does not control:

  1. `proto/a2a_v1/a2a.proto` — the ratified specification artifact, vendored
     here. `scripts/check_proto_copies.sh` separately asserts every vendored
     copy is byte-identical, so this file cannot drift from the one the gRPC
     binding is generated from.
  2. `a2aproject/a2a-tck` — the conformance suite written by the
     specification's owners. Cloned fresh by the Official TCK workflow, so the
     comparison is against whatever upstream currently says, not a snapshot
     this repo took once.
  3. `crates/a2a-protocol-types/src/method.rs` — this project's mirror, which
     is what the coverage assertions actually iterate.

All three must name the same set. Disagreement is a hard failure: if the spec
and the suite disagree, that is an upstream question worth surfacing; if this
project disagrees with either, its coverage numbers are measured against the
wrong denominator and should not be believed until fixed.

# Usage

    check_method_denominator.py --proto proto/a2a_v1/a2a.proto \\
                                --rust crates/a2a-protocol-types/src/method.rs \\
                                [--tck /tmp/a2a-tck]

`--tck` is optional so the check is runnable locally without a clone; when it
is omitted the script says so in its output rather than silently comparing two
sources and reporting success as though it had compared three.

Exit codes: 0 all supplied sources agree, 1 disagreement, 2 a source could not
be read or parsed (never treated as agreement).
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


def die(msg: str, code: int = 2) -> "None":
    print(f"check_method_denominator: {msg}", file=sys.stderr)
    raise SystemExit(code)


def rpcs_from_proto(path: Path) -> set[str]:
    """RPC names inside `service A2AService`.

    Tracks brace depth: each `rpc` carries an `option (google.api.http) = {...}`
    body, so stopping at the first `}` yields one method out of eleven.
    """
    try:
        src = path.read_text(encoding="utf-8")
    except OSError as e:
        die(f"cannot read proto {path}: {e}")

    names: set[str] = set()
    in_service = False
    depth = 0
    for line in src.splitlines():
        t = line.strip()
        if not in_service:
            if t.startswith("service A2AService"):
                in_service = True
                depth = 1 if "{" in t else 0
            continue
        m = re.match(r"rpc\s+(\w+)\s*\(", t)
        if m:
            names.add(m.group(1))
        depth += t.count("{") - t.count("}")
        if depth <= 0:
            break

    if not in_service:
        die(f"no `service A2AService` block in {path}")
    if not names:
        die(f"parsed zero RPCs from {path} — an empty denominator is not a pass")
    return names


def methods_from_rust(path: Path) -> set[str]:
    """Wire names from `Method::wire_name`'s match arms."""
    try:
        src = path.read_text(encoding="utf-8")
    except OSError as e:
        die(f"cannot read {path}: {e}")

    # `Self::SendMessage => "SendMessage",`
    names = set(re.findall(r'Self::\w+\s*=>\s*"(\w+)"', src))
    if not names:
        die(f"parsed zero method names from {path} — refusing to report agreement")
    return names


def methods_from_slimrpc(path: Path) -> set[str]:
    """Wire names from the SLIMRPC binding's `method` module.

    The binding dispatches on `"{service}/{method}"`, so these constants are the
    binding's method inventory — the thing a "SLIMRPC serves every A2A method"
    claim is measured against. They are literal strings rather than a reuse of
    `a2a_protocol_types::Method`, which is what makes them able to drift.
    """
    try:
        src = path.read_text(encoding="utf-8")
    except OSError as e:
        die(f"cannot read {path}: {e}")

    block = re.search(r"^pub mod method \{(.*?)^\}", src, re.S | re.M)
    if not block:
        die(f"no `pub mod method` block found in {path}")

    # `pub const SEND_MESSAGE: &str = "SendMessage";`
    names = set(re.findall(r'pub const \w+:\s*&str\s*=\s*"(\w+)"', block.group(1)))
    if not names:
        die(f"parsed zero method names from {path} — refusing to report agreement")
    return names


def methods_from_tck(root: Path) -> set[str]:
    """Method names the official conformance suite refers to.

    Matched against the known PascalCase spelling rather than scraped freely:
    the suite is a large Python tree and a loose regex would pull in unrelated
    class names. That makes this check able to catch a *removal* upstream (a
    name this project expects that the suite no longer mentions) but not an
    *addition* of a method nobody here has heard of — so the proto, which can
    catch both, stays the primary source. Stated plainly because a check whose
    blind spot is undocumented is worse than one with no blind spot claimed.
    """
    if not root.is_dir():
        die(f"--tck path {root} is not a directory")

    text = []
    for p in root.rglob("*.py"):
        try:
            text.append(p.read_text(encoding="utf-8", errors="replace"))
        except OSError:
            continue
    if not text:
        die(f"no .py files under {root} — refusing to report agreement")
    blob = "\n".join(text)

    return set(re.findall(r'"(SendMessage|SendStreamingMessage|GetTask|ListTasks|'
                          r'CancelTask|SubscribeToTask|CreateTaskPushNotificationConfig|'
                          r'GetTaskPushNotificationConfig|ListTaskPushNotificationConfigs|'
                          r'DeleteTaskPushNotificationConfig|GetExtendedAgentCard)"', blob))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--proto", type=Path, required=True)
    ap.add_argument("--rust", type=Path, required=True)
    ap.add_argument("--tck", type=Path, default=None)
    ap.add_argument("--slimrpc", type=Path, default=None)
    args = ap.parse_args()

    proto = rpcs_from_proto(args.proto)
    rust = methods_from_rust(args.rust)

    print("A2A method denominator — cross-source check")
    print(f"  ratified proto ({args.proto}): {len(proto)}")
    print(f"  this repo      ({args.rust}): {len(rust)}")

    failed = False

    only_proto = sorted(proto - rust)
    only_rust = sorted(rust - proto)
    if only_proto:
        print(f"  MISMATCH: in the ratified proto but not in this repo: {only_proto}")
        failed = True
    if only_rust:
        print(f"  MISMATCH: in this repo but not in the ratified proto: {only_rust}")
        failed = True

    if args.tck is None:
        print("  official TCK: NOT CHECKED (no --tck path given)")
        print("  -> this run compared 2 sources, not 3")
    else:
        tck = methods_from_tck(args.tck)
        print(f"  official TCK   ({args.tck}): {len(tck)}")
        missing_in_tck = sorted(proto - tck)
        if missing_in_tck:
            print(
                "  MISMATCH: the ratified proto declares method(s) the official "
                f"suite never names: {missing_in_tck}"
            )
            failed = True
        extra_in_tck = sorted(tck - proto)
        if extra_in_tck:
            print(
                "  MISMATCH: the official suite names method(s) absent from the "
                f"ratified proto: {extra_in_tck}"
            )
            failed = True

    # The SLIMRPC binding is checked against the same denominator, but it is
    # explicitly NOT a conformance claim. The official TCK cannot grade this
    # binding: the TCK deliberately has no dependency on `a2a-protocol-*` (a kit
    # that imports the implementation it grades shares its misreadings), and no
    # independent SLIM client exists to write it against. So this arm answers
    # only the narrower question the TCK's absence leaves open — "does the
    # binding still claim all eleven methods?" — and catches a method silently
    # disappearing from its inventory. It does not establish that any of them
    # behaves correctly on the wire; the binding's own e2e suite does that.
    if args.slimrpc is None:
        print("  SLIMRPC binding: NOT CHECKED (no --slimrpc path given)")
    else:
        slim = methods_from_slimrpc(args.slimrpc)
        print(f"  SLIMRPC binding ({args.slimrpc}): {len(slim)}")
        missing = sorted(proto - slim)
        extra = sorted(slim - proto)
        if missing:
            print(
                "  MISMATCH: the ratified proto declares method(s) the SLIMRPC "
                f"binding does not serve: {missing}"
            )
            failed = True
        if extra:
            print(
                "  MISMATCH: the SLIMRPC binding names method(s) absent from the "
                f"ratified proto: {extra}"
            )
            failed = True

    if failed:
        print("\nFAIL — the coverage denominator is not agreed across sources.")
        print("Any 'every method is exercised' claim measured against it is")
        print("unsafe until this is resolved.")
        return 1

    n = len(proto)
    scope = "3 sources" if args.tck is not None else "2 sources"
    print(f"\nOK — {scope} agree on the same {n} methods.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
