#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check that `release.yml` builds under the environment `ci.yml` builds under.

`release.yml`'s `ci` job is a reduced copy of `ci.yml`'s test, clippy, fmt and
doc jobs, and nothing compared the two. It lacked `ci.yml`'s
`CARGO_PROFILE_DEV_DEBUG: 0`, so the first two release runs of v0.16.0 (run
44400079339, attempts 1 and 2) ran out of disk in `CI / test · Linux`, and the
tag had to be made again on a commit with the line (ROADMAP *Release
engineering* item 10; FND-09 step 2).

The build environment is every variable named `RUSTFLAGS`, `RUSTDOCFLAGS` or
`CARGO_*`, in the workflow's `env` and in the `env` of the jobs that build:
`ci.yml`'s `test`, `clippy` and `doc`, and `release.yml`'s `ci`. Each must have
the same value in both files, or be absent from both. Other variables
(`RUST_BACKTRACE`, `CARGO_TERM_COLOR`) change what a run prints, not what it
builds, and are left out on purpose: `RUST_BACKTRACE` is set in `ci.yml` only.

A file whose jobs cannot be found is a failure, not a pass with nothing to
compare.

    python3 scripts/check_release_ci_env.py              # the check
    python3 scripts/check_release_ci_env.py --self-test  # each refusal, shown
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parent.parent
CI = ROOT / ".github/workflows/ci.yml"
RELEASE = ROOT / ".github/workflows/release.yml"
CI_JOBS = ("test", "clippy", "doc")
RELEASE_JOBS = ("ci",)
BUILD_VAR = re.compile(r"^(RUSTFLAGS|RUSTDOCFLAGS|CARGO_\w+)$")
# Print-only variables, named so that a new CARGO_ one is not skipped silently.
PRINT_ONLY = {"CARGO_TERM_COLOR"}


def build_env(workflow: dict, jobs: tuple[str, ...], name: str) -> tuple[dict[str, str], list[str]]:
    """The build variables `jobs` run under, and what could not be read."""
    problems = []
    env: dict[str, str] = {}
    for key, value in (workflow.get("env") or {}).items():
        env[key] = str(value)
    for job in jobs:
        spec = (workflow.get("jobs") or {}).get(job)
        if spec is None:
            problems.append(f"{name}: no job `{job}`")
            continue
        for key, value in (spec.get("env") or {}).items():
            if key in env and env[key] != str(value):
                problems.append(f"{name}: job `{job}` sets {key}={value!r} over the workflow's {env[key]!r}")
            env[key] = str(value)
    return {k: v for k, v in env.items() if BUILD_VAR.match(k) and k not in PRINT_ONLY}, problems


def compare(ci: dict, release: dict) -> list[str]:
    ci_env, problems = build_env(ci, CI_JOBS, "ci.yml")
    rel_env, more = build_env(release, RELEASE_JOBS, "release.yml")
    problems += more
    for key in sorted(set(ci_env) | set(rel_env)):
        a, b = ci_env.get(key), rel_env.get(key)
        if a != b:
            problems.append(
                f"{key}: ci.yml {'sets ' + repr(a) if a is not None else 'does not set it'}, "
                f"release.yml {'sets ' + repr(b) if b is not None else 'does not set it'}"
            )
    return problems


def self_test() -> int:
    base = {
        "env": {"RUSTFLAGS": "-D warnings", "CARGO_PROFILE_DEV_DEBUG": 0, "RUST_BACKTRACE": 1,
                "CARGO_TERM_COLOR": "always"},
        "jobs": {"test": {}, "clippy": {}, "doc": {}},
    }
    rel = {"env": {"RUSTFLAGS": "-D warnings", "CARGO_PROFILE_DEV_DEBUG": 0}, "jobs": {"ci": {}}}
    cases = [
        ("agreeing files", base, rel, None),
        # The v0.16.0 defect itself: the release lacks the debug-info setting.
        ("missing in release", base, {"env": {"RUSTFLAGS": "-D warnings"}, "jobs": {"ci": {}}},
         "CARGO_PROFILE_DEV_DEBUG: ci.yml sets '0', release.yml does not set it"),
        ("different value", base, {"env": {"RUSTFLAGS": "", "CARGO_PROFILE_DEV_DEBUG": 0}, "jobs": {"ci": {}}},
         "RUSTFLAGS: ci.yml sets '-D warnings', release.yml sets ''"),
        ("set in a building job", base,
         {"env": rel["env"], "jobs": {"ci": {"env": {"CARGO_INCREMENTAL": 0}}}},
         "CARGO_INCREMENTAL: ci.yml does not set it, release.yml sets '0'"),
        ("job missing", base, {"env": rel["env"], "jobs": {}}, "release.yml: no job `ci`"),
    ]
    failed = 0
    for label, ci, release, want in cases:
        got = compare(ci, release)
        ok = (not got) if want is None else any(want in g for g in got)
        if not ok:
            failed += 1
            print(f"check_release_ci_env self-test FAIL: {label}: wanted {want or 'no problem'!r}, got {got}")
    print(f"check_release_ci_env self-test: {len(cases) - failed} of {len(cases)} passed")
    return 1 if failed else 0


def main() -> int:
    if "--self-test" in sys.argv[1:]:
        return self_test()
    if self_test():
        return 1
    ci = yaml.safe_load(CI.read_text(encoding="utf-8"))
    release = yaml.safe_load(RELEASE.read_text(encoding="utf-8"))
    problems = compare(ci, release)
    if problems:
        print("check_release_ci_env: release.yml does not build as ci.yml does:", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        return 1
    ci_env, _ = build_env(ci, CI_JOBS, "ci.yml")
    print(f"check_release_ci_env: {len(ci_env)} build variable(s) agree: {', '.join(sorted(ci_env))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
