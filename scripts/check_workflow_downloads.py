#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Refuse a workflow step that fetches something from the network the fragile way.

On 2026-11-09 two workflows on `main` died on a download, not on the code
(an independent review read the logs): `ci.yml`'s static-checks job on
`curl -sSL <mdBook> | tar -xz` ("not in gzip format", so every gate after it
never ran), and `observability.yml` on an HTTP 504 fetching promtool. Neither
says anything about the Rust. Both are a download that was not retried and not
checked, so this gate states the rules once and applies them to every `run:` of
every workflow:

  D1  `curl` or `wget` may name only a loopback URL (a readiness probe of a
      server the job started). Anything else goes through
      `scripts/ci/fetch_verified.sh URL SHA256 DEST` (HTTPS, `--fail`, retried,
      pinned digest, nothing left at DEST unless the digest matches) or a
      script that calls it (`scripts/ci/install_mdbook.sh`,
      `deploy/observability/scripts/install_tools.sh`).
  D2  A step that runs `git clone`, or adds an HTTPS remote, runs it under
      `scripts/ci/retry.sh`: a clone that fails on a 5xx or a reset is run
      again instead of failing the job.
  D3  `cargo install` carries `--locked` (the dependency versions its authors
      tested) and `--version`, and a workflow that runs one sets
      `CARGO_NET_RETRY` of at least 10 in its top-level `env` (cargo's own
      default is 3 retries on a registry that answers 5xx).

What this does not check: that a digest is right (the helper does, at run
time), that an action is pinned (`pin-freshness.yml` and the scorecard do), or
`pip`/`npm` installs (`--require-hashes` is checked where it is used).

Usage:
    scripts/check_workflow_downloads.py              # the repository's workflows
    scripts/check_workflow_downloads.py --self-test  # the rules on fixtures

Exit codes: 0 no violation (or every self-test case behaves); 1 a violation (or
a self-test case that does not); 2 a workflow could not be read.
"""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover - environment problem, not logic
    sys.exit("error: PyYAML is required (ci.yml installs it from .github/requirements/pyyaml.txt)")

ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS = ROOT / ".github" / "workflows"

LOOPBACK = re.compile(r"https?://(?:127\.0\.0\.1|localhost|\[::1\]|0\.0\.0\.0)(?:[:/\s\"']|$)")
URL = re.compile(r"https?://[^\s\"')]+")
# A curl or wget in command position: start of a logical line, after a command
# separator, `then`/`do`/`else`, `$(`, a `!`, `sudo`, or leading VAR=value words.
FETCHER = re.compile(
    r"(?:^|[;&|(]|\bthen\b|\bdo\b|\belse\b|\$\()\s*!?\s*(?:sudo\s+)?(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*(curl|wget)\b"
)
HELPERS = ("scripts/ci/fetch_verified.sh", "scripts/ci/install_mdbook.sh", "install_tools.sh")
CLONE = re.compile(r"\bgit\s+(?:-C\s+\S+\s+)?clone\b|\bgit\s+(?:-C\s+\S+\s+)?remote\s+add\s+\S+\s+https?://")
CARGO_INSTALL = re.compile(r"\bcargo\s+install\b")


@dataclass(frozen=True)
class Violation:
    where: str
    rule: str
    text: str

    def __str__(self) -> str:
        return f"{self.where}: {self.rule}: {self.text}"


def logical_lines(body: str) -> list[str]:
    """The body's lines with backslash continuations joined and comments dropped."""
    joined = re.sub(r"\\\n\s*", " ", body)
    lines = []
    for raw in joined.splitlines():
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        lines.append(stripped)
    return lines


def check_step(where: str, body: str) -> list[Violation]:
    found: list[Violation] = []
    lines = logical_lines(body)
    for line in lines:
        if FETCHER.search(line) and not LOOPBACK.search(line):
            found.append(
                Violation(
                    where,
                    "D1 download without the verified helper",
                    f"{line[:140]} (use scripts/ci/fetch_verified.sh URL SHA256 DEST)",
                )
            )
        if CARGO_INSTALL.search(line):
            if "--locked" not in line:
                found.append(Violation(where, "D3 cargo install without --locked", line[:140]))
            if "--version" not in line:
                found.append(Violation(where, "D3 cargo install without --version", line[:140]))
    if any(CLONE.search(line) for line in lines) and "scripts/ci/retry.sh" not in body:
        offender = next(line for line in lines if CLONE.search(line))
        found.append(
            Violation(where, "D2 clone without scripts/ci/retry.sh", f"{offender[:140]}")
        )
    return found


def check_env(where: str, env: object, installs: bool) -> list[Violation]:
    if not installs:
        return []
    raw = env.get("CARGO_NET_RETRY") if isinstance(env, dict) else None
    try:
        ok = raw is not None and int(str(raw)) >= 10
    except ValueError:
        ok = False
    if ok:
        return []
    return [
        Violation(
            where,
            "D3 cargo install without CARGO_NET_RETRY >= 10 in the workflow env",
            f"CARGO_NET_RETRY is {raw!r}",
        )
    ]


