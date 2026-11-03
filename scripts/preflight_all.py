#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""One command: every gate this machine can run, each with its own exit code.

    scripts/preflight_all.py

runs, in this order, and prints one table at the end:

  1. every `ci.yml` gate, through `scripts/preflight.sh --full` (the gate list
     is read from the workflow, not restated), one row per gate command;
  2. the jobs of the other workflows that `scripts/lib/workflow_inventory.py`
     files under `run`, through `scripts/run_workflow_job.py`;
  3. the `local` equivalents of the two `uses:`-only blocking jobs.

Why a table of exit codes and not a verdict: a wrapper that returns the last
command's status hid a real failure from the maintainer once. Each row here is
that gate's own status, taken from the gate and not from a pipeline.

Rows are PASS (exit 0), FAIL (a non-zero exit), or INCOMPLETE (exit 3 from the
job runner: it ran what it could and names what it could not; never counted as
a pass). The process exits 1 if any row failed, else 3 if any row is
INCOMPLETE or a phase reported fewer gates than it was asked to run, else 0.
Jobs that only CI can run are listed after the table with their reasons; they
are not rows.

    --skip-ci-yml   leave out phase 1 (to re-run only the other workflows)
    --jobs a,b      phase 1 runs only those `ci.yml` jobs' gates (preflight.sh --jobs); the
                    table then names the `ci.yml` jobs that were left out
    --only NAME     run only the job or gate whose name contains NAME
    --list          print what would run and exit

Needs what `scripts/preflight.sh --list` says it needs (PostgreSQL on :5432,
SPIRE binaries, Go, ssh-keygen); a gate whose service is missing fails, as it should.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts/lib"))
import workflow_inventory  # noqa: E402


def sh(*cmd: str) -> str:
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()


def stream(cmd: list[str], log: Path, env: dict | None = None) -> int:
    """Run cmd, echo its output as it comes, keep it in log, return its own exit status."""
    with log.open("w", encoding="utf-8") as fh:
        p = subprocess.Popen(cmd, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                             text=True, errors="replace")
        assert p.stdout is not None
        for line in p.stdout:
            sys.stdout.write(line)
            fh.write(line)
        return p.wait()


def expected_ci_yml_gates(jobs: str | None) -> int:
    # --jobs has to come before --list: preflight.sh acts on --list when it reads it.
    out = sh("bash", "scripts/preflight.sh", *(["--jobs", jobs] if jobs else []), "--list")
    for line in out.splitlines():
        if "--full" in line and "command(s)" in line:
            n = int(line.split()[1])
            if n == 0:
                raise SystemExit("preflight_all: preflight.sh --full would run 0 gates; refusing")
            return n
    raise SystemExit("preflight_all: could not read the --full gate count from preflight.sh --list")


def verdict_of(code: int) -> str:
    return {0: "PASS", 3: "INCOMPLETE"}.get(code, "FAIL")


