#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Run one job of a GitHub Actions workflow on this machine, and say what was not run.

`scripts/preflight.sh` reads its gates from `ci.yml` only. The other eighteen
workflows carry jobs that block a merge too (`tck.yml`, `acts.yml`,
`official-tck.yml`, `verification.yml`, `fuzz.yml`, ...), and a local gate list
written from memory missed their steps. This runs a job's own `run:` steps, read
from its workflow file, in order, under the shell the runner would use, with the
environment the workflow gives them.

What it will not do is call a job it could not fully run a pass. Anything it
cannot reproduce is named, and the verdict is then INCOMPLETE:

    a `uses:` step that is not runner set-up (an action it cannot run here)
    a step `if:` or a `${{ }}` expression it cannot evaluate
    a job with `services:`, a `container:`, or a runner that is not Linux

Steps that call `sudo`, `git push` or `docker login` are refused before
anything runs: the job is not started at all (exit 2), because a half-run job
is a result nobody should read.

Verdicts and exit codes:
    PASS        0   every step ran and exited 0 (or failed under continue-on-error)
    FAIL        1   a step without continue-on-error exited non-zero
    refused     2   usage, or a refused step, or a matrix axis with no value given
    INCOMPLETE  3   nothing failed, but something could not be run
    SKIPPED     0   the job's own `if:` is false for --event

Every step's own exit code is printed; the job's verdict is the last line.

Usage:
    scripts/run_workflow_job.py [--matrix k=v] [--expr github.x.y=v] [--event E]
                                [--services-ready] [--log-dir DIR] [--json FILE] WORKFLOW JOB

    --expr supplies the value of a `${{ }}` expression the runner has no answer
    for, e.g. `--expr github.event.pull_request.base.sha=<sha>`; without it a
    step that needs the value is not run and the job is INCOMPLETE.
    scripts/run_workflow_job.py --self-test
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import yaml

# Runner set-up: renders no verdict about this repository, and running it
# locally would change the machine. The repository's own guard
# (scripts/lib/ci_gates.sh, SETUP_ACTIONS) draws the same line.
SETUP_ACTIONS = {
    "actions/checkout",
    "actions/setup-python",
    "actions/setup-go",
    "actions/setup-java",
    "actions/setup-node",
    "dtolnay/rust-toolchain",
    "Swatinem/rust-cache",
}
# Moves files between jobs or to the runner's cache; nothing is judged.
PLUMBING_ACTIONS = {
    "actions/upload-artifact",
    "actions/download-artifact",
    "actions/cache",
}
REFUSED = [
    (re.compile(r"\bsudo\b"), "sudo"),
    (re.compile(r"\bgit\s+push\b"), "git push"),
    (re.compile(r"\bdocker\s+login\b"), "docker login"),
]
EXPR = re.compile(r"\$\{\{\s*(.*?)\s*\}\}")


class Refused(Exception):
    pass


def load(workflow: Path, job_id: str) -> tuple[dict, dict]:
    doc = yaml.safe_load(workflow.read_text(encoding="utf-8"))
    jobs = doc.get("jobs") or {}
    if job_id not in jobs:
        raise Refused(f"{workflow}: no job {job_id!r} (jobs: {', '.join(jobs)})")
    return doc, jobs[job_id]


def matrix_axes(job: dict) -> dict[str, list]:
    m = (job.get("strategy") or {}).get("matrix") or {}
    if not isinstance(m, dict):  # `matrix: ${{ fromJSON(...) }}`
        raise Refused("matrix is an expression; this runner cannot expand it")
    axes = {k: v for k, v in m.items() if k not in ("include", "exclude")}
    if m.get("include") and not axes:
        raise Refused("matrix is `include` only; pass the combination with --matrix k=v")
    return axes


