#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Merges driver output directories into one JSON line per run, with a timeline.

    timeline.py DIR [DIR ...] > results.jsonl

Each DIR is a directory delegation_live.py wrote (it holds results.json and one
<scenario>-<run>/ directory per run). A line is the run's results.json entry plus
`timeline`: seconds from the supervisor's launch (its BEGIN line) to the first and
last `token-fired`, `cancel-hook` and `completed` line of the worker, and the
number of each. The lease stops and completions of a run, in the run's own clock.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

WORKER = re.compile(r"^\[[^\]]*\] t=(\d+) (completed|token-fired|cancel-hook) ")
BEGIN = re.compile(r"^t=(\d+) BEGIN ")


def timeline(run_dir: pathlib.Path) -> dict:
    begin = next((int(m[1]) for m in map(BEGIN.match, (run_dir / "supervisor.log").read_text().splitlines()) if m), None)
    seen: dict[str, list[int]] = {"completed": [], "token-fired": [], "cancel-hook": []}
    for line in (run_dir / "worker.log").read_text().splitlines():
        if m := WORKER.match(line):
            seen[m[2]].append(int(m[1]))
    out = {}
    for kind, stamps in seen.items():
        if stamps and begin:
            out[kind] = {"n": len(stamps), "first_s": round((min(stamps) - begin) / 1000, 1), "last_s": round((max(stamps) - begin) / 1000, 1)}
    return out


def main() -> int:
    for arg in sys.argv[1:]:
        base = pathlib.Path(arg)
        for r in json.loads((base / "results.json").read_text()):
            r["timeline"] = timeline(base / f"{r['scenario']}-{r['run']}")
            print(json.dumps(r, separators=(",", ":"), sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
