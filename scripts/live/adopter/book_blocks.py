#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""List the fenced code blocks of the README and the book, page by page.

An adopter types what a page shows. This tool is the inventory behind
`docs/audits/2026-11-07-adopter-walkthrough/02-readme-and-book.md`: for every
page named in book/src/SUMMARY.md (and the README) it prints the page, the
line of each fence, the language tag and attributes, and whether rustdoc would
compile it (a `rust` block that is not `ignore`, `text` or `no_run`-less is
compiled; `no_run` blocks are compiled but not run).

Usage:
    book_blocks.py [--summary] [--json] [--rust-only]

Needs: python3 only. Run it from anywhere inside the checkout.
"""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
BOOK = ROOT / "book" / "src"


def pages():
    out = [ROOT / "README.md"]
    seen = set()
    for m in re.finditer(r"\]\(\./([^)#]+\.md)\)", (BOOK / "SUMMARY.md").read_text()):
        p = BOOK / m.group(1)
        if p not in seen:
            seen.add(p)
            out.append(p)
    return out


def blocks(path):
    lines = path.read_text().splitlines()
    i = 0
    while i < len(lines):
        m = re.match(r"^(\s*)(`{3,}|~{3,})(.*)$", lines[i])
        if not m:
            i += 1
            continue
        indent, fence, info = m.groups()
        start = i + 1
        j = i + 1
        while j < len(lines) and not re.match(r"^\s*" + re.escape(fence) + r"\s*$", lines[j]):
            j += 1
        body = lines[i + 1 : j]
        info = info.strip()
        lang = re.split(r"[,\s{]", info, maxsplit=1)[0] if info else ""
        yield {
            "page": str(path.relative_to(ROOT)),
            "line": start,
            "info": info,
            "lang": lang,
            "text": "\n".join(l[len(indent):] if l.startswith(indent) else l for l in body),
        }
        i = j + 1


def compiled_by_rustdoc(b):
    info = b["info"]
    lang = b["lang"]
    tags = [t for t in re.split(r"[,\s]+", info) if t]
    if lang in ("", "rust") or "rust" in tags:
        return not ({"ignore", "text"} & set(tags)) and not any(t.startswith("ignore") for t in tags)
    return False


def main():
    as_json = "--json" in sys.argv
    rust_only = "--rust-only" in sys.argv
    rows = []
    for p in pages():
        if not p.exists():
            print(f"MISSING PAGE {p}", file=sys.stderr)
            continue
        for b in blocks(p):
            b["compiled"] = compiled_by_rustdoc(b)
            b["has_main"] = bool(re.search(r"\bfn main\b", b["text"]))
            if rust_only and b["lang"] not in ("rust", ""):
                continue
            rows.append(b)
    if as_json:
        json.dump(rows, sys.stdout, indent=1)
        return
    per_lang = {}
    for b in rows:
        per_lang[b["lang"] or "(untagged)"] = per_lang.get(b["lang"] or "(untagged)", 0) + 1
    print(f"pages: {len(pages())}  blocks: {len(rows)}")
    for k, v in sorted(per_lang.items(), key=lambda kv: -kv[1]):
        print(f"  {k:12} {v}")
    rust = [b for b in rows if b["lang"] in ("rust", "")]
    print(f"rust blocks: {len(rust)}  compiled by rustdoc: {sum(b['compiled'] for b in rust)}"
          f"  with fn main: {sum(b['has_main'] for b in rust)}")


if __name__ == "__main__":
    main()