def resolve_matrix(job: dict, given: dict[str, str]) -> dict[str, str]:
    axes = matrix_axes(job)
    out: dict[str, str] = {}
    for k, values in axes.items():
        if k in given:
            out[k] = given[k]
        elif isinstance(values, list) and len(values) == 1:
            out[k] = str(values[0])
        else:
            raise Refused(f"matrix axis {k!r} has values {values!r}; pass --matrix {k}=<value>")
    extra = set(given) - set(axes)
    if extra:
        raise Refused(f"--matrix names no axis of this job: {sorted(extra)}")
    return out


def resolve(expr: str, ctx: dict) -> str | None:
    """The value of one `${{ }}` operand, or None when this runner has no answer."""
    if expr in ctx["expr"]:
        return ctx["expr"][expr]
    if expr.startswith("matrix.") and expr[7:] in ctx["matrix"]:
        return ctx["matrix"][expr[7:]]
    if expr == "github.workspace":
        return str(ctx["root"])
    if expr == "runner.temp":
        return ctx["temp"]
    if expr.startswith("env.") and expr[4:] in ctx["env"]:
        return ctx["env"][expr[4:]]
    return None


def substitute(text: str, ctx: dict) -> tuple[str, list[str]]:
    """Expand the `${{ }}` forms that have an answer here; return the ones that do not.

    `a || b` takes the first operand that has a non-empty answer, as the
    expression language does; if none has, the whole expression is unresolved.
    """
    unresolved: list[str] = []

    def one(m: re.Match[str]) -> str:
        for operand in (o.strip() for o in m.group(1).split("||")):
            value = resolve(operand, ctx)
            if value:
                return value
        unresolved.append(m.group(1))
        return m.group(0)

    return EXPR.sub(one, text), unresolved


STATUS_FN = re.compile(r"\b(always|success|failure|cancelled)\(\)")


def evaluate_if(cond, failed_so_far: bool, event: str, outcomes: dict | None = None):
    """True or False, or None when this runner cannot tell.

    A condition with no status function is `success() && (condition)`, as on the
    hosted runner: it does not run after a failed step.
    """
    if cond is None:
        return not failed_so_far
    c = str(cond).strip()
    if c.startswith("${{") and c.endswith("}}"):
        c = c[3:-2].strip()
    value = terms(c, failed_so_far, event, outcomes or {})
    if value is None:
        return None
    return value if STATUS_FN.search(c) else (value and not failed_so_far)


def terms(cond: str, failed: bool, event: str, outcomes: dict):
    """Terms joined by `&&` and `||` (no parentheses): status functions, the event name, a step's outcome."""
    any_true = False
    for alternative in cond.split("||"):
        all_true = True
        for term in alternative.split("&&"):
            t = term.strip()
            if t in ("always()", "!cancelled()"):
                v = True
            elif t == "success()":
                v = not failed
            elif t == "failure()":
                v = failed
            else:
                m = re.fullmatch(r"(github\.event_name|steps\.[A-Za-z0-9_-]+\.(?:outcome|conclusion))\s*(==|!=)\s*'([^']*)'", t)
                if not m:
                    return None
                if m.group(1) == "github.event_name":
                    actual = event
                else:
                    _, step_id, which = m.group(1).split(".")
                    actual = (outcomes.get(step_id) or {}).get(which)
                    if actual is None:
                        return None
                v = (actual == m.group(3)) == (m.group(2) == "==")
            all_true = all_true and v
        any_true = any_true or all_true
    return any_true


