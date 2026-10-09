#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Summarise ACTS reports into an attestation predicate, and fail if any
test in them failed or errored, at any level.

Usage: acts_predicate.py --itk-revision SHA acts-report-*.json > predicate.json

The predicate names the suite revision and, per binding, the report's SHA-256
and its tally by level (MUST/SHOULD/MAY) and result. release.yml signs it
with actions/attest onto the released .crate files, so `gh attestation
verify --predicate-type https://a2a-rust.com/attestations/acts-conformance/v1`
shows what a release was graded against and how it scored.

Stricter than upstream's `acts_report.py --require-conformant`, which fails
only on a MUST-level failure (§12.7): this repository passes every SHOULD and
MAY test too (acts/reports/), and a gate at MUST alone would let those
regress unseen. Skips do not fail it — the upstream corpus can skip a test no
agent could pass — but they are counted in the predicate.

Exit codes: 0 no test failed or errored; 1 any did, or no report, or a
report with no tests; 2 usage.
"""

import argparse
import collections
import hashlib
import json
import sys

PREDICATE_TYPE = "https://a2a-rust.com/attestations/acts-conformance/v1"


def tests(node):
    """Every object carrying id, level and result, wherever it sits."""
    if isinstance(node, dict):
        if {"id", "level", "result"} <= node.keys():
            yield node
            return
        for v in node.values():
            yield from tests(v)
    elif isinstance(node, list):
        for v in node:
            yield from tests(v)


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--itk-revision", required=True)
    ap.add_argument("reports", nargs="*")
    args = ap.parse_args(argv)
    if not args.reports:
        print("acts_predicate: no reports given", file=sys.stderr)
        return 1

    bindings = {}
    failures = []
    for path in sorted(args.reports):
        try:
            raw = open(path, "rb").read()
        except OSError:
            # An unmatched shell glob arrives here literally.
            print(f"acts_predicate: no report at {path}", file=sys.stderr)
            return 1
        report = json.loads(raw)
        found = list(tests(report))
        if not found:
            print(f"acts_predicate: {path} holds no tests", file=sys.stderr)
            return 1
        tally = collections.Counter((t["level"], t["result"]) for t in found)
        transport = report.get("transport", path)
        bindings[transport] = {
            "report": path.rsplit("/", 1)[-1],
            "sha256": hashlib.sha256(raw).hexdigest(),
            "tally": {f"{lvl}:{res}": n for (lvl, res), n in sorted(tally.items())},
        }
        failures += [f"{transport} {t['id']} ({t['level']}): {t['result']}"
                     for t in found if t["result"] in ("fail", "error")]

    json.dump({"predicateType": PREDICATE_TYPE,
               "suite": {"repository": "https://github.com/a2aproject/a2a-itk",
                         "revision": args.itk_revision},
               "bindings": bindings,
               "noFailures": not failures}, sys.stdout, indent=2, sort_keys=True)
    print()
    for f in failures:
        print(f"acts_predicate: failed: {f}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
