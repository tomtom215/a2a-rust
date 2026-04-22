#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
"""Fail CI when a criterion benchmark regresses beyond a configured threshold.

Reads criterion's per-benchmark `change/estimates.json` files produced by
`cargo bench -- --baseline <name>`. For each benchmark, the script compares
the median regression's **lower 95 %-confidence-interval bound** against a
percentage threshold (default 25 %). A benchmark only counts as a regression
if we can say, with 95 % confidence, that the median is at least `threshold`
slower than the baseline.

Why the CI lower bound instead of the point estimate:
  GitHub-hosted runners have noticeable per-run variance (10–20 % typical on
  small benches, spikes above that are not unusual). Gating on the point
  estimate alone produces false-positive CI failures on that noise. The
  confidence-interval lower bound incorporates criterion's own uncertainty
  quantification: only differences large enough that the noise alone could
  not plausibly produce them pass the threshold. This gives us a gate that
  catches real regressions (accidental O(n²) loops, allocator thrash, lost
  inlining) without flaking on the runner.

Exit codes:
  0 — no benchmark regressed beyond the threshold.
  1 — one or more benchmarks regressed; details printed to stderr.
  2 — configuration or parse error.

Usage:
  ./benches/scripts/check_regression.py \\
      --target-dir target/criterion \\
      --threshold 0.25
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import NamedTuple


class Change(NamedTuple):
    """One benchmark's measured change from the baseline."""

    name: str
    median_point: float
    median_ci_lower: float
    median_ci_upper: float
    mean_point: float


def find_change_files(target_dir: Path) -> list[Path]:
    """Return every `change/estimates.json` under `target_dir`.

    Criterion writes these when invoked with `--baseline <name>`; a fresh
    run with no baseline produces no `change/` directories. An empty list
    here means the caller forgot to run with a baseline — we surface that
    as a config error rather than a silent pass.
    """
    return sorted(target_dir.glob("**/change/estimates.json"))


def benchmark_name(change_file: Path, target_dir: Path) -> str:
    """Derive a readable benchmark name from the file path.

    Layout: `<target>/<group>/<bench>/change/estimates.json` →
    `<group>/<bench>`.
    """
    rel = change_file.relative_to(target_dir)
    parts = rel.parts[:-2]  # drop "change/estimates.json"
    return "/".join(parts) if parts else str(rel)


def parse_change(path: Path, name: str) -> Change:
    """Extract point + CI bounds from one `change/estimates.json` file.

    Criterion's schema:
      { "median": { "point_estimate": f,
                    "confidence_interval": { "lower_bound": f,
                                             "upper_bound": f } },
        "mean":   { ... same shape } }
    """
    with path.open() as f:
        data = json.load(f)
    try:
        median = data["median"]
        mean = data["mean"]
        ci = median["confidence_interval"]
        return Change(
            name=name,
            median_point=float(median["point_estimate"]),
            median_ci_lower=float(ci["lower_bound"]),
            median_ci_upper=float(ci["upper_bound"]),
            mean_point=float(mean["point_estimate"]),
        )
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError(f"malformed {path}: {exc}") from exc


def format_row(change: Change, threshold: float) -> tuple[str, bool]:
    """Format one row of the summary table and report whether it regressed.

    A benchmark is considered regressed only when the 95 % CI lower bound
    of the median change is strictly greater than `threshold` — in other
    words, the whole confidence interval sits above the threshold line.
    """
    is_regression = change.median_ci_lower > threshold
    marker = "REGRESSED" if is_regression else "ok"
    row = (
        f"[{marker:>9}] {change.name:<55} "
        f"median {change.median_point * 100:+7.2f}% "
        f"(95% CI [{change.median_ci_lower * 100:+.2f}%, "
        f"{change.median_ci_upper * 100:+.2f}%])"
    )
    return row, is_regression


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=Path("target/criterion"),
        help="Criterion root directory (default: target/criterion)",
    )
    parser.add_argument(
        "--threshold",
        type=float,
        default=0.25,
        help="Fractional regression that fails the gate (default: 0.25 = 25%%)",
    )
    args = parser.parse_args()

    if not args.target_dir.is_dir():
        print(
            f"error: criterion target dir not found: {args.target_dir}",
            file=sys.stderr,
        )
        return 2

    change_files = find_change_files(args.target_dir)
    if not change_files:
        print(
            f"error: no benchmark change files under {args.target_dir}. "
            "Did you run `cargo bench -- --baseline <name>` with an "
            "existing baseline? If the workflow ran but produced no "
            "changes, criterion may have been unable to match baseline "
            "and new benchmark names (e.g. because a bench was renamed).",
            file=sys.stderr,
        )
        return 2

    print(
        f"Analysing {len(change_files)} benchmark change(s) against "
        f"threshold {args.threshold * 100:.0f}% "
        f"(using 95% CI lower bound):\n"
    )

    regressions: list[Change] = []
    for change_file in change_files:
        try:
            name = benchmark_name(change_file, args.target_dir)
            change = parse_change(change_file, name)
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
        row, is_reg = format_row(change, args.threshold)
        print(row)
        if is_reg:
            regressions.append(change)

    if regressions:
        print(
            f"\n{len(regressions)} benchmark(s) regressed beyond "
            f"{args.threshold * 100:.0f}% "
            f"(95% CI lower bound exceeds threshold):",
            file=sys.stderr,
        )
        for change in regressions:
            print(
                f"  - {change.name}: "
                f"median {change.median_point * 100:+.2f}% "
                f"(95% CI [{change.median_ci_lower * 100:+.2f}%, "
                f"{change.median_ci_upper * 100:+.2f}%])",
                file=sys.stderr,
            )
        return 1

    print(
        f"\nAll {len(change_files)} benchmarks within "
        f"{args.threshold * 100:.0f}% of baseline "
        "(or within the runner's noise envelope)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
