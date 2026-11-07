#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Scan the published tarballs for things nobody should ship.

    tarball_scan.py PACKAGE_DIR

Reports: private-key blocks, bearer/API-key-looking strings, absolute paths of
a developer's machine, `.env` files, binary blobs, the licence files present,
and manifests' metadata (license, readme, repository, rust-version).
"""
import re
import sys
import tarfile
import tomllib
from pathlib import Path

PATTERNS = {
    "private key block": re.compile(rb"-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----"),
    "github token": re.compile(rb"gh[pousr]_[A-Za-z0-9]{30,}"),
    "aws key id": re.compile(rb"AKIA[0-9A-Z]{16}"),
    "slack token": re.compile(rb"xox[baprs]-[A-Za-z0-9-]{10,}"),
    "developer path": re.compile(rb"/(?:home|Users)/[a-z][a-z0-9_-]+/[A-Za-z0-9_./-]{3,}"),
    "claude path": re.compile(rb"/root/\.claude|/tmp/claude"),
    "email other than maintainer": re.compile(rb"[A-Za-z0-9._%+-]+@(?!tomtomtech\.net|example\.(?:com|org)|users\.noreply)[A-Za-z0-9.-]+\.[a-z]{2,}"),
}


def main():
    pkgdir = Path(sys.argv[1])
    for crate in sorted(pkgdir.glob("a2a-protocol-*.crate")):
        print(f"=== {crate.name}")
        hits = {}
        with tarfile.open(crate) as tf:
            members = [m for m in tf.getmembers() if m.isfile()]
            root = members[0].name.split("/")[0]
            licence = [m.name[len(root) + 1 :] for m in members if re.search(r"(?i)(licen[cs]e|notice|copying)", m.name.split("/")[-1])]
            print("  licence/notice files in tarball:", licence or "NONE")
            for m in members:
                n = m.name[len(root) + 1 :]
                data = tf.extractfile(m).read()
                if b"\0" in data[:4096] and not n.endswith((".png", ".gz")):
                    hits.setdefault("binary file", []).append((n, len(data)))
                for label, rx in PATTERNS.items():
                    for mm in rx.finditer(data):
                        hits.setdefault(label, []).append((n, mm.group(0)[:60].decode("utf-8", "replace")))
                if n == "Cargo.toml":
                    t = tomllib.loads(data.decode())
                    p = t["package"]
                    print("  manifest:", {k: p.get(k) for k in ("name", "version", "license", "readme", "repository", "rust-version", "edition", "description")})
        for label, items in hits.items():
            files = sorted({i[0] for i in items})
            print(f"  {label}: {len(items)} in {len(files)} file(s)")
            for n, what in items[:4]:
                print(f"    {n}: {what}")


if __name__ == "__main__":
    main()
