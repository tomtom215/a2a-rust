#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

"""Fails when a fuzz target exists but nothing runs it, or vice versa.

# What problem this solves

A fuzz target reaches CI through three separate files, and nothing tied them
together:

  * `fuzz/fuzz_targets/<name>.rs` — the harness itself;
  * a `[[bin]]` stanza in `fuzz/Cargo.toml` — without which `cargo fuzz`
    cannot build it;
  * an entry in `.github/workflows/fuzz.yml`'s `matrix.target` — without which
    no runner ever invokes it.

Miss the third and the target is written, reviewed, committed and builds
clean, and is executed by nobody. Nothing reports this: the workflow is green
because every target it *does* list passed, and the fuzz suite's own count
goes up by one in every document that quotes it. That is precisely what
happened to `trace_context`, added 2026-09-20 with its `[[bin]]` stanza and no
matrix entry.

The failure mode is quiet in the direction that matters. A target listed in
the matrix but absent from `Cargo.toml` fails loudly on the runner, so the
build catches it. A target absent from the matrix fails nowhere.

# What this checks, and what it does not

Three sets are compared and must be identical: the `.rs` files under
`fuzz/fuzz_targets/`, the `[[bin]]` names in `fuzz/Cargo.toml`, and the
`matrix.target` entries in `.github/workflows/fuzz.yml`. Any name in one set
and not another is a failure naming the file to edit.

It does not check that a target is *effective* — that it reaches interesting
code, that its corpus is seeded, or that it has ever found anything. Coverage
of the fuzzers themselves is not a thing this can answer; that a fuzzer runs
at all is.

Usage:
    python3 scripts/check_fuzz_matrix.py

Exit codes: 0 the three lists agree; 2 drift. There is no exit 1 — every
finding here is a configuration error rather than a judgement about the code.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path


def repo_root() -> Path:
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
        check=True,
    )
    return Path(out.stdout.strip())


def targets_on_disk(root: Path) -> set[str]:
    d = root / "fuzz" / "fuzz_targets"
    return {p.stem for p in d.glob("*.rs")}


def targets_in_cargo_toml(root: Path) -> set[str]:
    """Every `name = "..."` inside a `[[bin]]` stanza."""
    text = (root / "fuzz" / "Cargo.toml").read_text(encoding="utf-8")
    names: set[str] = set()
    in_bin = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_bin = stripped == "[[bin]]"
            continue
        if in_bin:
            m = re.match(r'name\s*=\s*"([^"]+)"', stripped)
            if m:
                names.add(m.group(1))
    return names


def targets_in_workflow(root: Path) -> set[str]:
    """The `matrix.target` list, read as the block of `- name` items after it.

    Parsed rather than loaded as YAML so this has no third-party dependency —
    the same choice every other gate script here makes.
    """
    text = (root / ".github" / "workflows" / "fuzz.yml").read_text(encoding="utf-8")
    lines = text.splitlines()
    names: set[str] = set()
    for i, line in enumerate(lines):
        if re.match(r"^\s*target:\s*$", line):
            indent = len(line) - len(line.lstrip())
            for item in lines[i + 1 :]:
                if not item.strip():
                    continue
                item_indent = len(item) - len(item.lstrip())
                if item_indent <= indent or not item.lstrip().startswith("- "):
                    break
                names.add(item.lstrip()[2:].strip())
            break
    return names


def main() -> int:
    root = repo_root()
    disk = targets_on_disk(root)
    cargo = targets_in_cargo_toml(root)
    workflow = targets_in_workflow(root)

    if not disk or not cargo or not workflow:
        print(
            "check_fuzz_matrix: one of the three lists came back empty "
            f"(disk {len(disk)}, Cargo.toml {len(cargo)}, workflow {len(workflow)}) — "
            "the parser is broken or a file moved",
            file=sys.stderr,
        )
        return 2

    problems: list[str] = []
    for name in sorted(disk - cargo):
        problems.append(
            f"  {name}: fuzz_targets/{name}.rs exists but has no [[bin]] in "
            "fuzz/Cargo.toml, so cargo-fuzz cannot build it"
        )
    for name in sorted(cargo - disk):
        problems.append(
            f"  {name}: fuzz/Cargo.toml declares it but fuzz_targets/{name}.rs "
            "is missing"
        )
    for name in sorted(cargo - workflow):
        problems.append(
            f"  {name}: built by cargo-fuzz but absent from fuzz.yml's "
            "matrix.target, so no runner ever executes it"
        )
    for name in sorted(workflow - cargo):
        problems.append(
            f"  {name}: listed in fuzz.yml's matrix.target but has no [[bin]] "
            "in fuzz/Cargo.toml, so the job will fail to build"
        )

    if problems:
        print("check_fuzz_matrix: the fuzz target lists disagree", file=sys.stderr)
        print("\n".join(problems), file=sys.stderr)
        return 2

    print(f"check_fuzz_matrix: {len(disk)} fuzz targets, each built and each run")
    return 0


if __name__ == "__main__":
    sys.exit(main())
