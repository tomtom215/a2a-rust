#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F.
"""Fail CI when a criterion benchmark regresses beyond a configured threshold.

Reads criterion's per-benchmark `change/estimates.json` files produced by
`cargo bench -- --baseline <name>`. For each benchmark, the script compares
the median's point estimate against a percentage threshold (default 25 %).

Why 25 %: GitHub-hosted runners exhibit 10-20 % noise across runs even for
identical code; a tighter threshold would flake on every PR. Using a loose
threshold catches only genuine performance regressions (memory-allocator
thrash, accidental O(n²) loops, extra syscalls in hot paths) while staying
resistant to runner variance. The threshold is configurable for local runs
where the host is quieter.

Exit codes:
  0 — no benchmark regressed beyond the threshold.
  1 — one or more benchmarks regressed; details printed.
  2 — configuration or parse error.

Usage:
  ./benches/scripts/check_regression.py \\
      --target-dir target/criterion \\
      --threshold 0.25

The target directory is the criterion root (`target/criterion` by default),
and the threshold is a fractional regression (e.g. 0.25 = 25 %).
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def find_change_files(target_dir: Path) -> list[Path]:
    """Return every `change/estimates.json` file under `target_dir`.

    Criterion writes these when invoked with `--baseline <name>`. A fresh
    run (no baseline) produces no `change/` subdirectory, so an empty list
    means the caller forgot to run criterion with a baseline — we surface
    that as a config error rather than a silent pass.
    """
    return sorted(target_dir.glob("**/change/estimates.json"))


def benchmark_name(change_file: Path, target_dir: Path) -> str:
    """Derive a human-readable benchmark name from the file path.

    The path layout is `<target>/<group>/<bench>/change/estimates.json`;
    we return `<group>/<bench>`.
    """
    relative = change_file.relative_to(target_dir)
    parts = relative.parts[:-2]  # drop "change/estimates.json"
    return "/".join(parts) if parts else str(relative)


def parse_median_change(change_file: Path) -> float:
    """Return the median's fractional change from the baseline.

    Criterion's `change/estimates.json` stores a `median` object whose
    `point_estimate` is the fractional change (positive = slower). If the
    field is missing we raise — a malformed file is not silent-pass.
    """
    with change_file.open() as f:
        data = json.load(f)
    try:
        return float(data["median"]["point_estimate"])
    except (KeyError, TypeError, ValueError) as exc:
        raise ValueError(f"malformed {change_file}: {exc}") from exc


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
            f"error: no benchmark change files under {args.target_dir}; "
            "did you run `cargo bench -- --baseline <name>` with an "
            "existing baseline?",
            file=sys.stderr,
        )
        return 2

    regressions: list[tuple[str, float]] = []
    for change_file in change_files:
        try:
            change = parse_median_change(change_file)
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
        name = benchmark_name(change_file, args.target_dir)
        pct = change * 100.0
        marker = "REGRESSED" if change > args.threshold else "ok"
        print(f"[{marker:>9}] {name:<60} median change: {pct:+7.2f}%")
        if change > args.threshold:
            regressions.append((name, change))

    if regressions:
        print(
            f"\n{len(regressions)} benchmark(s) regressed beyond "
            f"{args.threshold * 100:.0f}%:",
            file=sys.stderr,
        )
        for name, change in regressions:
            print(f"  - {name}: median {change * 100:+.2f}%", file=sys.stderr)
        return 1

    print(
        f"\nAll {len(change_files)} benchmarks within "
        f"{args.threshold * 100:.0f}% of baseline."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
