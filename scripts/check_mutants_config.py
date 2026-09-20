#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Assert `mutants.toml`'s inert/effective status matches what it claims.

`mutants.toml` sits at the repository root. cargo-mutants does not look there —
it reads `.cargo/mutants.toml` — so every exclusion, timeout and test-arg in
the file governs nothing, and the sweep is configured entirely by the command
line in `.github/workflows/mutants.yml`. The file says so itself, at length, in
a banner reading "NOTHING IN THIS FILE IS IN EFFECT".

That banner is the problem this checker exists for. It is a measurement from
2026-08-14 written down as prose, and prose does not re-measure itself. The
moment somebody creates `.cargo/`, or copies the file into it, the banner
becomes a confident, detailed, wrong description of what the mutation gate
measures — and the mutation gate is the one that prints a percentage this
repository publishes. A stale assumption should fail a gate, not sit in a
comment.

The hazard, which is why "just move it" is not the fix
------------------------------------------------------
cargo-mutants 27.1.0 rejects unknown config fields outright rather than warning.
`mutants.toml` carries two keys it does not know — `cap_timeout` and `jobs` —
both measured, both recorded in the file:

    Error: parse toml from mutants.toml
    Caused by: TOML parse error at line 1, column 1
               unknown field `cap_timeout`

So moving the file into `.cargo/` as it stands does not narrow the mutation
gate, it stops the tool: every shard aborts before listing a single mutant. A
shard that cannot start is the one failure mode a mutation gate must not have.
This checker treats that as the highest-severity finding it can report.

What is asserted
----------------
* **While the root file is inert** (`.cargo/mutants.toml` absent) it must say
  so, and it must still carry the keys its own prose names as the reason the
  move is not a one-key edit. A banner claiming a hazard that is no longer
  there is as stale as one denying a hazard that is.

* **Once it is effective** (`.cargo/mutants.toml` present) that file must carry
  none of the rejected keys, and the root file must no longer claim to be
  inert.

* `.github/workflows/mutants.yml` must not point `--config` at a file carrying
  a rejected key, which would make the sweep read it and abort.

What is not asserted
--------------------
That cargo-mutants actually accepts the effective file. That needs the tool,
and it is not installed on the machines this gate runs on (`which cargo-mutants`
finds nothing here). The rejected-key list below is therefore a recorded
measurement, not a live one, and it is the *narrower* claim: a key not on the
list could still be rejected by a future version. Whoever moves this file must
run `cargo mutants --config <path> --list` and confirm it lists mutants before
trusting a green run — the file's own banner says the same thing, and this
gate cannot do it for them.

Exit codes: 0 the claim matches reality; 1 it does not; 2 the files cannot be
read or parsed.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

ROOT_CONFIG = ROOT / "mutants.toml"
# Where cargo-mutants actually looks. Measured 2026-08-14 with cargo-mutants
# 27.1.0 and recorded in mutants.toml's own banner: `--no-config` changes
# nothing about a root-level file, which is the tell that it was never read.
EFFECTIVE_CONFIG = ROOT / ".cargo" / "mutants.toml"
MUTANTS_YML = ROOT / ".github" / "workflows" / "mutants.yml"

# Keys cargo-mutants 27.1.0 has no field for and rejects outright. Measured
# 2026-08-19 by writing each key of mutants.toml into `.cargo/mutants.toml` on
# its own and running `cargo mutants -p a2a-protocol-types --list`; these two
# are the ones that produce `unknown field`.
REJECTED_KEYS = ("cap_timeout", "jobs")

INERT_BANNER = "NOTHING IN THIS FILE IS IN EFFECT"

CONFIG_FLAG = re.compile(r"--config[= \t]+(\S+)")


def keys_of(path: Path) -> set[str]:
    """Top-level keys of a TOML file. Exits 2 rather than guessing."""
    try:
        return set(tomllib.loads(path.read_text(encoding="utf-8")))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        print(f"check_mutants_config: cannot parse {path}: {exc}", file=sys.stderr)
        sys.exit(2)