def read_env_file(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            if "=" in line and "<<" not in line.split("=", 1)[0]:
                k, v = line.split("=", 1)
                out[k] = v
    return out


def live_members(group: int) -> list[int]:
    """Processes still running in a process group (a zombie is dead; its parent is gone)."""
    out = []
    for entry in Path("/proc").iterdir():
        if entry.name.isdigit():
            try:
                fields = (entry / "stat").read_text().rsplit(")", 1)[1].split()
            except (OSError, IndexError):
                continue
            if fields[0] != "Z" and int(fields[2]) == group:  # state, ppid, pgrp
                out.append(int(entry.name))
    return out


def step_outcomes(recs: list[dict]) -> dict[str, dict]:
    """`steps.<id>.outcome` and `.conclusion` for the steps that have an id and a known result.

    A set-up or plumbing step the runner skips is taken to have succeeded, as it does on the
    hosted runner. A step it could not run has no outcome, so a condition that reads it is
    not evaluated.
    """
    out: dict[str, dict] = {}
    for r in recs:
        if not r.get("id"):
            continue
        status = r["status"]
        if status == "ok" or (status == "skipped" and r.get("plumbing")):
            out[r["id"]] = {"outcome": "success", "conclusion": "success"}
        elif status == "failed":
            out[r["id"]] = {"outcome": "failure", "conclusion": "failure"}
        elif status == "failed-allowed":
            out[r["id"]] = {"outcome": "failure", "conclusion": "success"}
        elif status == "skipped":
            out[r["id"]] = {"outcome": "skipped", "conclusion": "skipped"}
    return out


def reap(groups: list[int]) -> None:
    """Stop whatever the steps left running, as the hosted runner does at the end of a job."""
    for sig, grace in ((signal.SIGTERM, 3.0), (signal.SIGKILL, 1.0)):
        live = [g for g in groups if live_members(g)]
        if not live:
            return
        for g in live:
            try:
                os.killpg(g, sig)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + grace
        while time.monotonic() < deadline and any(live_members(g) for g in live):
            time.sleep(0.05)


def run_job(workflow: Path, job_id: str, matrix: dict[str, str], event: str,
            services_ready: bool, log_dir: Path, root: Path, exprs: dict[str, str]) -> dict:
    doc, job = load(workflow, job_id)
    steps = job.get("steps") or []
    if not steps:
        raise Refused(f"{job_id} has no steps (a reusable-workflow call); nothing to run")
    mat = resolve_matrix(job, matrix)
    temp = tempfile.mkdtemp(prefix="wfjob-")
    ctx = {"matrix": mat, "root": root, "temp": temp, "env": {}, "expr": exprs}

    # Refuse before anything runs. Checked against the substituted text, so a
    # matrix value cannot smuggle one in.
    for i, s in enumerate(steps, 1):
        cmd, _ = substitute(s.get("run", ""), ctx)
        for rx, what in REFUSED:
            if rx.search(cmd):
                raise Refused(f"step {i} ({s.get('name', '?')}) runs `{what}`; not started")

    result = {"workflow": workflow.name, "job": job_id, "matrix": mat, "steps": [],
              "incomplete": [], "verdict": None}
    runs_on = job.get("runs-on")
    runs_on, _ = substitute(runs_on, ctx) if isinstance(runs_on, str) else (str(runs_on), [])
    if not runs_on.startswith("ubuntu"):
        result["incomplete"].append(f"runs-on {runs_on}: not a Linux runner")
    if job.get("services") and not services_ready:
        result["incomplete"].append(f"services {sorted(job['services'])} not started (--services-ready)")
    if job.get("container"):
        result["incomplete"].append("container: job runs in an image")
    if result["incomplete"]:
        result["verdict"] = "INCOMPLETE"
        return result

    job_if = evaluate_if(job.get("if"), False, event, {}) if job.get("if") is not None else True
    if job_if is None:
        result["incomplete"].append(f"job if: {job['if']!r} cannot be evaluated")
        result["verdict"] = "INCOMPLETE"
        return result
    if job_if is False:
        result["verdict"] = "SKIPPED"
        return result

    env_files = {n: Path(temp) / n for n in ("GITHUB_ENV", "GITHUB_PATH", "GITHUB_OUTPUT", "GITHUB_STEP_SUMMARY")}
    for p in env_files.values():
        p.touch()
    base_env = dict(os.environ)
    base_env.update({"CI": "true", "GITHUB_ACTIONS": "true", "GITHUB_WORKSPACE": str(root),
                     "RUNNER_TEMP": temp, "GITHUB_EVENT_NAME": event,
                     **{k: str(v) for k, v in env_files.items()}})
    base_env["GITHUB_SHA"] = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"],
                                            capture_output=True, text=True).stdout.strip()
    for scope in (doc.get("env"), job.get("env")):
        for k, v in (scope or {}).items():
            val, bad = substitute(str(v), {**ctx, "env": base_env})
            if bad:
                result["incomplete"].append(f"env {k}: cannot evaluate {bad}")
            base_env[k] = val
    if result["incomplete"]:
        # A step cannot be told apart from one that reads the variable, so none starts.
        result["verdict"] = "INCOMPLETE"
        return result
    default_run = (job.get("defaults") or {}).get("run") or (doc.get("defaults") or {}).get("run") or {}

    log_dir.mkdir(parents=True, exist_ok=True)
    failed = False
    groups: list[int] = []
    outcomes: dict[str, dict] = {}
    try:
        for i, step in enumerate(steps, 1):
            name = step.get("name") or step.get("uses") or f"step {i}"
            rec = {"n": i, "name": name, "id": step.get("id"), "status": None, "exit": None, "note": ""}
            result["steps"].append(rec)
            outcomes.update(step_outcomes(result["steps"][:-1]))
            cond = evaluate_if(step.get("if"), failed, event, outcomes)
            if cond is None:
                rec.update(status="not-run", note=f"if: {step['if']!r} cannot be evaluated")
                result["incomplete"].append(f"step {i} {name!r}: {rec['note']}")
                continue
            if not cond:
                rec.update(status="skipped", note="if: false" if step.get("if") else "an earlier step failed")
                continue
            if "uses" in step:
                action = step["uses"].split("@")[0]
                if action in SETUP_ACTIONS or action in PLUMBING_ACTIONS:
                    rec.update(status="skipped", plumbing=True, note=f"{action}: runner set-up or plumbing, no verdict")
                else:
                    rec.update(status="not-run", note=f"{action}: cannot run an action here")
                    result["incomplete"].append(f"step {i} {name!r}: {rec['note']}")
                continue
            step_env = dict(base_env)
            bad: list[str] = []
            for k, v in (step.get("env") or {}).items():
                val, b = substitute(str(v), {**ctx, "env": step_env})
                bad += b
                step_env[k] = val
            cmd, b = substitute(step.get("run", ""), {**ctx, "env": step_env})
            bad += b
            if bad:
                rec.update(status="not-run", note=f"cannot evaluate ${{{{ {bad[0]} }}}}")
                result["incomplete"].append(f"step {i} {name!r}: {rec['note']}")
                continue
            shell = step.get("shell") or default_run.get("shell") or "bash"
            # The runner's default `bash` has no pipefail unless shell: bash is written out.
            flags = ["-e"] if "shell" not in step and not default_run.get("shell") else ["--noprofile", "--norc", "-eo", "pipefail"]
            cwd = root / (step.get("working-directory") or default_run.get("working-directory") or ".")
            log = log_dir / f"{workflow.stem}-{job_id}-{i:02d}.log"
            timeout = float(step.get("timeout-minutes") or job.get("timeout-minutes") or 360) * 60
            print(f"\n▶ [{i}/{len(steps)}] {name}", flush=True)
            t0 = time.monotonic()
            try:
                with log.open("wb") as fh:
                    # Its own session: a server the step starts with `&` stays in this
                    # group, stays up for the later steps, and is stopped with the job.
                    proc = subprocess.Popen([shell, *flags, "-c", cmd], cwd=cwd, env=step_env, stdout=fh,
                                            stderr=subprocess.STDOUT, start_new_session=True)
                    groups.append(proc.pid)
                    try:
                        code = proc.wait(timeout=timeout)
                    except subprocess.TimeoutExpired:
                        os.killpg(proc.pid, signal.SIGKILL)
                        proc.wait()
                        code = 124
            except FileNotFoundError:
                code = 127
            tail = log.read_text(encoding="utf-8", errors="replace").splitlines()[-25:]
            print("\n".join("    " + t for t in tail), flush=True)
            rec.update(exit=code, seconds=round(time.monotonic() - t0, 1), log=str(log))
            for k, v in read_env_file(env_files["GITHUB_ENV"]).items():
                base_env[k] = v
            extra = env_files["GITHUB_PATH"].read_text(encoding="utf-8").split()
            if extra:
                base_env["PATH"] = os.pathsep.join(extra + [base_env["PATH"]])
                env_files["GITHUB_PATH"].write_text("")
            if code == 0:
                rec["status"] = "ok"
            elif step.get("continue-on-error"):
                rec.update(status="failed-allowed", note="continue-on-error: the job does not fail")
            else:
                rec["status"] = "failed"
                failed = True
    finally:
        reap(groups)
    if failed:
        result["verdict"] = "FAIL"
    elif result["incomplete"]:
        result["verdict"] = "INCOMPLETE"
    else:
        result["verdict"] = "PASS"
    return result


