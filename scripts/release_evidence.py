#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""A release evidence manifest: what a given commit was shown to be, and by what.

The adversarial review of 2026-11-12 asked for "a release evidence manifest tying the
source tree to tests, manual checks, known waivers, supported profiles, measured limits,
and migration compatibility". Prose says these things and drifts. This writes them as data,
at a named commit, so that a stranger can check each claim against that commit's tree.

    scripts/release_evidence.py generate [--commit REV] [--run-gates] [--out FILE]
    scripts/release_evidence.py verify FILE [--head]
    scripts/release_evidence.py --self-test

`generate` records, for the commit:
  * the commit and tree ids, each published crate's version, the MSRV and the toolchain;
  * the SHA-256 of each input that carries a claim (lockfiles, deny and OSV waivers, the
    CHANGELOG, the readiness bar, the spec ledger, the findings register, the stability
    policy), read from the commit's tree and not the working tree;
  * every advisory waiver, with its reason, parsed from `deny.toml` and the OSV configs;
  * the deployment profiles and the Cargo features of each published crate, parsed from source;
  * the findings register's counts and its open rows;
  * the measured limits, from the machine-readable block in `docs/readiness-bar.md`;
  * the number of unreleased breaking changes the CHANGELOG lists;
  * the exit status of each static gate, when `--run-gates` runs them (otherwise `not_run`:
    a manifest does not claim a check nobody ran).

`verify` re-reads the same inputs at the manifest's commit and fails if any recorded hash,
waiver, profile, feature, count or limit no longer matches, or if the commit is missing.
With `--head` it also fails unless the manifest is for the current HEAD, which is what a
release asks.

What it does not do: it does not run the test suites (their results are recorded by the
CI run of the commit and by `docs/handoff.md`), it does not sign anything, and a passing
`verify` says the manifest still describes that commit, not that the commit is good.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = 1

INPUTS = [
    "Cargo.toml",
    "Cargo.lock",
    "bindings/a2a-protocol-slimrpc/Cargo.lock",
    "deny.toml",
    "osv-scanner.toml",
    "fuzz/osv-scanner.toml",
    "CHANGELOG.md",
    "STABILITY.md",
    "docs/readiness-bar.md",
    "docs/compliance/spec-ledger.json",
    "docs/audits/2026-11-12-adversarial-review/findings.json",
]
CRATES = [
    "crates/a2a-protocol-types",
    "crates/a2a-protocol-client",
    "crates/a2a-protocol-discovery",
    "crates/a2a-protocol-server",
    "crates/a2a-protocol-sdk",
]
REGISTER = "docs/audits/2026-11-12-adversarial-review/findings.json"
READINESS = "docs/readiness-bar.md"
LIMITS_BEGIN = "<!-- measured-limits:begin -->"
LIMITS_END = "<!-- measured-limits:end -->"
GATES = [
    ["bash", "scripts/check_file_lengths.sh"],
    ["python3", "scripts/check_spec_ledger.py"],
    ["python3", "scripts/check_api_reference.py"],
    ["python3", "scripts/check_panic_paths.py"],
    ["python3", "scripts/check_doc_escapes.py"],
]


def git(*args: str, check: bool = True) -> str:
    out = subprocess.run(
        ["git", *args], cwd=ROOT, capture_output=True, text=True, check=False
    )
    if check and out.returncode != 0:
        raise SystemExit(f"git {' '.join(args)}: {out.stderr.strip()}")
    return out.stdout


def at(commit: str, path: str) -> str | None:
    """`path` as it is in `commit`'s tree, or None if it is not there."""
    out = subprocess.run(
        ["git", "show", f"{commit}:{path}"], cwd=ROOT, capture_output=True, check=False
    )
    return out.stdout.decode("utf-8", "replace") if out.returncode == 0 else None


def sha256(text: str) -> str:
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def waivers(deny: str | None, osv: dict[str, str | None]) -> list[dict]:
    """Every advisory waiver in `deny.toml` and the OSV configs, with its reason."""
    found: list[dict] = []
    if deny:
        ignore = tomllib.loads(deny).get("advisories", {}).get("ignore", [])
        for entry in ignore:
            if isinstance(entry, dict):
                found.append(
                    {
                        "source": "deny.toml",
                        "id": entry.get("id", ""),
                        "reason": entry.get("reason", ""),
                    }
                )
            else:
                found.append({"source": "deny.toml", "id": str(entry), "reason": ""})
    for path, text in sorted(osv.items()):
        if text:
            for entry in tomllib.loads(text).get("IgnoredVulns", []):
                found.append(
                    {
                        "source": path,
                        "id": entry.get("id", ""),
                        "reason": entry.get("reason", ""),
                    }
                )
    return found


