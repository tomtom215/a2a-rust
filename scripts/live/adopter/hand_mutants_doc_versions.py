#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Hand mutants of the new assertions in scripts/check_doc_versions.py.

Each mutant is one edit to a copy of the file; `--self-test` of the copy must then fail,
after the unmutated copy has passed it (a self-test that fails on every run kills every
mutant: docs/handoff.md, "A hand mutant is a kill only if the test passes without it").
Prints one line per mutant and a count. Exit 1 when one survives.

Usage: hand_mutants_doc_versions.py
"""
import subprocess
import sys
import tempfile
from pathlib import Path

SRC = Path(__file__).resolve().parents[2] / "check_doc_versions.py"
MUTANTS = [
    ("badge/prose compared with == instead of !=", "if badge[0] != prose[0]:", "if badge[0] == prose[0]:"),
    ("badge/prose: 'one of each' becomes 'one of either'", "if len(badge) != 1 or len(prose) != 1:", "if len(badge) != 1 and len(prose) != 1:"),
    ("a table row no longer starts a block", 'line.startswith("|") or ', ""),
    ("a list item no longer starts a block", 'or re.match(r"\\s*(?:[-*]|\\d+\\.)\\s", line)', ""),
    ("'deprecated' excuses nothing", '"deprecated" not in joined.lower()', '"deprecated" in joined.lower()'),
    ("history is read (deprecated-entry check)", "or path.startswith(HISTORICAL)", "", 0),
    ("history is read (dependency-block check)", "or path.startswith(HISTORICAL)", "", 1),
    ("non-markdown is read (deprecated-entry check)", 'not path.endswith(".md") or ', "", 0),
    ("non-markdown is read (dependency-block check)", 'not path.endswith(".md") or ', "", 1),
    ("a declared crate is a finding", "crate not in declared", "crate in declared"),
    ("a fragment (no fn main) is held to the block", 'if not re.search(r"\\bfn main\\b", body):', "if False:"),
    ("an ignored block is read", 'or any(x.startswith("ignore") for x in tags)', ""),
    ("comments are read", 'code = re.sub(r"//[^\\n]*", "", body)', "code = body"),
    ("a page with no dependency block is held to one", "if not shown:\n            continue", "if False:\n            continue"),
]


def run(path):
    return subprocess.run([sys.executable, "-I", str(path), "--self-test"], capture_output=True, text=True).returncode


def main():
    text = SRC.read_text()
    with tempfile.TemporaryDirectory() as d:
        base = Path(d) / "base.py"
        base.write_text(text)
        assert run(base) == 0, "the unmutated copy must pass its own self-test"
        survived = 0
        for name, old, new, *nth in MUTANTS:
            n = nth[0] if nth else 0
            assert text.count(old) > n and (nth or text.count(old) == 1), f"mutant anchor absent or ambiguous: {name!r} ({text.count(old)})"
            parts = text.split(old)
            mutated = old.join(parts[: n + 1]) + new + old.join(parts[n + 1 :])
            m = Path(d) / "mutant.py"
            m.write_text(mutated)
            rc = run(m)
            print(("KILLED  " if rc != 0 else "SURVIVED") + " " + name)
            survived += rc == 0
    print(f"{len(MUTANTS) - survived} of {len(MUTANTS)} mutants killed")
    return 1 if survived else 0


if __name__ == "__main__":
    sys.exit(main())