def main(argv: list[str]) -> int:
    # Line-buffered, so a log file or a pipe shows progress as it happens.
    sys.stdout.reconfigure(line_buffering=True)
    only = argv[argv.index("--only") + 1] if "--only" in argv else None
    jobs = argv[argv.index("--jobs") + 1] if "--jobs" in argv else None
    skip_ci = "--skip-ci-yml" in argv
    if jobs:
        unknown = sorted(set(jobs.split(",")) - workflow_inventory.gate_jobs())
        if unknown:
            print(f"preflight_all: --jobs names {unknown}, which are not ci.yml gate jobs "
                  f"({', '.join(sorted(workflow_inventory.gate_jobs()))})", file=sys.stderr)
            return 2
    table, problems = workflow_inventory.classify()
    if problems:
        for p in problems:
            print(f"preflight_all: workflow inventory drift: {p}", file=sys.stderr)
        print("preflight_all: refusing to run on an inventory that is not complete "
              "(scripts/lib/workflow_inventory.py)", file=sys.stderr)
        return 2
    run_jobs = [(k, v) for k, v in table.items() if v["mode"] == "run"]
    local_jobs = [(k, v) for k, v in table.items() if v["mode"] == "local"]
    ci_only = [(k, v) for k, v in table.items() if v["mode"] == "ci-only"]
    if "--list" in argv:
        print(f"phase 1: {'skipped' if skip_ci else str(expected_ci_yml_gates(jobs)) + ' ci.yml gates via preflight.sh --full' + (' --jobs ' + jobs if jobs else '')}")
        for (wf, j), v in run_jobs:
            print(f"phase 2: {wf}:{j} {' '.join(v['args'])}")
        for (wf, j), v in local_jobs:
            print(f"phase 3: {wf}:{j} (local equivalent) {v['note']}")
        for (wf, j), v in ci_only:
            print(f"ci only: {wf}:{j} - {v['note']}")
        return 0

    logs = Path(tempfile.mkdtemp(prefix="a2a-preflight-all."))
    rows: list[dict] = []  # name, code, verdict, seconds, note
    short: list[str] = []  # phases that reported fewer gates than asked for
    print(f"preflight_all: logs in {logs}; HEAD {sh('git', 'rev-parse', '--short', 'HEAD')}, "
          f"{len(sh('git', 'status', '--porcelain').splitlines())} uncommitted files")

    if not skip_ci:
        results = logs / "ci-yml-results.tsv"
        want = expected_ci_yml_gates(jobs)
        env = dict(os.environ, PREFLIGHT_RESULTS=str(results))
        t0 = time.monotonic()
        code = stream(["bash", "scripts/preflight.sh", "--full", *(["--jobs", jobs] if jobs else [])],
                      logs / "preflight-ci-yml.log", env)
        got = 0
        if results.exists():
            for line in results.read_text(encoding="utf-8").splitlines():
                status, secs, cmd = line.split("\t", 2)
                if only and only not in cmd:
                    continue
                got += 1
                rows.append({"name": f"ci.yml  {cmd}", "code": int(status), "verdict": verdict_of(int(status)),
                             "seconds": float(secs), "note": ""})
        if got < want and not only:
            short.append(f"preflight.sh --full reported {got} of {want} gates (its own exit status was {code})")
            rows.append({"name": "ci.yml  preflight.sh --full (process)", "code": code or 3,
                         "verdict": "FAIL" if code else "INCOMPLETE", "seconds": time.monotonic() - t0,
                         "note": f"{got} of {want} gates reported"})
        elif code != 0 and not any(r["code"] for r in rows):
            rows.append({"name": "ci.yml  preflight.sh --full (process)", "code": code, "verdict": "FAIL",
                         "seconds": time.monotonic() - t0, "note": "non-zero with every gate at 0"})

    base = sh("git", "rev-parse", "origin/main")
    head = sh("git", "rev-parse", "HEAD")
    for (wf, j), v in run_jobs:
        name = f"{wf}:{j}"
        if only and only not in name:
            continue
        args = [a.format(base=base, head=head) for a in v["args"]]
        out = logs / f"{wf[:-4]}-{j}.json"
        t0 = time.monotonic()
        code = stream([sys.executable, "-I", "scripts/run_workflow_job.py", "--log-dir", str(logs / f"{wf[:-4]}-{j}"),
                       "--json", str(out), *args, wf, j], logs / f"{wf[:-4]}-{j}.log")
        note = ""
        if out.exists():
            r = json.loads(out.read_text(encoding="utf-8"))
            allowed = [s for s in r["steps"] if s["status"] == "failed-allowed"]
            note = "; ".join([f"step {s['n']} {s['name']!r} exit {s['exit']} (continue-on-error)" for s in allowed]
                             + r["incomplete"])
        rows.append({"name": f"{wf}:{j}", "code": code, "verdict": verdict_of(code),
                     "seconds": time.monotonic() - t0, "note": note})

    for (wf, j), v in local_jobs:
        name = f"{wf}:{j} (local equivalent)"
        if only and only not in name:
            continue
        print(f"\n▶ {name}: {v['note']}", flush=True)
        t0 = time.monotonic()
        code = stream(["bash", "-c", v["note"]], logs / f"{wf[:-4]}-{j}.log")
        rows.append({"name": name, "code": code, "verdict": "PASS" if code == 0 else "FAIL",
                     "seconds": time.monotonic() - t0, "note": ""})

    print("\n── preflight_all: every gate, with its own exit code ──")
    for r in rows:
        print(f"  {r['verdict']:<10} exit {r['code']:>3} {r['seconds']:>7.0f}s  {r['name']}"
              + (f"\n{'':>34}{r['note']}" if r["note"] else ""))
    n = {v: sum(1 for r in rows if r["verdict"] == v) for v in ("PASS", "FAIL", "INCOMPLETE")}
    print(f"\n  {len(rows)} rows: {n['PASS']} PASS, {n['FAIL']} FAIL, {n['INCOMPLETE']} INCOMPLETE")
    for s in short:
        print(f"  short: {s}")
    if jobs and not skip_ci:
        left_out = sorted(workflow_inventory.gate_jobs() - set(jobs.split(",")))
        print(f"\n  ci.yml jobs NOT run (--jobs {jobs}): {', '.join(left_out)}")
    if ci_only:
        print(f"\n  Not run here ({len(ci_only)} jobs; CI is the only check of these):")
        for (wf, j), v in ci_only:
            print(f"    {wf}:{j} - {v['note']}")
    print(f"\n  HEAD {head[:8]}; a result belongs to this commit and to this working tree only.")
    if n["FAIL"]:
        return 1
    if n["INCOMPLETE"] or short or not rows:
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