def profiles(source: str | None) -> list[str]:
    """The variants of `pub enum Profile`."""
    if not source:
        return []
    body = source.split("pub enum Profile {", 1)[-1].split("\n}\n", 1)[0]
    names = re.findall(r"^    ([A-Z][A-Za-z]*)(?:\(|,)", body, flags=re.M)
    return names


def features(manifest: str | None) -> list[str]:
    if not manifest:
        return []
    return sorted(tomllib.loads(manifest).get("features", {}).keys())


def register_summary(text: str | None) -> dict:
    if not text:
        return {"present": False}
    rows = json.loads(text)
    counts: dict[str, int] = {}
    for r in rows:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    return {
        "present": True,
        "rows": len(rows),
        "counts": dict(sorted(counts.items())),
        "open": sorted(r["id"] for r in rows if r["status"] == "OPEN"),
        "partial": sorted(r["id"] for r in rows if r["status"] == "PARTIAL"),
        "maintainer": sorted(r["id"] for r in rows if r["status"] == "MAINTAINER"),
    }


def measured_limits(text: str | None) -> dict:
    """The block of `docs/readiness-bar.md` between the two markers, as JSON."""
    if not text or LIMITS_BEGIN not in text:
        return {"present": False}
    block = text.split(LIMITS_BEGIN, 1)[1].split(LIMITS_END, 1)[0]
    fence = re.search(r"```json\n(.*?)```", block, flags=re.S)
    if not fence:
        raise SystemExit("the measured-limits block has no ```json fence")
    return {"present": True, **json.loads(fence.group(1))}


def breaking_changes(changelog: str | None) -> int:
    """Bullets under `### Breaking Changes` of the `[Unreleased]` section."""
    if not changelog:
        return 0
    unreleased = changelog.split("## [Unreleased]", 1)[-1].split("\n## [", 1)[0]
    if "### Breaking Changes" not in unreleased:
        return 0
    section = unreleased.split("### Breaking Changes", 1)[1].split("\n### ", 1)[0]
    return len(re.findall(r"^- ", section, flags=re.M))


def workspace(manifest: str | None) -> dict:
    data = tomllib.loads(manifest or "")
    package = data.get("workspace", {}).get("package", {})
    return {"msrv": package.get("rust-version")}


def crate_version(manifest: str | None) -> str | None:
    """A crate's own `version`, or `workspace` when it inherits the workspace's."""
    version = tomllib.loads(manifest or "").get("package", {}).get("version")
    if isinstance(version, dict):
        return "workspace"
    return version


def build(commit: str, run_gates: bool) -> dict:
    sha = git("rev-parse", commit).strip()
    tree = git("rev-parse", f"{sha}^{{tree}}").strip()
    texts = {p: at(sha, p) for p in INPUTS}
    osv = {p: texts[p] for p in ("osv-scanner.toml", "fuzz/osv-scanner.toml")}
    gates: list[dict] | str = "not_run"
    if run_gates:
        gates = []
        for cmd in GATES:
            r = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=False)
            gates.append({"command": " ".join(cmd), "exit": r.returncode})
    return {
        "schema": SCHEMA,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "commit": sha,
        "tree": tree,
        "worktree_dirty_when_generated": bool(git("status", "--porcelain").strip()),
        "workspace": workspace(texts["Cargo.toml"]),
        "toolchain": subprocess.run(
            ["rustc", "--version"], capture_output=True, text=True, check=False
        ).stdout.strip(),
        "inputs": {p: sha256(t) if t is not None else None for p, t in texts.items()},
        "waivers": waivers(texts["deny.toml"], osv),
        "profiles": profiles(at(sha, "crates/a2a-protocol-server/src/profile.rs")),
        "versions": {c: crate_version(at(sha, f"{c}/Cargo.toml")) for c in CRATES},
        "features": {c: features(at(sha, f"{c}/Cargo.toml")) for c in CRATES},
        "register": register_summary(texts[REGISTER]),
        "measured_limits": measured_limits(texts[READINESS]),
        "unreleased_breaking_changes": breaking_changes(texts["CHANGELOG.md"]),
        "gates": gates,
    }


