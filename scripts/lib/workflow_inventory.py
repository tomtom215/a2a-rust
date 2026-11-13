#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Every job of every workflow, and what `scripts/preflight_all.py` does with it.

`scripts/preflight.sh` reads `ci.yml` only. The other workflows carry jobs that
block a merge, and a gate list kept from memory missed their steps. This file is
the list that is not from memory: it reads the workflow files, and every job in
them must be filed under exactly one of four headings here, or `--check` fails.

    preflight   a `ci.yml` job whose `run:` steps `scripts/preflight.sh --full`
                runs (the set is read from GATE_JOBS in scripts/lib/ci_gates.sh)
    run         run by scripts/run_workflow_job.py from the workflow's own steps
    local       a `uses:`-only job whose action has a command-line equivalent;
                the command here is copied from the workflow's `with:` and is not
                the workflow's own step, so it is labelled so wherever it prints
    ci-only     not run here; the reason is a fact about the job, not a verdict

A job that is filed but does not exist, or is filed twice, fails the check too:
a stale entry reads as a decision and covers nothing.

Usage:
    scripts/lib/workflow_inventory.py --check    # exit 0 complete, 2 drift
    scripts/lib/workflow_inventory.py --table    # every job, its heading, its reason
    scripts/lib/workflow_inventory.py --json     # the same, for preflight_all.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
WORKFLOWS = ROOT / ".github/workflows"

# (workflow, job) -> extra arguments for run_workflow_job.py. `{base}` and
# `{head}` are filled in by preflight_all.py: the tip of origin/main and HEAD.
RUN: dict[tuple[str, str], list[str]] = {
    ("dco.yml", "dco"): [
        "--expr", "github.event.pull_request.base.sha={base}",
        "--expr", "github.event.pull_request.head.sha={head}",
    ],
    ("tck.yml", "tck-self-test"): [],
    ("tck.yml", "tck-all-bindings"): [],
    ("tck.yml", "grpc-wire-compat"): [],
    ("tck.yml", "official-client-vs-rust-server"): [],
    ("tck.yml", "card-signature-interop"): [],
    ("itk.yml", "itk-traversal-selftest"): [],
    ("verification.yml", "loom"): [],
    ("official-tck.yml", "official-tck"): [],
    ("coverage.yml", "ignores-applied"): ["--event", "workflow_dispatch"],
    ("observability.yml", "promtool"): [],
}

# (workflow, job) -> scratch paths the job creates at a fixed place and cannot create twice. A hosted
# runner starts empty; this machine does not, so preflight_all.py removes these before the job starts
# (the second local run of official-tck failed on `git clone` into the first run's /tmp/a2a-tck).
# Absolute paths under /tmp only, no globs: preflight_all.py refuses anything else.
CLEAN: dict[tuple[str, str], list[str]] = {
    ("official-tck.yml", "official-tck"): ["/tmp/a2a-tck"],
}

# (workflow, job) -> the command-line equivalent of a `uses:`-only job.
LOCAL: dict[tuple[str, str], str] = {
    # ci.yml `deny`: cargo-deny-action with `command: check`.
    ("ci.yml", "deny"): "cargo deny check",
    # ci.yml `semver`: cargo-semver-checks-action, four packages, feature-group
    # all-features. The baseline is the newest crates.io release, as the action's is.
    ("ci.yml", "semver"): (
        "cargo semver-checks --package a2a-protocol-types --package a2a-protocol-client "
        "--package a2a-protocol-server --package a2a-protocol-sdk --all-features"
    ),
}

_REL = "@events"  # replaced by classify() with the workflow's own `on:` events
CI_ONLY: dict[tuple[str, str], str] = {
    ("ci.yml", "nightly"): "informational (continue-on-error) and needs the nightly toolchain",
    ("acts.yml", "acts"): "builds its image with docker/build-push-action, an action step this runner cannot run",
    ("benchmarks.yml", "bench"): "skipped on pull requests (if: event_name != 'pull_request'); on a push it ends in `git push` of the results to main",
    ("benchmarks.yml", "bench-regression"): "pull requests only: runs the benchmarks on the base branch and on the PR and compares them; not run here",
    ("codeql.yml", "analyze"): "the CodeQL action; results go to GitHub code scanning",
    ("coverage.yml", "coverage"): "uploads to Codecov with a secret token and needs a PostgreSQL service",
    ("deploy.yml", "image"): "builds its image with docker/build-push-action, an action step this runner cannot run, then runs the container",
    ("deploy.yml", "manifests"): "downloads a pinned kubeconform release from GitHub and the Kubernetes JSON schemas at run time",
    ("docs.yml", "build"): _REL + "; builds the Pages site",
    ("docs.yml", "deploy"): _REL + "; deploys the Pages site",
    ("fuzz.yml", "fuzz"): "needs the nightly toolchain and cargo-fuzz, one matrix entry per target (check_fuzz_matrix.py gates the matrix)",
    ("itk-nightly.yml", "nightly"): "builds its image with docker/build-push-action, an action step this runner cannot run (on pull requests only when itk/** or the workflow changes)",
    ("itk.yml", "itk-current-mount"): "workflow_dispatch only; clones the upstream ITK and runs it with uv",
    ("mutants.yml", "mutants-crate"): "schedule or manual dispatch only: cargo-mutants over a whole crate, in shards",
    ("mutants.yml", "mutants-summary"): "schedule or manual dispatch only: aggregates the crate shards' artifacts",
    ("mutants.yml", "mutants-incremental-shard"): "pull requests labelled `mutation-test`: cargo-mutants --in-diff in shards; its disk step runs sudo (run on types and client by hand: CONTRIBUTING.md, Mutation testing)",
    ("mutants.yml", "mutants-incremental"): "pull requests: aggregates the shards' results",
    ("osv.yml", "rust-pr"): "a reusable workflow of google/osv-scanner-action",
    ("osv.yml", "rust"): "OSV-Scanner action and a SARIF upload",
    ("osv.yml", "peers"): "a reusable workflow of google/osv-scanner-action",
    ("pin-freshness.yml", "cadence"): _REL,
    ("pin-freshness.yml", "freshness"): _REL,
    ("release.yml", "validate"): _REL,
    ("release.yml", "ci"): _REL,
    ("release.yml", "security"): _REL,
    ("release.yml", "ci-evidence"): _REL,
    ("release.yml", "package"): _REL,
    ("release.yml", "conformance"): _REL,
    ("release.yml", "attest-conformance"): _REL,
    ("release.yml", "publish-dry-run"): _REL,
    ("release.yml", "github-release"): _REL,
    ("release.yml", "cli-binaries"): _REL,
    ("release.yml", "deploy-agent-image"): _REL,
    ("release.yml", "publish"): _REL,
    ("scorecard.yml", "analysis"): "the Scorecard action; results go to GitHub",
    ("soak.yml", "soak"): "a PostgreSQL service, and A2A_SOAK_SECS is `steps.duration.outputs.seconds`, a step output the job runner does not evaluate",
    ("tck.yml", "tck-cross-language"): "an include-only matrix of eight peer legs (python, javascript, go, java and four *-sdk legs)",
    ("tck.yml", "tck-example-agents"): "an include-only matrix (three example agents, each with its own setup)",
    ("verification.yml", "kani"): "the Kani GitHub action",
}


