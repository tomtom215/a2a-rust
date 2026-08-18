#!/usr/bin/env python3
"""Assert scripts/lib/ci_gates.sh reads YAML block scalars the way YAML does.

`gates_for_jobs` has its own block-scalar reader, in awk, because the scripts
that use it must run with nothing installed but coreutils. That reader is a
second implementation of a fiddly corner of YAML, and a second implementation
is a thing that drifts.

It drifted twice while being written. The first version treated `>` exactly
like `|`, so a folded block became one command per line instead of one command
-- YAML folds `run: >` / `echo one` / `two` into `echo one two`, and the parser
would have produced `two` as a command of its own. The second got folding right
except next to a more-indented line, where a preceding blank line contributes
its newline *and* the unfoldable break survives: one blank line before an
indented continuation is two newlines, not one. Neither was visible by reading.

So this compares the awk against PyYAML on the shapes that distinguish them,
rather than against anyone's reading of the spec. Trailing newlines are excluded
from the comparison: the awk deliberately drops them because the result is a
shell command, where chomping cannot change behaviour.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys
import tempfile

import yaml

REPO = pathlib.Path(__file__).resolve().parent.parent

# Each value is what follows `run:` in a step. The set is chosen for the
# distinctions that matter: literal vs folded, blank lines, more-indented
# continuations, both orders of the two together, and explicit indentation
# indicators.
CASES: dict[str, str] = {
    "literal simple": "|\n          echo one\n          echo two",
    "literal continuation": "|\n          echo one \\\n            two",
    "literal blank line": "|\n          echo one\n\n          echo two",
    "literal strip": "|-\n          echo one\n          echo two",
    "literal keep": "|+\n          echo one\n",
    "literal comment hash": "|\n          # a comment\n          echo one",
    "folded two lines": ">\n          echo one\n          two",
    "folded single word": ">\n          echo hello",
    "folded blank line": ">\n          alpha\n\n          beta",
    "folded two blank lines": ">\n          alpha\n\n\n          beta",
    "folded blank with spaces": ">\n          alpha\n   \n          beta",
    "folded more-indented": ">\n          alpha\n            indented\n          beta",
    "folded consecutive more": ">\n          alpha\n            one\n            two\n          beta",
    "folded more plus blanks": ">\n          alpha\n\n            bullet\n\n          beta",
    "folded 2 blanks then more": ">\n          alpha\n\n\n            bullet",
    "folded more then 2 blanks": ">\n          alpha\n            bullet\n\n\n          beta",
    "folded strip": ">-\n          alpha\n          beta",
    "folded keep": ">+\n          alpha\n",
    "folded trailing blanks": ">\n          alpha\n\n",
    "folded with quotes": '>\n          echo "it is"\n          fine',
    "explicit indent folded": ">2\n            alpha\n          beta",
    "explicit indent literal": "|2\n            alpha\n          beta",
}

FIXTURE = "jobs:\n  fmt:\n    runs-on: ubuntu-latest\n    steps:\n      - name: Probe\n        run: {}\n"

EXTRACT = """
set -e
REPO_ROOT="{repo}"; CI_YML="{yml}"
. "$REPO_ROOT/scripts/lib/ci_gates.sh"
gates_for_jobs "^fmt$"
"""


def ours(yml: pathlib.Path) -> str:
    """The command `gates_for_jobs` emits, decoded back to its raw text."""
    out = subprocess.run(
        ["bash", "-c", EXTRACT.format(repo=REPO, yml=yml)],
        capture_output=True, text=True, check=False,
    )
    line = out.stdout.strip()
    if not line:
        raise AssertionError(f"no gate emitted (stderr: {out.stderr.strip()[:200]})")
    cmd = line.split("\t", 1)[1] if "\t" in line else line
    prefix = "bash -e -c "
    if not cmd.startswith(prefix):
        raise AssertionError(f"expected a {prefix.strip()!r} gate, got {cmd!r}")
    # Let bash itself decode the $'...' quoting, rather than reimplementing it
    # here and testing this file against itself.
    return subprocess.run(
        ["bash", "-c", f"printf '%s' {cmd[len(prefix):]}"],
        capture_output=True, text=True, check=True,
    ).stdout


def main() -> int:
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        yml = pathlib.Path(tmp) / "case.yml"
        for name, block in CASES.items():
            yml.write_text(FIXTURE.format(block))
            expected = yaml.safe_load(yml.read_text())["jobs"]["fmt"]["steps"][0]["run"]
            try:
                got = ours(yml)
            except AssertionError as exc:
                failures.append((name, str(exc), ""))
                continue
            if got != expected.rstrip("\n"):
                failures.append((name, repr(expected.rstrip("\n")), repr(got)))

    if failures:
        print("MISMATCH — the awk block-scalar reader disagrees with PyYAML:\n")
        for name, exp, got in failures:
            print(f"  {name}")
            print(f"      pyyaml: {exp}")
            if got:
                print(f"      ours:   {got}")
        print(
            "\nscripts/lib/ci_gates.sh parses ci.yml into the gates preflight and\n"
            "prove_gates_fail run. A block it reads differently from YAML is a gate\n"
            "running a different command than CI does."
        )
        return 1

    print(f"check_block_scalars: {len(CASES)} block-scalar shapes agree with PyYAML")
    return 0


if __name__ == "__main__":
    sys.exit(main())
