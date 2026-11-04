#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Apply mutants to one source file by hand, one at a time, and run a test command on each.

`cargo mutants` builds every test target of the package before it mutates anything, which
fills a small disk, and its sharded CI run is the arbiter for the server crate. This is the
local stand-in the kill-proof protocol asks for: each mutant is an exact text replacement
that you wrote, the test command is as narrow as you make it, the file is restored when the
run ends however it ends, and a mutant that hangs is killed and reported, not waited for.

    scripts/hand_mutate.py FILE MUTANTS.json 'TEST COMMAND' [--timeout SECONDS]
    scripts/hand_mutate.py --self-test

MUTANTS.json is a list of {"name": ..., "old": ..., "new": ...}. `old` must occur in FILE
exactly once; a mutant whose pattern occurs any other number of times is skipped and
counted, never applied. The test command runs under `bash -c` in a process group of its
own, from the current directory.

A mutant is
  CAUGHT    the command exited non-zero (a test failed);
  UNVIABLE  the command exited non-zero and printed a Rust compile error (`error[E`) or
            `could not compile`, so no test ran;
  TIMEOUT   the command ran past --timeout (default 900 s) and its process group was
            killed: the mutant hangs, as one that loops forever on malformed input does.
            cargo-mutants scores a timeout as neither caught nor missed, and so does this;
  MISSED    the command exited 0. Write a test that fails on it, apply the mutant by hand,
            watch it fail, restore, and run this again.

Exit status: 0 when nothing was MISSED and nothing was skipped, 1 otherwise, 2 for a usage
error. The summary line counts all five outcomes.
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import tempfile
from pathlib import Path

DEFAULT_TIMEOUT = 900


def run_command(cmd: str, timeout: float) -> tuple[str, str]:
    """Run `cmd`; return ("done"|"timeout", combined output)."""
    proc = subprocess.Popen(
        ["bash", "-c", cmd],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    try:
        out, _ = proc.communicate(timeout=timeout)
        return ("done", out or "") if proc.returncode == 0 else (f"exit {proc.returncode}", out or "")
    except subprocess.TimeoutExpired:
        os.killpg(proc.pid, signal.SIGKILL)
        out, _ = proc.communicate()
        return "timeout", out or ""


def classify(status: str, output: str) -> str:
    if status == "timeout":
        return "TIMEOUT"
    if status == "done":
        return "MISSED"
    if "error[E" in output or "could not compile" in output:
        return "UNVIABLE"
    return "CAUGHT"


def run(path: Path, mutants: list[dict], cmd: str, timeout: float) -> dict[str, int]:
    original = path.read_text(encoding="utf-8")
    counts = {"CAUGHT": 0, "UNVIABLE": 0, "TIMEOUT": 0, "MISSED": 0, "SKIPPED": 0}
    try:
        for m in mutants:
            name, old, new = m["name"], m["old"], m["new"]
            n = original.count(old)
            if n != 1:
                counts["SKIPPED"] += 1
                print(f"SKIPPED  {name}: the pattern occurs {n} times, not once", flush=True)
                continue
            path.write_text(original.replace(old, new), encoding="utf-8")
            outcome = classify(*run_command(cmd, timeout))
            counts[outcome] += 1
            print(f"{outcome:<8} {name}", flush=True)
    finally:
        path.write_text(original, encoding="utf-8")
    return counts


def summary(counts: dict[str, int]) -> str:
    return "SUMMARY " + " ".join(f"{k.lower()}={v}" for k, v in counts.items())


def self_test() -> int:
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / "src.txt"
        f.write_text("alpha beta gamma\n", encoding="utf-8")
        mutants = [
            {"name": "caught", "old": "alpha", "new": "ALPHA"},
            {"name": "missed", "old": "beta", "new": "beta"},
            {"name": "unviable", "old": "gamma", "new": "error[E0000]"},
            {"name": "timeout", "old": "alpha", "new": "alpha SLEEP"},
            {"name": "skipped", "old": "absent", "new": "x"},
            {"name": "ambiguous", "old": "a", "new": "b"},
        ]
        # The command reads the file the way a test would. It fails when ALPHA or the
        # compile error is present, hangs when SLEEP is, and passes otherwise.
        cmd = (
            f'f={f}; grep -q ALPHA "$f" && exit 1; '
            f'grep -q "error\\[E" "$f" && {{ echo "error[E0000]"; exit 1; }}; '
            f'grep -q SLEEP "$f" && sleep 60; exit 0'
        )
        got = run(f, mutants, cmd, timeout=2)
        want = {"CAUGHT": 1, "UNVIABLE": 1, "TIMEOUT": 1, "MISSED": 1, "SKIPPED": 2}
        restored = f.read_text(encoding="utf-8") == "alpha beta gamma\n"
    if got != want or not restored:
        print(f"hand_mutate --self-test: FAILED\n  got  {got}\n  want {want}\n  file restored: {restored}")
        return 1
    print("hand_mutate --self-test: caught, missed, unviable, timeout and skipped each reported; file restored")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    timeout = float(DEFAULT_TIMEOUT)
    if "--timeout" in argv:
        i = argv.index("--timeout")
        try:
            timeout = float(argv[i + 1])
        except (IndexError, ValueError):
            print(__doc__)
            return 2
        argv = argv[:i] + argv[i + 2 :]
    if len(argv) != 3:
        print(__doc__)
        return 2
    path, spec, cmd = Path(argv[0]), Path(argv[1]), argv[2]
    counts = run(path, json.loads(spec.read_text(encoding="utf-8")), cmd, timeout)
    print(summary(counts))
    return 0 if counts["MISSED"] == 0 and counts["SKIPPED"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
