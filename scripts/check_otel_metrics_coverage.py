#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when the bundled OpenTelemetry exporter ignores a `Metrics` callback.

# What problem this solves

`Metrics` gives every callback a no-op default, so implementations can override
only what they care about. That is right for third-party implementations and
wrong for the one this crate ships: `OtelMetrics` is the observability path the
project advertises, and a callback it does not override is silently discarded.

This is not hypothetical. `on_persistence_error` and `on_push_delivery` were
added to report the two failures nothing else reports — a store that has stopped
accepting writes, and a webhook refusing every delivery. Both were wired to
every call site, both compiled, both were covered by tests, and both were
dropped on the floor by `OtelMetrics`, which had been written before they
existed and inherited the defaults. The result was worse than no callback: the
signal appeared to exist.

No test could catch it, either. The existing OTel tests run against a noop meter
and assert only that calls do not panic — a no-op override passes that
perfectly.

# What this checks

Every method on the `Metrics` trait must appear in `impl Metrics for
OtelMetrics`. That is a coarse check — it cannot tell a real export from a stub
body — but it catches the failure that actually happened, which is a method
nobody remembered to add. Any method deliberately not exported must be listed in
`INTENTIONALLY_UNEXPORTED` below with a reason, which turns a silent omission
into a visible decision.

# Usage

    check_otel_metrics_coverage.py

Exit codes:
    0  every trait method is implemented by the exporter (or listed as exempt)
    1  the exporter is missing a callback
    2  a source file could not be read or parsed (never treated as agreement)
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TRAIT_SRC = REPO / "crates" / "a2a-protocol-server" / "src" / "metrics.rs"
OTEL_SRC = REPO / "crates" / "a2a-protocol-server" / "src" / "otel" / "mod.rs"

# Callbacks the OTLP exporter deliberately does not forward, each with the
# reason. Empty today: every callback the trait defines is exported.
INTENTIONALLY_UNEXPORTED: dict[str, str] = {}


def die(msg: str) -> None:
    print(f"check_otel_metrics_coverage: {msg}", file=sys.stderr)
    raise SystemExit(2)


def trait_methods(path: Path) -> list[str]:
    """Method names declared on `pub trait Metrics`."""
    try:
        src = path.read_text(encoding="utf-8")
    except OSError as e:
        die(f"cannot read {path}: {e}")

    m = re.search(r"pub trait Metrics:[^\{]*\{(.*?)\n\}", src, re.S)
    if not m:
        die(f"no `pub trait Metrics` block found in {path}")

    names = re.findall(r"^\s{4}fn (\w+)\s*\(", m.group(1), re.M)
    if not names:
        die(f"parsed zero methods from the Metrics trait in {path}")
    return names


def impl_methods(path: Path) -> list[str]:
    """Method names inside `impl Metrics for OtelMetrics`."""
    try:
        src = path.read_text(encoding="utf-8")
    except OSError as e:
        die(f"cannot read {path}: {e}")

    start = src.find("impl Metrics for OtelMetrics {")
    if start == -1:
        die(f"no `impl Metrics for OtelMetrics` block found in {path}")

    # Walk braces so a method body containing `}` cannot end the block early.
    depth = 0
    i = src.index("{", start)
    body_start = i + 1
    while i < len(src):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                break
        i += 1

    names = re.findall(r"^\s{4}fn (\w+)\s*\(", src[body_start:i], re.M)
    if not names:
        die(f"parsed zero methods from the OtelMetrics impl in {path}")
    return names


def main() -> int:
    declared = trait_methods(TRAIT_SRC)
    exported = set(impl_methods(OTEL_SRC))

    print("OpenTelemetry exporter coverage of the Metrics trait")
    print(f"  trait declares : {len(declared)}")
    print(f"  exporter covers: {len(exported)}")

    missing = [
        m for m in declared if m not in exported and m not in INTENTIONALLY_UNEXPORTED
    ]
    exempt = [m for m in declared if m in INTENTIONALLY_UNEXPORTED]

    for m in exempt:
        print(f"  EXEMPT  {m} — {INTENTIONALLY_UNEXPORTED[m]}")

    if missing:
        print(f"\nFAIL — {len(missing)} callback(s) the bundled exporter drops:\n")
        for m in missing:
            print(f"    {m}")
        print(
            "\n`Metrics` methods default to no-ops, so a callback OtelMetrics does"
            "\nnot override is discarded without a compile error and without a"
            "\nfailing test — the noop-meter tests pass against a no-op override."
            "\nImplement it in `impl Metrics for OtelMetrics`, or add it to"
            "\nINTENTIONALLY_UNEXPORTED with a reason."
        )
        return 1

    print("\nOK — the exporter forwards every callback the trait declares.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
