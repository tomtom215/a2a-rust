#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Hand mutants of the campaign's four fixes: the unmutated baseline first, then each mutant, the file
restored byte for byte (its SHA-256 compared) after every one.

Needs: cargo, the repository checkout this script lives in (clean tree: it edits sources in place and restores
them), $CARGO_TARGET_DIR (a directory of your own). Run from the repository root:

    CARGO_TARGET_DIR=<dir> CARGO_BUILD_JOBS=2 CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \\
      RUSTFLAGS="-D warnings" python3 scripts/live/campaign/live/hand_mutants.py [types|server]

Proves (or fails to): each mutant makes the tests of its fix fail. Exit 1 when a mutant survives or the
baseline fails.
"""
import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[4]
TYPES = "crates/a2a-protocol-types/src/budget/handle.rs"
OPENAI = "crates/a2a-protocol-server/src/tool_loop/openai.rs"
DISCOVER = "crates/a2a-protocol-server/src/skills/discover.rs"

TYPES_CMD = ["cargo", "test", "-p", "a2a-protocol-types", "--lib", "--", "budget::handle"]
SERVER_CMD = ["cargo", "test", "-p", "a2a-protocol-server", "--features", "skills", "--lib", "--",
              "tool_loop::openai::tests", "skills::discover::tests::listing"]

MUTANTS = {
    "types": (TYPES_CMD, [
        ("status: spent is not clamped", TYPES, "m.spent.load(Ordering::Acquire).min(MAX_WIRE_NUMBER)", "m.spent.load(Ordering::Acquire)"),
        ("status: remaining is not clamped", TYPES, "m.remaining().min(MAX_WIRE_NUMBER)", "m.remaining()"),
        ("grant: a named-by-request meter is not clamped", TYPES, "let asked = asked.min(MAX_WIRE_NUMBER);", "let asked = asked;"),
        ("bound is 2^52", TYPES, "const MAX_WIRE_NUMBER: u64 = 1 << 53;", "const MAX_WIRE_NUMBER: u64 = 1 << 52;"),
        ("bound is 2^53 + 1", TYPES, "const MAX_WIRE_NUMBER: u64 = 1 << 53;", "const MAX_WIRE_NUMBER: u64 = (1 << 53) + 1;"),
        ("bound is 2^53 - 1", TYPES, "const MAX_WIRE_NUMBER: u64 = 1 << 53;", "const MAX_WIRE_NUMBER: u64 = (1 << 53) - 1;"),
    ]),
    "server": (SERVER_CMD, [
        ("body: control characters are kept", OPENAI, "if c.is_control() {", "if false {"),
        ("body: an over-limit escape is kept", OPENAI, "shown.truncate(before);", "let _ = before;"),
        ("body: the bound is off by one (>=)", OPENAI, "if shown.len() > limit {", "if shown.len() >= limit {"),
        ("body: the bound is off by one (limit + 1)", OPENAI, "if shown.len() > limit {", "if shown.len() > limit + 1 {"),
        ("listing: a link is not detected", DISCOVER, ".is_ok_and(|m| m.file_type().is_symlink())", ".is_ok_and(|m| m.file_type().is_file())"),
        ("listing: references/assets links are listed", DISCOVER, "            listing.linked.push(dir);\n        } else {", "            let _ = dir;\n        } else {"),
        ("listing: a scripts link is listed", DISCOVER, 'if is_link(root, "scripts") {', "if false {"),
        ("listing: the warning is only Info", DISCOVER, "                Severity::Warning,\n                DiagnosticKind::SymlinkNotFollowed,\n                file,", "                Severity::Info,\n                DiagnosticKind::SymlinkNotFollowed,\n                file,"),
    ]),
}


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(cmd):
    p = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)
    tail = [l for l in (p.stdout + p.stderr).splitlines() if l.startswith("test result") or "error[" in l or "could not compile" in l]
    return p.returncode, tail[-2:]


def main(which):
    bad = 0
    for name, (cmd, mutants) in MUTANTS.items():
        if which and name not in which:
            continue
        code, tail = run(cmd)
        print(f"[{name}] baseline: exit {code} {tail}", flush=True)
        if code != 0:
            return 1
        for label, rel, old, new in mutants:
            path = ROOT / rel
            original = path.read_bytes()
            before = sha(path)
            text = original.decode()
            if text.count(old) != 1:
                print(f"  ?? {label}: the mutated text occurs {text.count(old)} times", flush=True)
                bad += 1
                continue
            path.write_text(text.replace(old, new))
            try:
                code, tail = run(cmd)
            finally:
                path.write_bytes(original)
            restored = sha(path) == before
            verdict = "CAUGHT" if code != 0 else "SURVIVED"
            compiled = not any("could not compile" in t for t in tail)
            print(f"  {verdict:9} {label} (exit {code}, compiled: {compiled}, file restored: {restored}) {tail}", flush=True)
            if code == 0 or not restored or not compiled:
                bad += 1
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(set(sys.argv[1:])))
