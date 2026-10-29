#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Runs every PostgreSQL-gated test suite against a live database.

A test that needs PostgreSQL is `#[ignore]`d unless `A2A_TEST_POSTGRES_URL`
is set, so a suite CI does not name passes green having run nothing. CI named
its suites by hand, and on 2026-10-28 ten test binaries and four library
modules with PostgreSQL cases were in no workflow at all, among them the
regression test for the transient-write retry. This finds them instead: every
test target and library module that reads `A2A_TEST_POSTGRES_URL`, run with
`--ignored`, each required to run at least one test.

Suites a CI step runs by name with flags of their own (a thread limit, a
count it asserts, a feature) are in `NAMED_ELSEWHERE`, which says where;
`--check` (no database needed) fails when an entry no longer appears there,
so the list cannot quietly go stale.

Usage
-----
    python3 scripts/run_postgres_suites.py           # run (needs the URL)
    python3 scripts/run_postgres_suites.py --plan    # print what would run
    python3 scripts/run_postgres_suites.py --check   # NAMED_ELSEWHERE is current

Exit codes: 0 clean, 1 a suite failed or ran no test (or a stale entry), 2
the environment is not set up.
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CRATE = "a2a-protocol-server"
URL_ENV = "A2A_TEST_POSTGRES_URL"

# Suites run by a step of their own, and where: test targets by name, library
# modules by path. Each must still appear in that workflow (`--check`).
NAMED_ELSEWHERE = {
    "test:postgres_store_tests": ".github/workflows/ci.yml",
    "test:multi_replica": ".github/workflows/ci.yml",
    "test:encrypted_store": ".github/workflows/ci.yml",
    "test:turn_claims": ".github/workflows/ci.yml",
    "test:replica_processes": ".github/workflows/ci.yml",
    "test:cross_replica_cancel": ".github/workflows/ci.yml",
    "test:stale_write_position": ".github/workflows/ci.yml",
    "test:soak_multi_replica": ".github/workflows/soak.yml",
    "lib:rate_limit::shared": ".github/workflows/ci.yml",
}

# Suites with PostgreSQL cases that no CI job runs, and why.
NOT_IN_CI = {
    "test:swarm_scale": "load experiments that print tables for a person to "
    "read; each arm takes minutes and none asserts a bound a shared runner "
    "could hold",
}


def crate_dir() -> pathlib.Path:
    return ROOT / "crates" / CRATE


def test_targets() -> list[str]:
    """Test targets (`--test` names) with a file that reads the URL."""
    tests = crate_dir() / "tests"
    found = set()
    for path in tests.rglob("*.rs"):
        if URL_ENV not in path.read_text(encoding="utf-8"):
            continue
        rel = path.relative_to(tests)
        found.add(rel.stem if len(rel.parts) == 1 else rel.parts[0])
    return sorted(found)


def lib_modules() -> list[str]:
    """Library module paths (`a::b::c`) whose file reads the URL."""
    src = crate_dir() / "src"
    found = set()
    for path in src.rglob("*.rs"):
        if URL_ENV not in path.read_text(encoding="utf-8"):
            continue
        parts = list(path.relative_to(src).with_suffix("").parts)
        if parts[-1] == "mod":
            parts.pop()
        found.add("::".join(parts))
    return sorted(found)


def plan() -> tuple[list[str], list[str]]:
    skip = set(NAMED_ELSEWHERE) | set(NOT_IN_CI)
    tests = [t for t in test_targets() if f"test:{t}" not in skip]
    libs = [m for m in lib_modules() if f"lib:{m}" not in skip]
    return tests, libs


def check() -> int:
    bad = []
    for key, workflow in NAMED_ELSEWHERE.items():
        kind, name = key.split(":", 1)
        text = (ROOT / workflow).read_text(encoding="utf-8")
        needle = rf"--test {re.escape(name)}\b" if kind == "test" else re.escape(name)
        if not re.search(needle, text):
            bad.append(f"{key}: listed as run by {workflow}, which no longer names it")
    known = {f"test:{t}" for t in test_targets()} | {f"lib:{m}" for m in lib_modules()}
    for key in list(NAMED_ELSEWHERE) + list(NOT_IN_CI):
        if key not in known:
            bad.append(f"{key}: listed here but reads no {URL_ENV}; remove it")
    if bad:
        print("\n".join(bad))
        return 1
    tests, libs = plan()
    print(f"run_postgres_suites --check: {len(tests)} test target(s) and {len(libs)} "
          f"library module(s) run here, {len(NAMED_ELSEWHERE)} by name elsewhere, "
          f"{len(NOT_IN_CI)} not in CI with a reason")
    return 0


def run_cargo(args: list[str]) -> tuple[int, str]:
    cmd = ["cargo", "test", "-p", CRATE, "--all-features", *args]
    print("$", " ".join(cmd), flush=True)
    # One stream: cargo names each binary on stderr and libtest reports on
    # stdout, and the counts are read by which result follows which name.
    done = subprocess.run(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    out = done.stdout
    print(out[-20000:], flush=True)
    return done.returncode, out


def per_binary_counts(out: str) -> dict[str, int]:
    """Tests run per `--test` target, from cargo's `Running tests/x.rs` lines."""
    counts, current = {}, None
    for line in out.splitlines():
        m = re.match(r"\s*Running tests/([^/\s]+?)(?:\.rs|/main\.rs) \(", line)
        if m:
            current = m.group(1)
            continue
        m = re.match(r"test result: \w+\. (\d+) passed; (\d+) failed", line)
        if m and current:
            counts[current] = counts.get(current, 0) + int(m.group(1)) + int(m.group(2))
            current = None
    return counts


def run() -> int:
    if not os.environ.get(URL_ENV):
        print(f"{URL_ENV} is not set: there is no database to run against")
        return 2
    tests, libs = plan()
    failed = []
    if tests:
        args = [a for t in tests for a in ("--test", t)]
        code, out = run_cargo([*args, "--", "--ignored"])
        if code != 0:
            failed.append("a test target failed (output above)")
        counts = per_binary_counts(out)
        for t in tests:
            n = counts.get(t, 0)
            print(f"  {t}: {n} test(s)")
            if n == 0:
                failed.append(f"test target {t} ran no test: its cases are no longer "
                              "#[ignore]d behind the URL, or the target was renamed")
    if libs:
        code, out = run_cargo(["--lib", "--", "--ignored", *libs])
        if code != 0:
            failed.append("a library module failed (output above)")
        for m in libs:
            n = len(re.findall(rf"^test {re.escape(m)}::\S+ \.\.\. ", out, re.M))
            print(f"  {m}: {n} test(s)")
            if n == 0:
                failed.append(f"library module {m} ran no test")
    if failed:
        print("\n".join(failed))
        return 1
    print(f"run_postgres_suites: {len(tests)} test target(s), {len(libs)} library module(s)")
    return 0


if __name__ == "__main__":
    if "--check" in sys.argv[1:]:
        sys.exit(check())
    if "--plan" in sys.argv[1:]:
        tests, libs = plan()
        print("test targets:", " ".join(tests) or "-")
        print("library modules:", " ".join(libs) or "-")
        sys.exit(0)
    sys.exit(run())
