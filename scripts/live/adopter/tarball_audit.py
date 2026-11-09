#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""What is in each published tarball, and what a build of it would need.

Reads the `.crate` files `cargo package` wrote (01-package.sh), lists each
tarball by top-level directory, flags what an adopter does not need
(tests, benches, fuzz inputs, CI files, editor files, large data) and
checks the inputs the crate's own `build.rs` and `include_str!` /
`include_bytes!` / `include!` / `#[doc = include_str!]` calls name are
present in the tarball.

Usage: tarball_audit.py PACKAGE_DIR   (PACKAGE_DIR is target/package)
Exit 1 when a file a build input names is missing from its tarball.
"""
import re
import sys
import tarfile
from collections import Counter
from pathlib import Path, PurePosixPath

SUSPECT = re.compile(
    r"(^|/)(tests?|benches|fuzz|\.github|\.gitignore|\.config|corpus|target|"
    r"\.idea|\.vscode|node_modules|\.DS_Store|.*\.(orig|rej|swp|bak|log|profraw))(/|$)"
)


def main():
    pkgdir = Path(sys.argv[1])
    bad = 0
    for crate in sorted(pkgdir.glob("a2a-protocol-*.crate")):
        with tarfile.open(crate) as tf:
            members = [m for m in tf.getmembers() if m.isfile()]
            root = members[0].name.split("/")[0]
            names = [m.name[len(root) + 1 :] for m in members]
            sizes = {m.name[len(root) + 1 :]: m.size for m in members}
            texts = {}
            for m in members:
                n = m.name[len(root) + 1 :]
                if n.endswith((".rs", ".toml", ".proto")) or n == "build.rs":
                    texts[n] = tf.extractfile(m).read().decode("utf-8", "replace")
        print(f"=== {root}: {len(names)} files, {sum(sizes.values())/1024:.0f} KiB uncompressed")
        top = Counter(n.split("/")[0] if "/" in n else n for n in names)
        for k, v in sorted(top.items()):
            print(f"  {k:30} {v:4}")
        print("  largest:")
        for n in sorted(names, key=lambda n: -sizes[n])[:6]:
            print(f"    {sizes[n]/1024:8.1f} KiB  {n}")
        sus = [n for n in names if SUSPECT.search(n)]
        print(f"  files an adopter does not need (tests/benches/fuzz/CI/...): {len(sus)}")
        for n in sus[:8]:
            print(f"    {n}")
        # Inputs named by include!-family macros and build.rs, resolved
        # relative to the file that names them.
        have = set(names)
        for n, t in texts.items():
            base = PurePosixPath(n).parent
            for m in re.finditer(
                r'(?:include_str!|include_bytes!|include!)\(\s*(?:concat!\(env!\("CARGO_MANIFEST_DIR"\),\s*)?"([^"]+)"', t
            ):
                p = m.group(1)
                if "OUT_DIR" in t[max(0, m.start() - 80) : m.start()]:
                    continue
                if p.startswith("/"):
                    continue
                target = PurePosixPath(
                    p.lstrip("/") if "CARGO_MANIFEST_DIR" in t[max(0, m.start() - 80) : m.end()] else str(base / p)
                )
                parts = []
                for seg in target.parts:
                    if seg == "..":
                        if parts:
                            parts.pop()
                    elif seg != ".":
                        parts.append(seg)
                res = "/".join(parts)
                if res not in have:
                    print(f"  MISSING: {n} names `{p}` -> {res}")
                    bad += 1
        if "build.rs" in texts:
            for m in re.finditer(r'"((?:\.\./|proto/|\./)[^"]+\.proto)"', texts["build.rs"]):
                p = m.group(1)
                print(f"  build.rs names proto: {p}  in tarball: {p in have}")
                if p not in have:
                    bad += 1
            print("  build.rs present; protos in tarball:",
                  sorted(n for n in names if n.endswith(".proto")))
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