def verify(manifest: dict, head: bool) -> list[str]:
    problems: list[str] = []
    if manifest.get("schema") != SCHEMA:
        return [f"schema {manifest.get('schema')!r}, this reads {SCHEMA}"]
    sha = manifest["commit"]
    if git("cat-file", "-t", sha, check=False).strip() != "commit":
        return [f"commit {sha} is not in this repository"]
    if head and git("rev-parse", "HEAD").strip() != sha:
        problems.append(f"the manifest is for {sha[:10]}, HEAD is {git('rev-parse', 'HEAD').strip()[:10]}")
    if git("rev-parse", f"{sha}^{{tree}}").strip() != manifest["tree"]:
        problems.append("the tree id does not match the commit")
    fresh = build(sha, run_gates=False)
    for key in ("workspace", "versions", "inputs", "waivers", "profiles", "features", "register",
                "measured_limits", "unreleased_breaking_changes"):
        if fresh[key] != manifest.get(key):
            problems.append(f"{key}: the manifest and the tree at {sha[:10]} disagree")
    if isinstance(manifest.get("gates"), list):
        for g in manifest["gates"]:
            if g.get("exit") != 0:
                problems.append(f"gate {g.get('command')} exited {g.get('exit')}")
    return problems


def self_test() -> int:
    deny = '[advisories]\nignore = [ { id = "RUSTSEC-1", reason = "r1" }, "RUSTSEC-2" ]\n'
    osv = '[[IgnoredVulns]]\nid = "GHSA-x"\nreason = "why"\n'
    w = waivers(deny, {"osv-scanner.toml": osv, "fuzz/osv-scanner.toml": None})
    assert w == [
        {"source": "deny.toml", "id": "RUSTSEC-1", "reason": "r1"},
        {"source": "deny.toml", "id": "RUSTSEC-2", "reason": ""},
        {"source": "osv-scanner.toml", "id": "GHSA-x", "reason": "why"},
    ], w
    src = "pub enum Profile {\n    /// doc\n    Production,\n    #[cfg(x)]\n    Auditable(Arc<AuditLog>),\n    MultiTenant,\n}\n"
    assert profiles(src) == ["Production", "Auditable", "MultiTenant"], profiles(src)
    assert features('[features]\nb = []\na = ["b"]\n') == ["a", "b"]
    reg = json.dumps([{"id": "A", "status": "OPEN"}, {"id": "B", "status": "FIXED"}, {"id": "C", "status": "OPEN"}])
    s = register_summary(reg)
    assert s["counts"] == {"FIXED": 1, "OPEN": 2} and s["open"] == ["A", "C"], s
    block = f'x\n{LIMITS_BEGIN}\n```json\n{{"commit": "abc", "rows": [1]}}\n```\n{LIMITS_END}\ny'
    assert measured_limits(block) == {"present": True, "commit": "abc", "rows": [1]}
    assert measured_limits("nothing") == {"present": False}
    log = "## [Unreleased]\n\n### Added\n- a\n\n### Breaking Changes\n- one\n- two\n  more\n- three\n\n### Fixed\n- x\n\n## [0.1.0]\n### Breaking Changes\n- old\n"
    assert breaking_changes(log) == 3, breaking_changes(log)
    assert breaking_changes("## [Unreleased]\n### Added\n- a\n") == 0
    assert workspace('[workspace.package]\nrust-version = "1.94"\n') == {"msrv": "1.94"}
    assert crate_version('[package]\nversion = "0.16.0"\n') == "0.16.0"
    assert crate_version('[package]\nversion.workspace = true\n') == "workspace"
    assert crate_version("") is None
    # A recorded hash that no longer matches is a problem, and a matching one is not.
    head = git("rev-parse", "HEAD").strip()
    good = build(head, run_gates=False)
    assert verify(good, head=True) == [], verify(good, head=True)
    bad = json.loads(json.dumps(good))
    bad["inputs"]["Cargo.toml"] = "sha256:" + "0" * 64
    assert any("inputs" in p for p in verify(bad, head=False))
    bad = json.loads(json.dumps(good))
    bad["gates"] = [{"command": "x", "exit": 1}]
    assert any("gate x exited 1" in p for p in verify(bad, head=False))
    bad = json.loads(json.dumps(good))
    bad["commit"] = "0" * 40
    assert any("not in this repository" in p for p in verify(bad, head=False))
    print("release_evidence --self-test: 15 checks passed")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--self-test", action="store_true")
    sub = ap.add_subparsers(dest="cmd")
    g = sub.add_parser("generate")
    g.add_argument("--commit", default="HEAD")
    g.add_argument("--run-gates", action="store_true")
    g.add_argument("--out")
    v = sub.add_parser("verify")
    v.add_argument("file")
    v.add_argument("--head", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    if args.cmd == "generate":
        text = json.dumps(build(args.commit, args.run_gates), indent=1, sort_keys=True) + "\n"
        if args.out:
            Path(args.out).write_text(text)
        else:
            sys.stdout.write(text)
        return 0
    if args.cmd == "verify":
        problems = verify(json.loads(Path(args.file).read_text()), args.head)
        for p in problems:
            print(f"release_evidence: {p}", file=sys.stderr)
        if not problems:
            print("release_evidence: the manifest matches the tree it names")
        return 1 if problems else 0
    ap.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
