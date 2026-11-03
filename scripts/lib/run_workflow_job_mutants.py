#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215/a2a-rust)
"""Prove the self-test of scripts/run_workflow_job.py can fail.

A self-test that passes proves nothing until it is shown to fail on a wrong
runner. This applies each mutant (one exact text replacement in the runner),
runs the self-test, requires it to fail, and restores the file whatever
happens. A mutant whose `old` text is not found exactly once is an error, not a
kill: it means the runner moved and this list is stale.

Usage: scripts/lib/run_workflow_job_mutants.py
Exit: 0 every mutant was killed; 1 a mutant survived; 2 a mutant did not apply.
"""

from __future__ import annotations

import hashlib
import signal
import subprocess
import sys
from pathlib import Path

RUNNER = Path(__file__).resolve().parents[1] / "run_workflow_job.py"

MUTANTS = [
    ("INCOMPLETE exits 0", '"INCOMPLETE": 3}', '"INCOMPLETE": 0}'),
    ("FAIL exits 0", '"FAIL": 1,', '"FAIL": 0,'),
    ("a non-zero exit is recorded as ok", "            if code == 0:\n                rec[\"status\"] = \"ok\"", "            if code != 99:\n                rec[\"status\"] = \"ok\""),
    ("sudo is no longer refused", 'r"\\bsudo\\b"', 'r"\\bsudoXX\\b"'),
    ("git push is no longer refused", 'r"\\bgit\\s+push\\b"', 'r"\\bgit\\s+pushXX\\b"'),
    ("incomplete reasons do not change the verdict", "    elif result[\"incomplete\"]:\n        result[\"verdict\"] = \"INCOMPLETE\"", "    elif False:\n        result[\"verdict\"] = \"INCOMPLETE\""),
    ("a step it cannot evaluate is run anyway", "cond = evaluate_if(step.get(\"if\"), failed, event, outcomes)\n", "cond = evaluate_if(step.get(\"if\"), failed, event, outcomes) or True\n"),
    ("continue-on-error is ignored", "elif step.get(\"continue-on-error\"):", "elif False:"),
    ("the default shell gains pipefail", "[\"-e\"] if \"shell\" not in step and not default_run.get(\"shell\") else", "[\"--noprofile\", \"--norc\", \"-eo\", \"pipefail\"] if True else"),
    ("a failed step does not stop the job", "                rec[\"status\"] = \"failed\"\n                failed = True", "                rec[\"status\"] = \"failed\"\n                failed = False"),
    ("every action counts as set-up", "if action in SETUP_ACTIONS or action in PLUMBING_ACTIONS:", "if True:"),
    ("services are ignored", "if job.get(\"services\") and not services_ready:", "if False:"),
    ("a non-Linux runner is accepted", "if not runs_on.startswith(\"ubuntu\"):", "if False:"),
    ("GITHUB_ENV is not carried forward", "                base_env[k] = v\n            extra", "                pass\n            extra"),
    ("a timeout counts as success", "code = 124", "code = 0"),
    ("a job-level if: false still runs", "if job_if is False:", "if job_if is None and False:"),
    ("a matrix value is not substituted", "return ctx[\"matrix\"][expr[7:]]", "return m.group(0)"),
    ("working-directory is ignored", "cwd = root / (step.get(\"working-directory\") or default_run.get(\"working-directory\") or \".\")", "cwd = root"),
    ("--expr values are ignored", "    if expr in ctx[\"expr\"]:\n        return ctx[\"expr\"][expr]\n", "    if False:\n        return \"\"\n"),
    ("a job env expression with no answer lets the steps run", "    if result[\"incomplete\"]:\n        # A step cannot be told apart", "    if False:\n        # A step cannot be told apart"),
    ("|| takes the last operand", "        for operand in (o.strip() for o in m.group(1).split(\"||\")):", "        for operand in reversed([o.strip() for o in m.group(1).split(\"||\")]):"),
    ("|| never falls through", "            if value:\n                return value", "            if value is not None or True:\n                return value or m.group(0)"),
    ("left-running servers are not stopped", "    finally:\n        reap(groups)", "    finally:\n        pass"),
    ("cleanup looks at the wrong field for the group", "int(fields[2]) == group:  # state", "int(fields[1]) == group:  # state"),
    ("cleanup never sends SIGKILL", "for sig, grace in ((signal.SIGTERM, 3.0), (signal.SIGKILL, 1.0)):", "for sig, grace in ((signal.SIGTERM, 3.0),):"),
    ("a timed-out step's group is not killed", "os.killpg(proc.pid, signal.SIGKILL)\n                        proc.wait()", "proc.kill()\n                        proc.wait()"),
    ("|| in an if: is read as &&", "        any_true = any_true or all_true", "        any_true = any_true and all_true"),
    ("&& in an if: is read as ||", "            all_true = all_true and v", "            all_true = all_true or v"),
    ("!= in an if: is read as ==", "v = (actual == m.group(3)) == (m.group(2) == \"==\")", "v = (actual == m.group(3)) == True"),
    ("a term it cannot read is taken as true", "                if not m:\n                    return None", "                if not m:\n                    continue"),
    ("a condition with no status function ignores an earlier failure", "return value if STATUS_FN.search(c) else (value and not failed_so_far)", "return value"),
    ("conclusion ignores continue-on-error", "out[r[\"id\"]] = {\"outcome\": \"failure\", \"conclusion\": \"success\"}", "out[r[\"id\"]] = {\"outcome\": \"failure\", \"conclusion\": \"failure\"}"),
    ("a skipped set-up action has no outcome", "if status == \"ok\" or (status == \"skipped\" and r.get(\"plumbing\")):", "if status == \"ok\":"),
    ("an unknown step id is read as an empty outcome", "                    if actual is None:\n                        return None", "                    if actual is None:\n                        actual = ''"),
    ("a step's outcome is read from the wrong key", "actual = (outcomes.get(step_id) or {}).get(which)", "actual = (outcomes.get(step_id) or {}).get('outcome')"),
    ("secrets resolve to nothing instead of INCOMPLETE", "        unresolved.append(m.group(1))\n        return m.group(0)", "        return \"\""),
]


