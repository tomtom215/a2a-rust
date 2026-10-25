#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when a binary of this repository would export no telemetry.

# What problem this solves

The SDK records spans and metrics into whatever OpenTelemetry providers the
process has installed. A binary that installs none exports nothing, whatever
the library does, and nothing in the build says so: it compiles, its tests
pass, and the dashboards simply have no series for it. A wiring audit of the
tree on 2026-10-24 found three such cases by reading, each in a binary that
documentation said was instrumented:

- `examples/live-model` ran the tool loop on every binding with no
  `Telemetry`, the richest `gen_ai.*` source in the repository;
- `bindings/a2a-protocol-slimrpc` built `a2a-protocol-client` with
  `default-features = false`, which drops `tracing` and `otel`, so its client
  half recorded no span and no `rpc.client.call.duration`;
- `itk` did the same for its outbound calls.

# What this checks

1. Every tracked Rust file that defines `fn main` (a `build.rs` excepted)
   starts `Telemetry`: it calls `Telemetry::builder(`, or calls
   `telemetry::init(` with a `telemetry.rs` beside it that does. A `main`
   that deliberately does not is listed in `MAIN_EXEMPT` with the reason.
2. Every Cargo manifest that depends on `a2a-protocol-client`,
   `a2a-protocol-server` or `a2a-protocol-sdk` with `default-features =
   false` lists `otel` among the features it asks for. A manifest that
   deliberately does not is listed in `MANIFEST_EXEMPT` with the reason.
3. No exemption is stale: each names a file that exists and that the check
   would otherwise have failed.

It reads source text, so it proves that a binary asks for telemetry, not that
what it asks for reaches a collector; `deploy/observability/README.md` covers
the live stack, and `crates/a2a-protocol-server/tests/telemetry_export` the
export itself.

# Usage

    check_otel_wiring.py              # check the tree
    check_otel_wiring.py --self-test  # prove each rule on a fixture tree

Exit codes: 0 clean; 1 a binary or manifest is unwired or an exemption is
stale; 2 the check itself could not run.
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# A `fn main` that starts no Telemetry, and why that is right.
MAIN_EXEMPT: dict[str, str] = {
    "tck/src/main.rs": (
        "the official-TCK runner is an independent implementation with no "
        "dependency on the SDK, by design (tck/Cargo.toml); the system under "
        "test, tck/sut, is the instrumented binary"
    ),
    "examples/mcp-agent/src/bin/tool_server.rs": (
        "an MCP server over stdio with no A2A code"
    ),
    "tools/a2a-codegen/src/main.rs": "a code generator with no I/O beyond files",
    "benches/examples/send_probe.rs": (
        "a measurement probe; an exporter would add to what it measures"
    ),
    "benches/sdk-comparison/harness/agent-a2a-rs/src/bin/client.rs": (
        "benchmark harness comparing other SDKs; measures, does not serve"
    ),
    "benches/sdk-comparison/harness/agent-a2a-rs/src/main.rs": (
        "benchmark harness comparing other SDKs; measures, does not serve"
    ),
    "benches/sdk-comparison/harness/agent-a2a-rust/src/bin/client.rs": (
        "benchmark harness pinned to the published SDK, measures"
    ),
    "benches/sdk-comparison/harness/agent-a2a-rust/src/main.rs": (
        "benchmark harness pinned to the published SDK, measures"
    ),
    "benches/sdk-comparison/harness/loadgen/src/main.rs": (
        "a load generator; an exporter would add to what it measures"
    ),
}

# A manifest whose `default-features = false` dependency does not ask for
# `otel`, and why that is right.
MANIFEST_EXEMPT: dict[str, str] = {
    "crates/a2a-protocol-sdk/Cargo.toml": (
        "the facade forwards its own `tracing` and `otel` features to the "
        "crates below it"
    ),
    "fuzz/Cargo.toml": "fuzz targets run no process that exports",
}

SDK_CRATES = ("a2a-protocol-client", "a2a-protocol-server", "a2a-protocol-sdk")
DEP_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def starts_telemetry(path: Path) -> bool:
    text = path.read_text(encoding="utf-8")
    if "Telemetry::builder(" in text:
        return True
    sibling = path.parent / "telemetry.rs"
    return (
        "telemetry::init(" in text
        and sibling.is_file()
        and "Telemetry::builder(" in sibling.read_text(encoding="utf-8")
    )


def tracked(root: Path, pattern: str) -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", pattern],
        check=True,
        capture_output=True,
        text=True,
    )
    return sorted(out.stdout.split())


