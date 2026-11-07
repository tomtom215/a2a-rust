#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Check, without running them, the commands the README and the book tell a reader to type.

For every `bash`/`sh`/`console` block of the README, the book and the example
READMEs this reads each line (backslash continuations joined) and checks that

  * `cargo ... -p NAME` names a package of the workspace;
  * `--bin NAME` / `--example NAME` / `--test NAME` names a target of that package;
  * `--features a,b` / `-p x --features x/f` names features that exist;
  * `--manifest-path P`, `-f FILE` (docker, compose, kubectl) and a leading
    `./scripts/...`, `scripts/...`, `deploy/...`, `examples/...`, `tools/...`
    path name a file or directory that exists.

It does not run anything: a command that parses and names things that exist can
still fail, and `02-book-getting-started.sh`, `03-examples-offline.sh` and
`04-cli.sh` are where commands are run. Prints one line per finding and a
summary; exit 1 when there is a finding.

Usage: 02-commands-static.py        (from anywhere in the checkout; runs `cargo metadata --no-deps`)
"""
import json
import re
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(Path(__file__).parent))
import book_blocks  # noqa: E402

meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"],
                                 cwd=ROOT, capture_output=True, text=True, check=True).stdout)
PKGS = {p["name"]: p for p in meta["packages"]}
# Crates outside the workspace a page may name with `-p` after `cd`: skip checking those.
OUTSIDE = {"a2a-codegen", "a2a-protocol-slimrpc", "a2a-live-replica"}
PATHISH = re.compile(r"^(?:\./)?(scripts|deploy|examples|tools|docs|book|crates|bindings|itk|tck|fuzz|benches|packaging)/\S*$")

findings, checked = [], 0


def targets(pkg, kind):
    return {t["name"] for t in pkg["targets"] if kind in t["kind"]}


def check_line(where, line):
    global checked
    try:
        words = shlex.split(line, comments=True)
    except ValueError:
        return
    if not words:
        return
    # drop leading VAR=value and `env`
    while words and (re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", words[0]) or words[0] == "env"):
        words = words[1:]
    if not words:
        return
    if words[0] == "cargo":
        checked += 1
        pkg = None
        i = 1
        while i < len(words):
            w = words[i]
            nxt = words[i + 1] if i + 1 < len(words) else ""
            if w in ("-p", "--package"):
                if nxt in OUTSIDE:
                    pkg = None
                elif nxt not in PKGS:
                    findings.append(f"{where}: `-p {nxt}` is not a workspace package")
                else:
                    pkg = PKGS[nxt]
                i += 1
            elif w in ("--bin", "--example", "--test", "--bench"):
                kind = {"--bin": "bin", "--example": "example", "--test": "test", "--bench": "bench"}[w]
                if pkg is not None and nxt not in targets(pkg, kind):
                    findings.append(f"{where}: package {pkg['name']} has no {w[2:]} `{nxt}` (has {sorted(targets(pkg, kind))})")
                i += 1
            elif w == "--features" or w.startswith("--features="):
                val = w.split("=", 1)[1] if "=" in w else nxt
                for f in val.replace(" ", ",").split(","):
                    if not f:
                        continue
                    owner, _, feat = f.rpartition("/")
                    p = PKGS.get(owner) if owner else pkg
                    if owner and owner not in PKGS:
                        findings.append(f"{where}: feature `{f}` names package `{owner}`, which is not in the workspace")
                    elif p is not None and feat not in p["features"]:
                        findings.append(f"{where}: package {p['name']} has no feature `{feat}`")
                if "=" not in w:
                    i += 1
            elif w == "--manifest-path":
                if not (ROOT / nxt).exists():
                    findings.append(f"{where}: --manifest-path {nxt} does not exist")
                i += 1
            i += 1
    for j, w in enumerate(words):
        if w in ("-f", "--filename", "--file", "-c") and j + 1 < len(words) and words[0] in ("docker", "kubectl", "podman"):
            checked += 1
            t = words[j + 1]
            if PATHISH.match(t) and not (ROOT / t.lstrip("./")).exists():
                findings.append(f"{where}: `{words[0]} ... {w} {t}`: no such file")
    first = words[0]
    if PATHISH.match(first) and first.startswith(("./scripts/", "scripts/", "./deploy/", "deploy/")):
        checked += 1
        if not (ROOT / first.lstrip("./")).exists():
            findings.append(f"{where}: `{first}` does not exist")


for p in book_blocks.pages():
    for b in book_blocks.blocks(p):
        if b["lang"] not in ("bash", "sh", "console", "shell"):
            continue
        text = re.sub(r"\\\n\s*", " ", b["text"])
        for n, line in enumerate(text.splitlines()):
            line = line.strip().removeprefix("$ ")
            if line and not line.startswith("#"):
                check_line(f"{p.relative_to(ROOT)}:{b['line'] + n}", line)

# Example READMEs are not in SUMMARY.md; read them too.
for readme in sorted((ROOT / "examples").glob("*/README.md")) + [ROOT / "tools/a2a-cli/README.md", ROOT / "deploy/observability/README.md"]:
    for b in book_blocks.blocks(readme):
        if b["lang"] in ("bash", "sh", "console", "shell"):
            text = re.sub(r"\\\n\s*", " ", b["text"])
            for n, line in enumerate(text.splitlines()):
                line = line.strip().removeprefix("$ ")
                if line and not line.startswith("#"):
                    check_line(f"{readme.relative_to(ROOT)}:{b['line'] + n}", line)

for f in findings:
    print(f)
print(f"02-commands-static: {checked} command(s)/paths checked, {len(findings)} finding(s)")
sys.exit(1 if findings else 0)