def gate_jobs() -> set[str]:
    """ci.yml jobs that preflight.sh --full runs, read from its own definition."""
    text = (ROOT / "scripts/lib/ci_gates.sh").read_text(encoding="utf-8")
    m = re.search(r"^GATE_JOBS='\^\(([^)]*)\)\$'", text, re.M)
    if not m:
        raise SystemExit("workflow_inventory: GATE_JOBS not found in scripts/lib/ci_gates.sh")
    return set(m.group(1).split("|"))


def workflow_events(workflow: str) -> list[str]:
    doc = yaml.safe_load((WORKFLOWS / workflow).read_text(encoding="utf-8"))
    on = doc.get(True, doc.get("on"))  # PyYAML reads the key `on` as True
    return sorted(on) if isinstance(on, dict) else sorted([on] if isinstance(on, str) else on)


def all_jobs() -> list[tuple[str, str]]:
    out = []
    for f in sorted(WORKFLOWS.glob("*.yml")):
        doc = yaml.safe_load(f.read_text(encoding="utf-8"))
        out.extend((f.name, j) for j in doc["jobs"])
    return out


def clean_paths(key: tuple[str, str]) -> list[str]:
    """The registered scratch paths of a job, refusing any that is not an absolute path under /tmp."""
    paths = CLEAN.get(key, [])
    for path in paths:
        if not path.startswith("/tmp/") or ".." in path.split("/") or any(c in path for c in "*?[]"):
            raise SystemExit(f"workflow_inventory: CLEAN path {path!r} must be an absolute path under /tmp, with no glob")
    return paths


def classify() -> tuple[dict[tuple[str, str], dict], list[str]]:
    """Return ({job: {mode, note, args}}, [problems])."""
    jobs = all_jobs()
    existing = set(jobs)
    pre = {("ci.yml", j) for j in gate_jobs()}
    registries = {"preflight": {k: "" for k in pre}, "run": RUN, "local": LOCAL, "ci-only": CI_ONLY}
    problems: list[str] = []
    for key in CLEAN:
        if key not in RUN:
            problems.append(f"CLEAN: {key[0]}:{key[1]} is not a `run` job")
        clean_paths(key)
    out: dict[tuple[str, str], dict] = {}
    for mode, reg in registries.items():
        for key, val in reg.items():
            if key not in existing:
                problems.append(f"{mode}: {key[0]}:{key[1]} is not a job in any workflow")
            elif key in out:
                problems.append(f"{key[0]}:{key[1]} is filed under {out[key]['mode']} and {mode}")
            else:
                note = val if mode in ("local", "ci-only") else ""
                if note.startswith("@events"):
                    note = f"not triggered by pull requests (on: {', '.join(workflow_events(key[0]))})" + note[len("@events"):]
                out[key] = {"mode": mode, "args": val if mode == "run" else [], "note": note}
    for key in jobs:
        if key not in out:
            problems.append(f"{key[0]}:{key[1]} is filed under no heading")
    return {k: out[k] for k in jobs if k in out}, problems


def main(argv: list[str]) -> int:
    table, problems = classify()
    if "--json" in argv:
        print(json.dumps([{"workflow": k[0], "job": k[1], **v} for k, v in table.items()], indent=1))
        return 2 if problems else 0
    if "--table" in argv:
        for (wf, job), v in table.items():
            extra = v["note"] or (" ".join(v["args"]) if v["args"] else "")
            print(f"{v['mode']:<10} {wf}:{job}" + (f"   {extra}" if extra else ""))
    counts: dict[str, int] = {}
    for v in table.values():
        counts[v["mode"]] = counts.get(v["mode"], 0) + 1
    print(f"{len(table)} jobs in {len({k[0] for k in table})} workflows: "
          + ", ".join(f"{n} {m}" for m, n in sorted(counts.items())))
    for p in problems:
        print(f"workflow_inventory: {p}", file=sys.stderr)
    return 2 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
