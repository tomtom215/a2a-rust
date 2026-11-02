#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fails when an example serves an agent with a hand-written hyper accept loop.

`Server` (`a2a_protocol_server::serve`) accepts connections, caps them, drains
them on shutdown and survives an accept error that is only a descriptor limit.
The loop an example would write instead,

    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        ... serve_connection(io, service) ...
    }

does none of that, and its `continue` spins a core on a persistent accept error.
The examples carried fifteen of them (audit G-05), so a reader copying "how do
I serve an agent" copied the defect.

A stub the example needs to misbehave or to answer a fixed way (a webhook sink,
a fault injector, a JWKS document) is not an agent and has no use for `Server`.
Those are listed in `scripts/example_accept_loops.txt`, one line per file:

    <path> <count>  # why this is not an agent server

The check fails on a loop that is not listed, on a count above the listed one,
and on an entry that no longer matches anything.

Usage:
    python3 scripts/check_example_servers.py
    python3 scripts/check_example_servers.py --self-test
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ALLOWLIST = ROOT / "scripts" / "example_accept_loops.txt"

# `serve_connection(` within this many lines after an `accept()` that is in a loop.
WINDOW = 60


def strip_comments(source):
    return re.sub(r"//[^\n]*", "", source)


def hand_rolled_loops(source):
    """Number of accept loops in `source` that serve a hyper connection."""
    lines = strip_comments(source).split("\n")
    found = 0
    for i, line in enumerate(lines):
        if ".accept()" not in line:
            continue
        # In a loop: `while let` on the same line, or a `loop {` shortly above.
        above = "\n".join(lines[max(0, i - 4) : i + 1])
        if not (re.search(r"\bwhile\s+let\b", line) or re.search(r"\bloop\s*\{", above)):
            continue
        below = "\n".join(lines[i : i + WINDOW])
        if "serve_connection(" in below:
            found += 1
    return found


def load_allowlist(text):
    allowed = {}
    for raw in text.splitlines():
        line = raw.split("#", 1)[0].strip()
        if not line:
            continue
        path, count = line.rsplit(None, 1)
        allowed[path] = int(count)
    return allowed


def check(counts, allowed):
    problems = []
    for path, n in sorted(counts.items()):
        have = allowed.get(path, 0)
        if n > have:
            problems.append(
                f"{path}: {n} hand-rolled accept loop(s), {have} allowed. Serve the agent with "
                "`Server::from_listener(listener).serve_with_shutdown(dispatcher, ..)` "
                "(`CombinedDispatcher` for JSON-RPC and REST on one port), or list a stub in "
                "scripts/example_accept_loops.txt with its reason"
            )
    for path, have in sorted(allowed.items()):
        n = counts.get(path, 0)
        if n < have:
            problems.append(
                f"{path}: listed with {have} but has {n}: lower the count or remove the line"
            )
    return problems


def scan():
    counts = {}
    for p in sorted((ROOT / "examples").rglob("*.rs")):
        if "target" in p.parts:
            continue
        n = hand_rolled_loops(p.read_text(encoding="utf-8"))
        if n:
            counts[str(p.relative_to(ROOT))] = n
    return counts


def run():
    counts = scan()
    allowed = load_allowlist(ALLOWLIST.read_text(encoding="utf-8"))
    problems = check(counts, allowed)
    for p in problems:
        print(f"check_example_servers: {p}", file=sys.stderr)
    if problems:
        return 1
    print(
        f"check_example_servers: {sum(counts.values())} listed stub loop(s) in {len(counts)} "
        "file(s), no hand-rolled agent server"
    )
    return 0


LOOP = """
fn serve(listener: L, d: D) {
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let _ = Builder::new(TokioExecutor::new()).serve_connection(io, svc).await;
            });
        }
    });
}
"""


def self_test():
    assert hand_rolled_loops(LOOP) == 1, "the loop is found"
    assert hand_rolled_loops(LOOP.replace("serve_connection", "other")) == 0, "no hyper serving"
    assert hand_rolled_loops(LOOP.replace("loop {", "{")) == 0, "an accept outside a loop"
    assert hand_rolled_loops("// " + LOOP.replace("\n", "\n// ")) == 0, "a commented-out loop"
    assert hand_rolled_loops(LOOP + LOOP) == 2, "two loops"
    wl = load_allowlist("a/b.rs 2  # stub\n\n# comment\nc.rs 1\n")
    assert wl == {"a/b.rs": 2, "c.rs": 1}, wl
    assert check({"a/b.rs": 2}, {"a/b.rs": 2}) == []
    assert len(check({"a/b.rs": 3}, {"a/b.rs": 2})) == 1, "more than listed"
    assert len(check({"x.rs": 1}, {})) == 1, "unlisted"
    assert len(check({}, {"a/b.rs": 1})) == 1, "stale entry"
    print("check_example_servers self-test: 9 assertions passed")
    return 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv else run())
