#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Relative links and anchors of every tracked Markdown file, outside the book.

`scripts/check_book_links.py` checks the rendered book. This checks the rest of
what a stranger opens on GitHub: the README, CONTRIBUTING, STABILITY, ROADMAP,
SECURITY, the crate and example READMEs and `docs/`. For each `](target)` that is
not a URL it checks the file or directory exists, and for `#anchor` that the target
file has a heading whose GitHub slug is that anchor. `docs/audits/2026-10-29/` and
`docs/audits/2026-11-07-adopter-walkthrough/` are skipped (records, not instructions).

Usage: 07-links.py [--all]    (--all also reads docs/, default: README-level files, crates, examples, tools, deploy)
Exit 1 when a link is dead.
"""
import re
import subprocess
import sys
from pathlib import Path
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[3]
files = subprocess.run(["git", "ls-files", "*.md"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.split()
SKIP = ("docs/audits/2026-10-29/", "docs/audits/2026-11-07-adopter-walkthrough/", "book/src/")
if "--all" not in sys.argv:
    SKIP += ("docs/",)
LINK = re.compile(r"(?<!\!)\[[^\]]*\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")
FENCE = re.compile(r"^([ \t]*)(`{3,}|~{3,}).*?^\1\2[ \t]*$", re.M | re.S)


def slug(h):
    h = re.sub(r"[`*_]", "", h.strip().lower())
    h = re.sub(r"<[^>]+>", "", h)
    h = re.sub(r"[^\w\- ]", "", h, flags=re.U)
    return h.replace(" ", "-")


def anchors(path):
    text = FENCE.sub("", path.read_text(errors="replace"))
    seen, out = {}, set()
    for m in re.finditer(r"^#{1,6}\s+(.*?)\s*#*\s*$", text, re.M):
        s = slug(m.group(1))
        n = seen.get(s, 0)
        out.add(s if n == 0 else f"{s}-{n}")
        seen[s] = n + 1
    return out


bad, n = [], 0
for f in files:
    if f.startswith(SKIP):
        continue
    p = ROOT / f
    text = FENCE.sub(lambda m: "\n" * m.group(0).count("\n"), p.read_text(errors="replace"))
    for m in LINK.finditer(text):
        t = m.group(1)
        if re.match(r"^(https?:|mailto:|tel:|#?$)", t) or t.startswith("<"):
            continue
        n += 1
        path_part, _, frag = t.partition("#")
        line = text.count("\n", 0, m.start()) + 1
        target = (p.parent / unquote(path_part)).resolve() if path_part else p
        try:
            target.relative_to(ROOT)
        except ValueError:
            bad.append(f"{f}:{line}: {t} leaves the repository")
            continue
        if not target.exists():
            bad.append(f"{f}:{line}: {t} (no such file)")
        elif frag and target.is_file() and target.suffix == ".md" and slug(unquote(frag)) not in anchors(target) and unquote(frag) not in anchors(target):
            bad.append(f"{f}:{line}: {t} (no heading with that anchor)")
for b in bad:
    print(b)
print(f"07-links: {n} relative link(s) in {len([f for f in files if not f.startswith(SKIP)])} file(s), {len(bad)} dead")
sys.exit(1 if bad else 0)
