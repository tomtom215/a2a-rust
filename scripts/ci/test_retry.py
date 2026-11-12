#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Behavioural test of scripts/ci/retry.sh.

Each case runs a command that counts its own invocations in a file, so the
number of attempts is read, not assumed:

  first_try      succeeds at once: exit 0, one attempt
  recovers       fails twice then succeeds: exit 0, three attempts
  gives_up       fails every time with status 7: exit 7, exactly COUNT attempts
  keeps_last     fails with 3, then 9: exit 9 (the last attempt's status)
  bad_count      COUNT of 0 or not a number: exit 2, the command never runs
  missing_dashes no `--` separator: exit 2, the command never runs

Usage: scripts/ci/test_retry.py          (exit 0 all pass, 1 a failure)
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent / "retry.sh"

# `$0` is the counter file, `$1` the comma-separated exit statuses to return in
# order (the last one repeats).
COUNTER = r"""
n=$(cat "$0" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "$0"
IFS=, read -ra codes <<< "$1"
i=$((n - 1)); [ "$i" -ge "${#codes[@]}" ] && i=$((${#codes[@]} - 1))
exit "${codes[$i]}"
"""


def run(count: str, counter: Path, codes: str, dashes: bool = True) -> subprocess.CompletedProcess[str]:
    argv = ["bash", str(SCRIPT), count] + (["--"] if dashes else []) + ["bash", "-c", COUNTER, str(counter), codes]
    env = dict(os.environ, RETRY_DELAY="0")
    return subprocess.run(argv, env=env, capture_output=True, text=True, timeout=60, check=False)


def attempts(counter: Path) -> int:
    return int(counter.read_text()) if counter.exists() else 0


def main() -> int:
    failures: list[str] = []

    def check(name: str, ok: bool, detail: str) -> None:
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: {detail}")
        if not ok:
            failures.append(name)

    with tempfile.TemporaryDirectory(prefix="retry-test.") as tmp:
        t = Path(tmp)

        c = t / "first_try"
        r = run("5", c, "0")
        check("first_try", r.returncode == 0 and attempts(c) == 1, f"exit {r.returncode}, {attempts(c)} attempt(s)")

        c = t / "recovers"
        r = run("5", c, "1,1,0")
        check("recovers", r.returncode == 0 and attempts(c) == 3, f"exit {r.returncode}, {attempts(c)} attempt(s) (want 3)")

        c = t / "gives_up"
        r = run("4", c, "7")
        check("gives_up", r.returncode == 7 and attempts(c) == 4, f"exit {r.returncode}, {attempts(c)} attempt(s) (want 7 and 4)")

        c = t / "keeps_last"
        r = run("2", c, "3,9")
        check("keeps_last", r.returncode == 9 and attempts(c) == 2, f"exit {r.returncode}, {attempts(c)} attempt(s) (want 9 and 2)")

        for bad in ("0", "x", "-1"):
            c = t / f"bad_count_{bad}"
            r = run(bad, c, "0")
            check(f"bad_count {bad!r}", r.returncode == 2 and attempts(c) == 0, f"exit {r.returncode}, {attempts(c)} attempt(s)")

        c = t / "missing_dashes"
        r = run("3", c, "0", dashes=False)
        check("missing_dashes", r.returncode == 2 and attempts(c) == 0, f"exit {r.returncode}, {attempts(c)} attempt(s)")

    if failures:
        print(f"\nretry: {len(failures)} failing: {', '.join(failures)}")
        return 1
    print("\nretry: every case behaves as stated")
    return 0


if __name__ == "__main__":
    sys.exit(main())
