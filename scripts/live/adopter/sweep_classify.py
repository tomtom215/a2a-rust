#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Group the failures of 03-book-sweep.sh by page, with the first compiler error.

    sweep_classify.py DEFAULT_LOG [ALL_LOG]

For each doctest that failed under the default-feature build it prints the page
and line, the first `error` line, whether the same block failed with every
feature on (ALL_LOG), and the features the page itself names (`features = [..]`,
`--features x`, or a backticked feature word), so a block that needs a feature the
page never mentions stands out.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
FEATURES = {
    "signing", "audit", "proto", "websocket", "grpc", "grpc-tls", "testing", "sqlite", "postgres",
    "otlp", "otlp-json", "otel-prometheus", "conformance", "axum", "tool-loop", "auth-jwt",
    "encryption", "tls-rustls", "otel", "tracing", "webhook",
}


def parse(path):
    text = Path(path).read_text()
    res = {}
    for m in re.finditer(r"^test (\S+) - (\S+) \(line (\d+)\)(?: - compile)? \.\.\. (ok|FAILED|ignored)", text, re.M):
        page, _, line, st = m.groups()
        res[(page, int(line))] = st
    errs = {}
    for m in re.finditer(r"^---- (\S+) - (\S+) \(line (\d+)\)(?: - compile)? stdout ----\n(.*?)(?=^---- |^failures:|\Z)", text, re.M | re.S):
        page, _, line, body = m.groups()
        e = re.search(r"^error(?:\[E\d+\])?: .*", body, re.M)
        errs[(page, int(line))] = (e.group(0) if e else body.strip().splitlines()[0:2])
    return res, errs


def page_features(page):
    p = ROOT / page.replace("src/../pages/", "")
    if not p.exists():
        return set()
    t = p.read_text()
    named = set()
    for m in re.finditer(r"features\s*=\s*\[([^\]]*)\]", t):
        named |= set(re.findall(r'"([a-z0-9-]+)"', m.group(1)))
    for m in re.finditer(r"--features[ =]([a-z0-9,-]+)", t):
        named |= set(m.group(1).split(","))
    for m in re.finditer(r"`([a-z0-9-]+)`", t):
        if m.group(1) in FEATURES:
            named.add(m.group(1))
    return named & FEATURES


# Which feature a compiler error is about, by the symbol it cannot find.
NEEDS = [
    ("tool_loop", "tool-loop"), ("audit", "audit"), ("WebSocket", "websocket"),
    ("websocket", "websocket"), ("signing", "signing"), ("ContentSigner", "signing"),
    ("provenance", "signing"), ("Postgres", "postgres"), ("Jwt", "auth-jwt"), ("jwt", "auth-jwt"),
    ("build_grpc", "grpc"), ("grpc", "grpc"), ("Grpc", "grpc"), ("Sqlite", "sqlite"),
    ("sqlite", "sqlite"), ("RestRouter", "axum"), ("A2aRouter", "axum"), ("conformance", "conformance"),
    ("types::proto", "proto"), ("Encrypted", "encryption"), ("encrypt", "encryption"),
]


def needed(err):
    err = str(err)
    for k, v in NEEDS:
        if k in err:
            return v
    return None


def main():
    dres, derr = parse(sys.argv[1])
    ares = parse(sys.argv[2])[0] if len(sys.argv) > 2 else {}
    fails = sorted(k for k, v in dres.items() if v == "FAILED")
    print(f"default-feature doctests: {len(dres)}  failed: {len(fails)}")
    both = 0
    unnamed = []
    for k in fails:
        page, line = k
        a = ares.get(k, "-")
        both += a == "FAILED"
        e = derr.get(k, "")
        named = sorted(page_features(page))
        want = needed(e)
        if want is None or want not in named:
            unnamed.append((page, line, want, named, e))
        print(f"{page.replace('src/../pages/','')}:{line}  all-features={a}\n    {e}\n    page names features: {named or 'NONE'}")
    print(f"\nblocks whose page never names the feature the block needs (or the symbol is not one I can map): {len(unnamed)}")
    for page, line, want, named, e in unnamed:
        print(f"  {page.replace('src/../pages/','')}:{line}  needs {want}; page names {named or 'none'}; {str(e)[:100]}")
    if ares:
        print(f"\nfailed in both default and all-features: {both}")


if __name__ == "__main__":
    main()
