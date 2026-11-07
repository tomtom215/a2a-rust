#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Make a stranger's project from what a page shows, verbatim.

    mkproject.py NAME --deps PAGE:LANG:N [--deps ...] --main PAGE:LANG:N
                      [--bin NAME=PAGE:LANG:N ...] [--extra-dep 'line' ...]

* PAGE is a path relative to the repository (README.md, book/src/...md).
* LANG:N picks the N-th (0-based) fenced block of that language on the page.
* The `[dependencies]` text is the page's `toml` block, verbatim (the
  `[dependencies]` header is added when the block has none). `--extra-dep`
  adds a line the page does not give, and is reported: it marks a page that
  does not say everything its code needs.
* Dependencies on this project's own crates resolve to the unpacked tarballs
  (`$ADOPTER_SCRATCH/unpacked`, from 01-package.sh) through `[patch.crates-io]`,
  so a file missing from a published tarball fails here as it would for an
  adopter. No `RUSTFLAGS`, no lock file from the repository.

The project goes to `$ADOPTER_SCRATCH/adopter-projects/NAME`, replacing it.
"""
import argparse
import os
import re
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]


def block(spec):
    page, lang, n = spec.rsplit(":", 2)
    text = (ROOT / page).read_text().splitlines()
    found = []
    i = 0
    while i < len(text):
        m = re.match(r"^(\s*)(`{3,})\s*([A-Za-z0-9_+-]*)(.*)$", text[i])
        if m and m.group(2):
            indent, fence, l, rest = m.groups()
            j = i + 1
            while j < len(text) and not re.match(r"^\s*" + fence + r"\s*$", text[j]):
                j += 1
            if l == lang:
                found.append("\n".join(x[len(indent):] if x.startswith(indent) else x for x in text[i + 1 : j]))
            i = j + 1
        else:
            i += 1
    return found[int(n)]


def unhide(src):
    """mdbook/rustdoc hide lines that start with `# `; a reader sees the rest."""
    out = []
    for line in src.splitlines():
        if re.match(r"^\s*#( |$)", line):
            continue  # hidden by mdbook and rustdoc: the reader never sees it
        out.append(re.sub(r"^(\s*)##", r"\1#", line))  # `##` shows as `#`
    return "\n".join(out) + "\n"


def rustdoc_view(src):
    """What rustdoc compiles: the hidden `# ` lines are kept, minus their marker."""
    out = []
    for line in src.splitlines():
        m = re.match(r"^(\s*)# ?(.*)$", line)
        if m and not re.match(r"^\s*#[!\[]", line) and not line.lstrip().startswith("##"):
            out.append(m.group(1) + m.group(2))
        else:
            out.append(re.sub(r"^(\s*)##", r"\1#", line))
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("name")
    ap.add_argument("--deps", action="append", default=[])
    ap.add_argument("--main", action="append", default=[], help="repeatable: blocks are concatenated in order")
    ap.add_argument("--bin", action="append", default=[])
    ap.add_argument("--extra-dep", action="append", default=[])
    ap.add_argument("--show-hidden", action="store_true", help="keep rustdoc-hidden `# ` lines (what rustdoc compiles)")
    a = ap.parse_args()

    scratch = Path(os.environ["ADOPTER_SCRATCH"])
    proj = scratch / "adopter-projects" / a.name
    tgt = proj / "target"
    if proj.exists():
        for child in proj.iterdir():
            if child.name == "target":
                continue
            shutil.rmtree(child) if child.is_dir() else child.unlink()
    (proj / "src" / "bin").mkdir(parents=True, exist_ok=True)

    deps = []
    for spec in a.deps:
        deps.append(block(spec).strip())
    dep_text = "\n".join(deps)
    if "[dependencies]" not in dep_text:
        dep_text = "[dependencies]\n" + dep_text
    dep_text += "\n" + "\n".join(a.extra_dep) + "\n"

    unpacked = scratch / "unpacked"
    patch = ["[patch.crates-io]"]
    for d in sorted(unpacked.iterdir()):
        crate = re.sub(r"-\d+\.\d+\.\d+$", "", d.name)
        patch.append(f'{crate} = {{ path = "{d}" }}')

    (proj / "Cargo.toml").write_text(
        f'[package]\nname = "{a.name}"\nversion = "0.1.0"\nedition = "2024"\n\n{dep_text}\n' + "\n".join(patch) + "\n"
    )
    conv = rustdoc_view if a.show_hidden else unhide
    if a.main:
        (proj / "src" / "main.rs").write_text("\n".join(conv(block(m)) for m in a.main))
    for spec in a.bin:
        n, s = spec.split("=", 1)
        (proj / "src" / "bin" / f"{n}.rs").write_text(conv(block(s)))
    if not a.main:
        (proj / "src" / "main.rs").write_text("fn main() {}\n")
    print(f"project: {proj}")
    print(f"target:  {tgt}")
    if a.extra_dep:
        print("extra dependency lines the page did not give:", a.extra_dep)


if __name__ == "__main__":
    sys.exit(main())
