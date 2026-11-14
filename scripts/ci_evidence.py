#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Refuses a release unless main CI passed for the exact commit being released.

The release workflow's own test matrix is a reduced copy of `ci.yml`: it ran the
workspace's all-feature tests on three systems, clippy, fmt, docs and the MSRV,
and none of `ci.yml`'s feature combinations, its no-default-features leg on
macOS, its PostgreSQL integration job or its static gates (adversarial review
N13, 2026-11-13). A signature over the artifacts says who built them, not that
these passed. This script reads, through the GitHub API, the `ci.yml` runs for
the tagged commit, and passes only when:

  * a `push` or `workflow_dispatch` run of `ci.yml` for that exact SHA has
    completed (a `pull_request` run with the same head SHA tested a merge
    with main, not the commit, and is not counted); the newest such run, at
    its latest attempt, is the one read, so a later red run is not hidden by
    an earlier green one;
  * its conclusion is `success`;
  * no job in it ended `failure`, `cancelled`, `timed_out`, `action_required`,
    `startup_failure` or `stale`, or did not end at all.

Jobs that were skipped are listed, so an unexpectedly skipped gate is seen in
the release's evidence rather than read as a pass. The evidence (run id, URL,
conclusion, every job and its conclusion) is written as JSON for the release.

Usage
-----
    scripts/ci_evidence.py --repo OWNER/NAME --sha SHA --out ci-evidence.json
    scripts/ci_evidence.py --self-test

Uses `gh api` (GH_TOKEN in the environment). Exit codes: 0 the commit passed
CI, 1 it did not (or CI has not finished for it), 2 the API could not be read.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys

EVENTS = {"push", "workflow_dispatch"}
BAD = {"failure", "cancelled", "timed_out", "action_required", "startup_failure", "stale", None}


def newest(runs: list[dict]) -> dict | None:
    """The newest completed push or dispatch run, or None."""
    completed = [r for r in runs if r.get("status") == "completed" and r.get("event") in EVENTS]
    return max(completed, key=lambda r: (r.get("created_at", ""), r.get("run_attempt", 1)), default=None)


def decide(runs: list[dict], jobs: list[dict]) -> tuple[bool, list[str], dict]:
    """Whether the evidence passes, the reasons it does not, and the record.

    `runs` are the `ci.yml` workflow runs for the commit; `jobs` the jobs of
    the run chosen (the latest completed attempt), or empty when there is none.
    """
    run = newest(runs)
    if run is None:
        return False, ["no completed push or dispatch run of ci.yml for this commit (has CI finished?)"], {}
    reasons = []
    if run.get("conclusion") != "success":
        reasons.append(f"ci.yml run {run.get('id')} concluded {run.get('conclusion')!r}")
    bad = [j for j in jobs if j.get("conclusion") in BAD]
    for j in bad:
        reasons.append(f"job {j.get('name')!r} ended {j.get('conclusion')!r}")
    if not jobs:
        reasons.append(f"ci.yml run {run.get('id')} lists no jobs")
    record = {
        "workflow": "ci.yml",
        "run_id": run.get("id"),
        "run_attempt": run.get("run_attempt"),
        "url": run.get("html_url"),
        "head_sha": run.get("head_sha"),
        "conclusion": run.get("conclusion"),
        "jobs": sorted(
            ({"name": j.get("name"), "conclusion": j.get("conclusion")} for j in jobs),
            key=lambda j: j["name"] or "",
        ),
        "skipped": sorted(j.get("name") or "" for j in jobs if j.get("conclusion") == "skipped"),
    }
    return not reasons, reasons, record


def gh(path: str) -> dict:
    out = subprocess.run(["gh", "api", "-X", "GET", path], capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"gh api {path}: {out.stderr.strip()}")
    return json.loads(out.stdout)


def fetch(repo: str, sha: str) -> tuple[list[dict], list[dict]]:
    runs = gh(f"repos/{repo}/actions/workflows/ci.yml/runs?head_sha={sha}&per_page=100")
    runs = [r for r in runs.get("workflow_runs", []) if r.get("head_sha") == sha]
    run = newest(runs)
    if run is None:
        return runs, []
    jobs: list[dict] = []
    page = 1
    while True:
        got = gh(f"repos/{repo}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100&page={page}")
        jobs.extend(got.get("jobs", []))
        if len(got.get("jobs", [])) < 100:
            return runs, jobs
        page += 1


def self_test() -> int:
    ok = {"id": 1, "status": "completed", "conclusion": "success", "run_attempt": 1,
          "created_at": "2026-01-01T00:00:00Z", "head_sha": "s", "html_url": "u", "event": "push"}
    later = {**ok, "id": 2, "created_at": "2026-01-02T00:00:00Z"}
    green = [{"name": "Test", "conclusion": "success"}, {"name": "Bench", "conclusion": "skipped"}]
    cases = [
        ("a green run passes, its skipped job listed", [ok], green, True),
        ("no run at all fails", [], [], False),
        ("a run still going fails", [{**ok, "status": "in_progress", "conclusion": None}], [], False),
        ("a red run fails", [{**ok, "conclusion": "failure"}], green, False),
        ("a cancelled job fails even in a green run", [ok],
         green + [{"name": "Test (macOS)", "conclusion": "cancelled"}], False),
        ("a job that never ended fails", [ok], green + [{"name": "x", "conclusion": None}], False),
        ("a run with no jobs fails", [ok], [], False),
        ("a newer green run decides over an older red one",
         [{**ok, "conclusion": "failure"}, later], green, True),
        ("a newer red run is not hidden by an older green one",
         [ok, {**later, "conclusion": "failure"}], green, False),
        ("a re-run older run does not outrank a newer one",
         [{**ok, "run_attempt": 2}, {**later, "conclusion": "failure"}], green, False),
        ("a pull_request run does not count", [{**ok, "event": "pull_request"}], green, False),
        ("a dispatch run counts", [{**ok, "event": "workflow_dispatch"}], green, True),
    ]
    failures = 0
    for name, runs, jobs, want in cases:
        got, reasons, record = decide(runs, jobs)
        if got != want:
            failures += 1
            print(f"FAIL {name}: got {got}, reasons {reasons}")
    passed, _, record = decide([ok], green)
    if record.get("skipped") != ["Bench"]:
        failures += 1
        print(f"FAIL the skipped list: {record.get('skipped')}")
    print(f"ci_evidence self-test: {len(cases) + 1 - failures} of {len(cases) + 1} passed")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--repo")
    ap.add_argument("--sha")
    ap.add_argument("--out", default="ci-evidence.json")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    if not args.repo or not args.sha:
        ap.error("--repo and --sha are required")
    try:
        runs, jobs = fetch(args.repo, args.sha)
    except (RuntimeError, json.JSONDecodeError, KeyError) as e:
        print(f"::error::the CI evidence could not be read: {e}")
        return 2
    passed, reasons, record = decide(runs, jobs)
    with open(args.out, "w", encoding="utf-8") as f:
        json.dump({"passed": passed, "reasons": reasons, **record}, f, indent=1)
        f.write("\n")
    if record.get("skipped"):
        print(f"Skipped in ci.yml run {record['run_id']}: {', '.join(record['skipped'])}")
    for reason in reasons:
        print(f"::error::{reason}")
    if passed:
        print(f"ci.yml passed for {args.sha}: run {record['run_id']} ({record['url']}), {len(record['jobs'])} jobs")
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
