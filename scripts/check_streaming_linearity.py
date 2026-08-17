#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when streaming cost stops being linear in the number of events.

# What problem this solves

Streaming benchmarks report a total, and a total that grows with event count
looks correct no matter how it grows. Between v0.5.0 and v0.8.0 this project
shipped a quadratic streaming path — `process_event_bg` saved the whole task on
every artifact event, and the in-memory store deep-clones what it is given, so
event *i* copied *i* artifacts. At 502 events that was 43.4 ms of which 40.2 ms
was re-persisting artifacts already persisted.

Nothing caught it, and not for lack of looking. The number was seen, discussed,
and explained twice — first as broadcast-ring overflow, then as "the inherent
cost of SSE frame serialization + HTTP chunked encoding", recorded in a comment
that called it "NOT a regression". Both explanations were consistent with the
total being large. Neither was consistent with the *marginal* cost of an event
growing with the number of events before it, which is what the data actually
showed and what nothing was checking.

So this checks the shape rather than the size. Per-event cost may be whatever
the hardware makes it; what it may not do is grow with the length of the
stream.

# How

For each configured benchmark group, read criterion's medians across the event
counts, convert to marginal cost per event between adjacent points, and compare
the marginal cost at the top of the range against a reference taken from the
middle. Linear streaming holds the ratio near 1. The quadratic version scored
26. The threshold sits at 3, which is far enough above the noise of a shared
runner to avoid flapping and far enough below 26 to catch a reintroduction long
before it reaches the old magnitude.

Marginal rather than average cost, because averages hide this: a quadratic
curve's *average* per-event cost also rises, but slowly enough at small n to
look like ordinary overhead amortization.

# Usage

    check_streaming_linearity.py [--criterion-dir target/criterion]
                                 [--max-ratio 3.0]

Exit codes:
    0  every group's per-event cost is flat within the threshold
    1  a group's per-event cost grows with event count
    2  the measurements needed are missing (benchmarks not run)
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

# Groups to police, as (criterion directory, benchmark-id template, event
# counts, max ratio). The counts must be ascending and at least four long:
# three points give one reference and one probe with nothing left over to
# sanity-check.
#
# The per-group ratio exists because backends have legitimately different
# shapes, and one global threshold would either exempt the strict cases or
# flake on the loose one. Each value below is justified where it is set;
# `--max-ratio` overrides all of them for a one-off investigation.
GROUPS: list[tuple[str, str, list[int], float]] = [
    # In-memory paths do constant work per event, so anything above noise is a
    # regression. The pre-fix quadratic version scored 26.
    ("backpressure_stream_volume", "{n}_events", [7, 27, 52, 252, 502], 3.0),
    ("backpressure_append_volume", "task_store_{n}_events", [7, 27, 52, 252, 502], 3.0),
    ("backpressure_append_volume", "discard_store_{n}_events", [7, 27, 52, 252, 502], 3.0),
    # SQLite stores one JSON document per task and rewrites the row on every
    # update, so *some* growth with document size is inherent to the schema and
    # cannot be removed by any delta API — only by normalising artifacts into
    # their own table, which the measurements say would buy the smaller half of
    # the cost. Measured at 2.03 after `save_artifact_delta`; 4.0 leaves room
    # for runner noise while still catching a return to the pre-fix shape.
    ("backpressure_append_volume", "sqlite_store_{n}_events", [7, 27, 52, 252, 502], 4.0),
]


def median_ns(criterion_dir: Path, group: str, bench_id: str) -> float | None:
    """Median for one benchmark id, in nanoseconds."""
    # Criterion flattens every `/` in both the group name and the benchmark id
    # into `_` when it lays out directories, so the id used here is the
    # flattened form rather than the one printed in the benchmark output.
    path = criterion_dir / group / bench_id / "new" / "estimates.json"
    if not path.is_file():
        path = criterion_dir / group / bench_id / "base" / "estimates.json"
    if not path.is_file():
        return None
    try:
        return json.loads(path.read_text(encoding="utf-8"))["median"]["point_estimate"]
    except (OSError, KeyError, ValueError):
        return None


def marginal(points: list[tuple[int, float]]) -> list[tuple[int, int, float]]:
    """Cost of each additional event between adjacent measurements, in ns."""
    out = []
    for (n0, t0), (n1, t1) in zip(points, points[1:]):
        out.append((n0, n1, (t1 - t0) / (n1 - n0)))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--criterion-dir", type=Path, default=Path("target/criterion"))
    ap.add_argument(
        "--max-ratio",
        type=float,
        default=None,
        help="override every group's own limit (for investigation)",
    )
    args = ap.parse_args()

    print("streaming linearity — marginal cost per event must not grow")
    missing: list[str] = []
    failures: list[str] = []
    checked = 0

    for group, template, counts, group_limit in GROUPS:
        limit = args.max_ratio if args.max_ratio is not None else group_limit
        points: list[tuple[int, float]] = []
        for n in counts:
            ns = median_ns(args.criterion_dir, group, template.format(n=n))
            if ns is None:
                missing.append(f"{group}/{template.format(n=n)}")
            else:
                points.append((n, ns))

        if len(points) < 4:
            continue

        steps = marginal(points)
        # Reference from the middle of the range: the first step carries
        # per-request fixed cost amortized over very few events, which makes it
        # an unrepresentatively expensive baseline.
        reference = min(cost for _, _, cost in steps[1:-1])
        top_lo, top_hi, top = steps[-1]
        ratio = top / reference if reference > 0 else float("inf")
        checked += 1

        label = f"{group}/{template.format(n='N')}"
        detail = ", ".join(f"{lo}->{hi}: {c / 1000:.1f}µs" for lo, hi, c in steps)
        print(f"  {label}")
        print(f"    marginal: {detail}")
        print(
            f"    top step {top_lo}->{top_hi} is {ratio:.2f}x the flat reference "
            f"({reference / 1000:.1f}µs); limit {limit}x"
        )

        if ratio > limit:
            failures.append(
                f"{label}: per-event cost at {top_lo}->{top_hi} events is "
                f"{ratio:.2f}x the {reference / 1000:.1f}µs seen mid-range. "
                f"Streaming cost is growing with stream length — the usual "
                f"cause is per-event work proportional to what the task has "
                f"already accumulated."
            )

    if missing:
        print("\nMissing measurements:")
        for m in missing:
            print(f"  {m}")
        if checked == 0:
            print("\nRun the benchmarks first:")
            print("  cargo bench -p a2a-benchmarks --bench backpressure")
            return 2

    if failures:
        print("\nFAIL — streaming is not linear in event count.")
        for f in failures:
            print(f"  {f}")
        return 1

    print(f"\nOK — {checked} group(s) within their limits.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