def check_workflow(name: str, text: str) -> list[Violation]:
    doc = yaml.safe_load(text) or {}
    found: list[Violation] = []
    installs = False
    for job_id, job in (doc.get("jobs") or {}).items():
        for index, step in enumerate(job.get("steps") or []):
            body = step.get("run")
            if not isinstance(body, str):
                continue
            label = step.get("name") or f"step {index + 1}"
            where = f"{name} job `{job_id}` step `{label}`"
            found += check_step(where, body)
            installs = installs or any(CARGO_INSTALL.search(line) for line in logical_lines(body))
    found += check_env(f"{name} top-level env", doc.get("env"), installs)
    return found


def check_tree(root: Path) -> list[Violation]:
    found: list[Violation] = []
    files = sorted((root / ".github" / "workflows").glob("*.yml"))
    if not files:
        raise SystemExit(f"error: no workflows under {root / '.github' / 'workflows'}")
    for path in files:
        found += check_workflow(path.name, path.read_text(encoding="utf-8"))
    return found


# ── self-test ────────────────────────────────────────────────────────────────

SELF_TEST: list[tuple[str, str, list[str]]] = [
    # (case, step body, rules that must be reported, by prefix)
    (
        "the line that failed on 2026-11-09",
        "curl -sSL https://github.com/rust-lang/mdBook/releases/download/v0.4.40/x.tar.gz \\\n  | tar -xz -C \"$HOME/.local/bin\"",
        ["D1"],
    ),
    ("wget is a download too", "wget -q https://example.com/tool.tgz", ["D1"]),
    ("a curl after && is in command position", "mkdir -p x && curl -fsSL https://example.com/a -o x/a", ["D1"]),
    ("a curl whose URL is a variable", 'curl -fsSL "$URL" -o out', ["D1"]),
    ("the verified helper is fine", "scripts/ci/fetch_verified.sh https://example.com/a 0000 /tmp/a", []),
    ("a loopback probe is fine", "curl -sf http://127.0.0.1:9999/.well-known/agent-card.json", []),
    ("a localhost probe is fine", "curl -fsS http://localhost:8080/ready", []),
    ("the word curl in a comment or echo is not a download", "# curl is used below\necho \"curl the card\"", []),
    ("a clone without the retry helper", "git clone --depth 1 https://github.com/a/b /tmp/b", ["D2"]),
    ("a clone under the retry helper", "scripts/ci/retry.sh 5 -- git clone --depth 1 https://github.com/a/b /tmp/b", []),
    (
        "an https remote without the retry helper",
        "git init /tmp/x\ngit -C /tmp/x remote add origin https://github.com/a/b\ngit -C /tmp/x fetch origin main",
        ["D2"],
    ),
    ("cargo install without --locked", "cargo install cargo-hack --version 0.6.30", ["D3"]),
    ("cargo install without --version", "cargo install cargo-hack --locked", ["D3"]),
    ("cargo install pinned and locked", "cargo install cargo-hack --locked --version 0.6.30", []),
]


def self_test() -> int:
    failures = 0
    for case, body, want in SELF_TEST:
        got = [v.rule.split()[0] for v in check_step("fixture", body)]
        ok = sorted(set(got)) == sorted(set(want)) and (not want or all(w in got for w in want))
        print(f"  {'ok  ' if ok else 'FAIL'} {case}: reported {sorted(set(got)) or 'nothing'}")
        failures += 0 if ok else 1

    env_cases = [
        ("env without CARGO_NET_RETRY when a step installs", {"CARGO_TERM_COLOR": "always"}, True, 1),
        ("env with CARGO_NET_RETRY 10", {"CARGO_NET_RETRY": 10}, True, 0),
        ("env with CARGO_NET_RETRY 3 is too low", {"CARGO_NET_RETRY": 3}, True, 1),
        ("no install, no env needed", {}, False, 0),
    ]
    for case, env, installs, want in env_cases:
        got = len(check_env("fixture", env, installs))
        ok = got == want
        print(f"  {'ok  ' if ok else 'FAIL'} {case}: {got} violation(s)")
        failures += 0 if ok else 1

    wf = "jobs:\n  a:\n    steps:\n      - name: dl\n        run: |\n          curl -sSL https://e.com/x | tar -xz\n"
    got = check_workflow("w.yml", wf)
    ok = len(got) == 1 and "job `a` step `dl`" in str(got[0])
    print(f"  {'ok  ' if ok else 'FAIL'} a violation names its workflow, job and step: {[str(v)[:60] for v in got]}")
    failures += 0 if ok else 1

    print("check_workflow_downloads self-test:", "every case behaves" if not failures else f"{failures} case(s) FAIL")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    if argv == ["--self-test"]:
        return self_test()
    if argv:
        print(__doc__)
        return 2
    try:
        violations = check_tree(ROOT)
    except (OSError, yaml.YAMLError) as err:
        print(f"check_workflow_downloads: cannot read a workflow: {err}", file=sys.stderr)
        return 2
    for v in violations:
        print(f"check_workflow_downloads: {v}")
    if violations:
        print(f"check_workflow_downloads: {len(violations)} violation(s)")
        return 1
    count = len(list((ROOT / ".github" / "workflows").glob("*.yml")))
    print(f"check_workflow_downloads: ok ({count} workflows)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