EXIT = {"PASS": 0, "SKIPPED": 0, "FAIL": 1, "INCOMPLETE": 3}


def report(r: dict) -> str:
    lines = [f"\n== {r['workflow']}:{r['job']}" + (f" {r['matrix']}" if r["matrix"] else "")]
    for s in r["steps"]:
        code = "-" if s["exit"] is None else str(s["exit"])
        lines.append(f"  [{s['n']:>2}] {s['status']:<14} exit {code:>3}  {s['name']}"
                     + (f"  ({s['note']})" if s["note"] else ""))
    for why in r["incomplete"]:
        lines.append(f"  not run: {why}")
    ok = sum(1 for s in r["steps"] if s["status"] == "ok")
    bad = sum(1 for s in r["steps"] if s["status"] == "failed")
    allowed = sum(1 for s in r["steps"] if s["status"] == "failed-allowed")
    lines.append(f"RESULT {r['workflow']}:{r['job']} {r['verdict']}  steps ok={ok} failed={bad} "
                 f"failed-under-continue-on-error={allowed} not-run={len(r['incomplete'])}")
    return "\n".join(lines)


def parse_args(argv: list[str]) -> dict:
    a = {"matrix": {}, "expr": {}, "event": "pull_request", "services": False, "log": None,
         "json": None, "pos": []}
    it = iter(argv)
    for x in it:
        if x == "--matrix":
            k, _, v = next(it).partition("=")
            a["matrix"][k] = v
        elif x == "--expr":
            k, _, v = next(it).partition("=")
            a["expr"][k] = v
        elif x == "--event":
            a["event"] = next(it)
        elif x == "--services-ready":
            a["services"] = True
        elif x == "--log-dir":
            a["log"] = Path(next(it))
        elif x == "--json":
            a["json"] = Path(next(it))
        elif x.startswith("-"):
            raise Refused(f"unknown option {x}")
        else:
            a["pos"].append(x)
    if len(a["pos"]) != 2:
        raise Refused("usage: run_workflow_job.py [options] WORKFLOW JOB")
    return a


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        from importlib import import_module  # noqa: F401  (self-test lives beside this file)
        sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
        return import_module("run_workflow_job_selftest").main(Path(__file__).resolve())
    try:
        a = parse_args(argv)
        root = Path(subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True,
                                   text=True, check=True).stdout.strip())
        wf = Path(a["pos"][0])
        if not wf.exists():
            wf = root / ".github/workflows" / a["pos"][0]
        log_dir = a["log"] or Path(tempfile.mkdtemp(prefix="wfjob-logs-"))
        r = run_job(wf, a["pos"][1], a["matrix"], a["event"], a["services"], log_dir, root, a["expr"])
    except Refused as e:
        print(f"run_workflow_job: refused: {e}", file=sys.stderr)
        return 2
    print(report(r))
    if a["json"]:
        a["json"].write_text(json.dumps(r, indent=2), encoding="utf-8")
    return EXIT[r["verdict"]]


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
