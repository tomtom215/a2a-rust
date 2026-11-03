#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Apply rust_literal's rewrite to a checkout: apply.py ROOT [--dry-run] [--report FILE]

Which files, and why:
  .rs  everything tracked except the generated types, other SDKs' benchmark harnesses
       (benches/sdk-comparison pins published crates and other SDKs) and, for code
       (not doc comments), the types crate's own src/, where a struct literal is
       still legal. Doc comments everywhere are converted: a doctest is an
       external caller.
  .md  fenced rust blocks of the book and the READMEs; not the changelog, the audit
       and handoff records, or the upgrade guides, whose old-API snippets are the
       point.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from rust_literal import convert  # noqa: E402
from specs import SPECS  # noqa: E402

WRAP = 96
IN_CRATE = "crates/a2a-protocol-types/src/"
SKIP_RS = ("crates/a2a-protocol-types/src/generated/", "benches/sdk-comparison/", "target/")
SKIP_MD = ("CHANGELOG.md", "docs/", "RELEASING.md", "STABILITY.md", "CONTRIBUTING.md", "SPEC_COMPLIANCE.md",
           "book/src/reference/upgrading/", "target/", "benches/sdk-comparison/")
FENCE = re.compile(r"^(\s*)```([A-Za-z0-9_,\- ]*)\s*$")
RUSTY = re.compile(r"^(rust|)(,.*)?$")


def is_rust_fence(info: str) -> bool:
    info = info.strip()
    # `no_run`, `should_panic`, `compile_fail`, `ignore` and `edition…` are rustdoc's words, not languages: a
    # fence that opens with one is Rust (the first version missed `no_run` and left one literal behind).
    first = info.split(",")[0].strip()
    if first in ("", "rust", "no_run", "should_panic", "compile_fail", "ignore") or first.startswith("edition"):
        return "text" not in info and "toml" not in info
    return False


DOC = re.compile(r"^(\s*(?:///|//!) ?)(.*)$")


def convert_fences(lines: list[str], doc: bool, stats: dict, where: str) -> list[str]:
    """Convert the rust fenced blocks of `lines`: markdown, or (doc=True) the `///` and `//!`
    comment lines of a Rust file. Every fence is consumed with its closing fence, rust or not, so a
    closing fence is never read as the start of the next block."""
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        if doc:
            m = DOC.match(line)
            body = m.group(2) if m else None
        else:
            m, body = None, line
        fm = FENCE.match(body) if body is not None else None
        if not fm:
            out.append(line)
            i += 1
            continue
        j, block, closed = i + 1, [], False
        while j < len(lines):
            if doc:
                mm = DOC.match(lines[j])
                if not mm:
                    break  # the comment ended with the fence still open
                b = mm.group(2)
            else:
                b = lines[j]
            if FENCE.match(b):
                closed = True
                break
            block.append(b)
            j += 1
        if not closed or not is_rust_fence(fm.group(2)):
            out.extend(lines[i:j + 1] if closed else lines[i:j])
            i = j + 1 if closed else j
            continue
        code = "\n".join(block)
        res = convert(code, SPECS, wrap=WRAP)
        for item in res.skipped:
            stats["skipped"].append(f"{where}:{i + 1}: {item}")
        stats["converted"] += res.converted
        stats["patterns"] += res.patterns
        out.append(line)
        if res.text == code:
            out.extend(lines[i + 1:j])
        else:
            prefix = m.group(1) if doc and m else ""
            for nb in res.text.split("\n"):
                out.append((prefix + nb) if nb else prefix.rstrip())
        out.append(lines[j])
        i = j + 1
    return out


def process_rs(path: str, text: str, stats: dict) -> str:
    if not path.startswith(IN_CRATE):
        res = convert(text, SPECS)
        for item in res.skipped:
            stats["skipped"].append(f"{path}: {item}")
        stats["converted"] += res.converted
        stats["patterns"] += res.patterns
        text = res.text
    lines = text.split("\n")
    lines = convert_fences(lines, True, stats, path)
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    root = Path(argv[0]).resolve()
    dry = "--dry-run" in argv
    report = Path(argv[argv.index("--report") + 1]) if "--report" in argv else None
    files = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True, check=True).stdout.split("\n")
    stats = {"converted": 0, "patterns": 0, "skipped": [], "files": []}
    for f in files:
        if not f:
            continue
        if f.endswith(".rs") and not f.startswith(SKIP_RS):
            kind = "rs"
        elif f.endswith(".md") and not f.startswith(SKIP_MD) and not any(f.endswith(s) for s in ("CHANGELOG.md",)):
            kind = "md"
        else:
            continue
        p = root / f
        try:
            text = p.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        before = (stats["converted"], stats["patterns"])
        new = process_rs(f, text, stats) if kind == "rs" else "\n".join(convert_fences(text.split("\n"), False, stats, f))
        if new != text:
            stats["files"].append(f)
            if not dry:
                p.write_text(new, encoding="utf-8")
    print(f"converted {stats['converted']} literals, added `..` to {stats['patterns']} patterns, "
          f"changed {len(stats['files'])} files, left {len(stats['skipped'])} for hand")
    if report:
        report.write_text(json.dumps(stats, indent=1))
    for s in stats["skipped"][:60]:
        print("  skipped:", s)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