def main() -> int:
    if not ROOT_CONFIG.exists() and not EFFECTIVE_CONFIG.exists():
        print(
            "check_mutants_config: neither mutants.toml nor .cargo/mutants.toml "
            "exists. If the configuration was deleted on purpose, delete this "
            "gate and its ci.yml step with it.",
            file=sys.stderr,
        )
        return 2

    findings: list[str] = []
    effective = EFFECTIVE_CONFIG.exists()

    if effective:
        present = sorted(k for k in REJECTED_KEYS if k in keys_of(EFFECTIVE_CONFIG))
        if present:
            findings.append(
                ".cargo/mutants.toml is in effect and carries key(s) cargo-mutants "
                "rejects: " + ", ".join(f"`{k}`" for k in present) + ".\n"
                "      cargo-mutants 27.1.0 rejects unknown fields outright, so "
                "every mutation shard\n"
                "      aborts before listing a mutant. Delete these keys — `jobs` "
                "and the timeout cap\n"
                "      are command-line flags, and mutants.yml already passes "
                "`--jobs` — then confirm\n"
                "      with `cargo mutants --config .cargo/mutants.toml --list` "
                "that it lists mutants."
            )
        if ROOT_CONFIG.exists() and INERT_BANNER in ROOT_CONFIG.read_text(
            encoding="utf-8"
        ):
            findings.append(
                "mutants.toml still carries its "
                f'"{INERT_BANNER}" banner, but .cargo/mutants.toml now exists '
                "and\n"
                "      cargo-mutants reads it. Every measurement the banner "
                "records — 731 mutants\n"
                "      listed, 155 of them generated proto code, a 328s "
                "auto-timeout — describes a\n"
                "      sweep that is no longer the one running. Re-measure and "
                "rewrite it, or delete\n"
                "      the root file now that it has a live replacement."
            )
    else:
        text = ROOT_CONFIG.read_text(encoding="utf-8")
        if INERT_BANNER not in text:
            findings.append(
                f'mutants.toml no longer carries its "{INERT_BANNER}" banner, '
                "and .cargo/mutants.toml\n"
                "      does not exist — so the file is still inert and no "
                "longer says so. Every value\n"
                "      in it reads as live configuration and none of it is. "
                "Restore the banner, or\n"
                "      move the file to .cargo/ (see the rejected keys below "
                "before you do)."
            )
        absent = sorted(k for k in REJECTED_KEYS if k not in keys_of(ROOT_CONFIG))
        if absent:
            findings.append(
                "mutants.toml no longer carries key(s) its own prose names as "
                "the reason moving\n"
                "      it is not a one-key edit: "
                + ", ".join(f"`{k}`" for k in absent)
                + ".\n"
                "      That is progress, not a defect in itself — but the banner "
                "and the per-key\n"
                "      notes still describe them, and the next person to read "
                "them will plan a move\n"
                "      around a hazard that is already gone. Update the prose, "
                "and update\n"
                "      REJECTED_KEYS in this script, together."
            )

    # A `--config` pointing at a file with a rejected key makes the sweep read
    # it and abort. mutants.yml passes no `--config` today; this is what stops
    # one being added without the keys being cleaned up first.
    if MUTANTS_YML.exists():
        yml = MUTANTS_YML.read_text(encoding="utf-8")
        for m in CONFIG_FLAG.finditer(yml):
            target = ROOT / m.group(1).strip("\"'")
            if not target.exists():
                continue
            present = sorted(k for k in REJECTED_KEYS if k in keys_of(target))
            if present:
                findings.append(
                    f"mutants.yml passes `--config {m.group(1)}`, and that file "
                    "carries key(s)\n"
                    "      cargo-mutants rejects: "
                    + ", ".join(f"`{k}`" for k in present)
                    + ". The sweep will abort on\n"
                    "      the parse error rather than run."
                )

    status = "effective (.cargo/mutants.toml)" if effective else "inert (root only)"
    if findings:
        print(f"MUTANTS CONFIG DRIFT — the file is {status}, and says otherwise\n")
        for f in findings:
            print(f"  * {f}")
        print(
            "\nmutants.toml describes what the mutation gate measures, and the "
            "mutation gate\nprints a percentage this repository publishes. A "
            "description that has stopped\nbeing true is worse than none: it is "
            "read and believed."
        )
        return 1

    print(
        f"check_mutants_config: mutants.toml is {status}, consistent with what "
        f"it claims; {len(REJECTED_KEYS)} recorded rejected key(s) accounted for"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