def unwired_mains(root: Path) -> tuple[list[str], list[str]]:
    """The `main` files that start nothing and are not exempt, and the stale
    exemptions among `MAIN_EXEMPT`."""
    missing, stale = [], []
    mains = []
    for rel in tracked(root, "*.rs"):
        if rel.endswith("build.rs"):
            continue
        if re.search(r"^(pub )?(async )?fn main\b", (root / rel).read_text("utf-8"), re.M):
            mains.append(rel)
    for rel in mains:
        wired = starts_telemetry(root / rel)
        if rel in MAIN_EXEMPT:
            if wired:
                stale.append(f"{rel}: starts Telemetry but is listed in MAIN_EXEMPT")
        elif not wired:
            missing.append(f"{rel}: fn main starts no Telemetry and is not exempt")
    for rel in MAIN_EXEMPT:
        if rel not in mains:
            stale.append(f"{rel}: listed in MAIN_EXEMPT but defines no fn main")
    return missing, stale


def dependency_tables(manifest: dict) -> list[dict]:
    tables = [manifest.get(t, {}) for t in DEP_TABLES]
    tables.append(manifest.get("workspace", {}).get("dependencies", {}))
    for target in manifest.get("target", {}).values():
        tables.extend(target.get(t, {}) for t in DEP_TABLES)
    return tables


def unwired_manifests(root: Path) -> tuple[list[str], list[str]]:
    missing, stale, offending = [], [], set()
    for rel in tracked(root, "*Cargo.toml"):
        manifest = tomllib.loads((root / rel).read_text("utf-8"))
        for table in dependency_tables(manifest):
            for name in SDK_CRATES:
                spec = table.get(name)
                if not isinstance(spec, dict) or spec.get("default-features", True):
                    continue
                if "otel" in spec.get("features", []):
                    continue
                offending.add(rel)
                if rel not in MANIFEST_EXEMPT:
                    missing.append(
                        f"{rel}: {name} without default features asks for no `otel`"
                    )
    for rel in MANIFEST_EXEMPT:
        if rel not in offending:
            stale.append(f"{rel}: listed in MANIFEST_EXEMPT but nothing there needs it")
    return missing, stale


def check(root: Path) -> list[str]:
    problems = []
    for found in (unwired_mains(root), unwired_manifests(root)):
        problems.extend(found[0])
        problems.extend(found[1])
    return problems


def self_test() -> int:
    """Each rule, on a tree where it must fire and the same tree where it must not."""
    global MAIN_EXEMPT, MANIFEST_EXEMPT
    saved = MAIN_EXEMPT, MANIFEST_EXEMPT
    MAIN_EXEMPT, MANIFEST_EXEMPT = {}, {}
    try:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            run = lambda *a: subprocess.run(["git", "-C", tmp, *a], check=True, capture_output=True)
            run("init", "-q")
            (root / "bin").mkdir()
            (root / "bin/main.rs").write_text("fn main() {}\n")
            (root / "Cargo.toml").write_text(
                '[dependencies]\na2a-protocol-client = { path = ".", default-features = false }\n'
            )
            run("add", "-A")
            before = check(root)
            assert len(before) == 2, before
            (root / "bin/main.rs").write_text(
                "fn main() { let _t = Telemetry::builder().build(); }\n"
            )
            (root / "Cargo.toml").write_text(
                '[dependencies]\na2a-protocol-client = { path = ".", default-features = false, features = ["otel"] }\n'
            )
            assert check(root) == [], check(root)
            # A main reaching Telemetry only through a telemetry module whose
            # file does not build one is still unwired.
            (root / "bin/main.rs").write_text("fn main() { let _t = telemetry::init(); }\n")
            (root / "bin/telemetry.rs").write_text("pub fn init() {}\n")
            run("add", "-A")
            assert len(check(root)) == 1, check(root)
            (root / "bin/telemetry.rs").write_text("fn init() { Telemetry::builder(); }\n")
            assert check(root) == [], check(root)
            # An exemption that nothing needs is stale.
            MAIN_EXEMPT = {"bin/main.rs": "x"}
            assert len(check(root)) == 1, check(root)
    finally:
        MAIN_EXEMPT, MANIFEST_EXEMPT = saved
    print("check_otel_wiring: self-test passed")
    return 0


def main() -> int:
    if sys.argv[1:] == ["--self-test"]:
        return self_test()
    if sys.argv[1:]:
        print(__doc__, file=sys.stderr)
        return 2
    problems = check(REPO)
    for p in problems:
        print(f"check_otel_wiring: {p}", file=sys.stderr)
    mains = sum(
        1
        for rel in tracked(REPO, "*.rs")
        if not rel.endswith("build.rs")
        and re.search(r"^(pub )?(async )?fn main\b", (REPO / rel).read_text("utf-8"), re.M)
    )
    print(
        f"check_otel_wiring: {mains} fn main file(s), {len(MAIN_EXEMPT)} exempt; "
        f"{len(MANIFEST_EXEMPT)} manifest exemption(s); {len(problems)} problem(s)"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