def sha(p: Path) -> str:
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main() -> int:
    # A signal must not leave the runner mutated: turn SIGTERM into an exit the
    # `finally` below sees, and take SIGINT even when the shell started this in
    # the background (where SIGINT is ignored). An earlier version did neither,
    # and an interrupted run left one mutant in the file.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    signal.signal(signal.SIGINT, signal.default_int_handler)
    original = RUNNER.read_bytes()
    before = sha(RUNNER)
    survived: list[str] = []
    stale: list[str] = []
    try:
        for name, old, new in MUTANTS:
            text = original.decode("utf-8")
            if text.count(old) != 1:
                stale.append(f"{name}: old text found {text.count(old)} times")
                print(f"STALE    {name}")
                continue
            RUNNER.write_text(text.replace(old, new), encoding="utf-8")
            p = subprocess.run([sys.executable, "-I", str(RUNNER), "--self-test"],
                               capture_output=True, text=True)
            failing = [l for l in p.stdout.splitlines() if l.startswith("FAIL")]
            if p.returncode == 0:
                survived.append(name)
                print(f"SURVIVED {name}")
            else:
                print(f"killed   {name}  ({len(failing)} case(s) failed, first: {failing[0][5:] if failing else 'crash'})")
    finally:
        RUNNER.write_bytes(original)
    after = sha(RUNNER)
    print(f"\nrunner sha256 before {before[:16]}…  after {after[:16]}…  restored={before == after}")
    print(f"{len(MUTANTS) - len(survived) - len(stale)} killed, {len(survived)} survived, {len(stale)} did not apply")
    if stale:
        return 2
    return 1 if survived else 0


if __name__ == "__main__":
    sys.exit(main())
