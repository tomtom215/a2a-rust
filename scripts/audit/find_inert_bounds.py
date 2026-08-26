#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Finds configurable bounds that do not bound what they claim to.

Why this exists
---------------
`docs/v0.9.0-post-release-review.md` names a shape it hit five times across
five passes: **a knob whose default masks its own absence.** A configurable
bound is dropped, or half-applied, or never read — and nothing notices, because
the fallback that takes over has the same value as the default. It is inert
until somebody *tightens* it, and the person who tightens a limit is the person
who decided the default was wrong for them. They then hold a false belief
instead of a bound.

Every one of those five was found by a person reading code on purpose. This
script is the attempt to make the next one cost a run instead of a pass.

This is NOT a CI gate
---------------------
Signature C reports 29 candidates against 3 worth reading, and most of the 26
are legitimate — a knob that genuinely belongs to one implementation of a
trait. A check that cries 26 times is a check people learn to skip. Promoting
it needs an allowlist of family/knob pairs with a reason each; that is backlog
item B21, and the allowlist is the deliverable, not the script.

Run it by hand, read every hit, and expect to explain rather than to fix. The
two false positives in the last sweep were worth more than the one defect: one
of them was correct-but-untested, and disproving it is what produced the test.

The signatures
--------------
A  A knob nothing reads. Every mention is a declaration, a `Default`
   initialiser, a `with_*` assignment, or a test. Found
   `ClientConfig::preferred_bindings`, which documented an ordered preference
   the builder never consulted.

C  A knob honoured by some members of a family and not all, where a family is
   the set of types implementing one trait. This is the shape the hand-found
   instances actually had: `max_page_size` was read by the in-memory store,
   `body_read_timeout` on one route — so A is silent on them. What is wrong is
   that the readers are a strict subset of the implementations that owe it.

   Read the ratio, not the count. "Majority honours it, a minority does not"
   is the shape; "one implementation honours it" is usually a knob that belongs
   to that implementation.

B  (tried, abandoned) A literal equal to some config default, in a bound
   position — how instances 3 and 4 hid. Seventeen hits, all coincidence:
   `1024` as a channel buffer colliding with `HandlerLimits::max_id_length` and
   the like. A number is not evidence of a relationship to another number. Not
   shipped, and recorded here so nobody spends the afternoon again.

Usage
-----
    scripts/audit/find_inert_bounds.py            # both signatures
    scripts/audit/find_inert_bounds.py --only a

Always exits 0. It reports; it does not judge.
"""

import argparse
import collections
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parents[2]

CFG_TEST_MOD = re.compile(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+[A-Za-z_]\w*\s*\{")
STRUCT = re.compile(r"pub struct ([A-Za-z_]\w*(?:Config|Limits|Policy|Options|Settings))\s*[\{<]")
FIELD = re.compile(r"^\s*pub (?:const )?([a-z_]\w*)\s*:", re.M)
IMPL = re.compile(
    r"^\s*impl(?:<[^>]*>)?\s+(?:async\s+)?([A-Za-z_]\w*)(?:<[^>]*>)?\s+for\s+([A-Za-z_]\w*)", re.M
)
TRAIT_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?trait\s+([A-Za-z_]\w*)", re.M)

# A knob is a *bound* if its name says so. "Some honour it, some do not" is a
# defect for a bound — a cap half the implementations ignore is not a cap. For
# an ordinary data field it is usually just a different shape.
BOUND = re.compile(
    r"^(max_|min_|default_max_)|"
    r"(_timeout|_limit|_size|_capacity|_interval|_ttl|_retries|"
    r"_connections|_depth|_bytes|_backoff|_attempts|_age|_window)$"
)


def matching_brace(text: str, open_at: int) -> int:
    """Index just past the `}` closing the `{` that precedes `open_at`."""
    depth, i = 1, open_at
    while i < len(text) and depth:
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
        i += 1
    return i


def strip_tests(text: str) -> str:
    """Blank every `#[cfg(test)] mod` body, preserving offsets and line numbers.

    Blanking rather than deleting is deliberate: an earlier detector in this
    repository truncated at the first `#[cfg(test)]` it saw, which in one file
    was a `mod fixtures;` declaration on line 29. It read 29 lines of the file
    it existed for and printed a confident zero.
    """
    out, pos = text, 0
    while True:
        m = CFG_TEST_MOD.search(out, pos)
        if not m:
            return out
        end = matching_brace(out, m.end())
        out = out[: m.start()] + re.sub(r"[^\n]", " ", out[m.start() : end]) + out[end:]
        pos = end


def strip_comments(text: str) -> str:
    """Blank `//` line comments, doc comments included.

    This is not tidiness. The first version of signature A counted a doc
    example as a reader: `tenant_config.rs`'s module header contains
    `config.get("premium-corp").max_concurrent_tasks`, which looks exactly like
    a read, and so four of the five inert `TenantLimits` fields were reported
    as live. The one field the example does not mention was the only one
    flagged. A detector that reads prose as code under-reports precisely where
    the prose is most confident.

    Block comments are left alone: this repository uses `//` and `///`
    throughout, and a `/* */` scanner that mishandles a string literal would
    trade a known miss for an unknown one.
    """
    return "\n".join(re.sub(r"//.*$", "", line) for line in text.splitlines())


def sources() -> dict:
    paths = sorted(
        list(REPO.glob("crates/*/src/**/*.rs")) + list(REPO.glob("bindings/*/src/**/*.rs"))
    )
    if not paths:
        sys.exit(f"no sources found under {REPO} — wrong repo root?")
    return {p: strip_comments(strip_tests(p.read_text(encoding="utf-8"))) for p in paths}


def knob_names(bodies: dict) -> set:
    names = set()
    for body in bodies.values():
        for m in STRUCT.finditer(body):
            brace = body.find("{", m.end() - 1)
            if brace < 0:
                continue
            end = matching_brace(body, brace + 1)
            names |= {f.group(1) for f in FIELD.finditer(body[brace:end])}
    return names


def signature_a(bodies: dict, knobs: set) -> int:
    """A knob whose every mention is a declaration, a default, a setter, or a test."""
    print("=" * 78)
    print("SIGNATURE A — a knob nothing reads")
    print("=" * 78)
    hits = 0
    for knob in sorted(knobs):
        readers = []
        for path, body in bodies.items():
            for line in body.splitlines():
                if not re.search(rf"(?:\.{knob}\b|\b{knob}\s*[:,=])", line):
                    continue
                stripped = line.strip()
                declaration = re.match(rf"pub (?:const )?{knob}\s*:", stripped)
                initialiser = re.match(rf"{knob}\s*:", stripped)
                setter = re.search(rf"self\.\w*\.?{knob}\s*=", stripped)
                if declaration or initialiser or setter:
                    continue
                readers.append((path, stripped))
        if not readers:
            hits += 1
            print(f"\n  `{knob}` — declared, defaulted, settable, never consulted")
    print(f"\n{hits} candidate(s)\n")
    return hits


def signature_c(bodies: dict, knobs: set) -> int:
    """A bound honoured by some members of a trait's implementations, not all."""
    families = collections.defaultdict(lambda: collections.defaultdict(set))
    for path, body in bodies.items():
        for m in IMPL.finditer(body):
            families[m.group(1)][m.group(2)].add(path)

    # Only traits this repository declares. A foreign trait (TryFrom, From,
    # FromRequest) groups types that are not alternative implementations of one
    # policy, so "member A mentions the field and member B does not" carries no
    # signal — TryFrom alone produced 44 of the first run's 73 hits.
    own_traits = set()
    for body in bodies.values():
        own_traits |= {m.group(1) for m in TRAIT_DECL.finditer(body)}

    print("=" * 78)
    print("SIGNATURE C — a bound honoured by some of a family, not all")
    print("=" * 78)
    print("Read the ratio. Majority-honoured is the shape; one-honoured is")
    print("usually a knob that belongs to that one implementation.\n")

    hits = 0
    for trait, impls in sorted(families.items()):
        if len(impls) < 2 or trait not in own_traits:
            continue
        for knob in sorted(knobs):
            if not BOUND.search(knob):
                continue
            have = {
                ty
                for ty, paths in impls.items()
                if any(re.search(rf"(?:\.{knob}\b|\b{knob}\s*[:,=])", bodies[p]) for p in paths)
            }
            missing = set(impls) - have
            if not have or not missing:
                continue
            hits += 1
            ratio = f"{len(have)}/{len(impls)}"
            print(f"{trait}  knob `{knob}`  [{ratio}]")
            print(f"    honoured by : {', '.join(sorted(have))}")
            print(f"    NOT in      : {', '.join(sorted(missing))}\n")
    print(f"{hits} candidate(s)\n")
    return hits


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--only", choices=["a", "c"], help="run one signature")
    args = parser.parse_args()

    bodies = sources()
    knobs = knob_names(bodies)
    print(f"{len(bodies)} sources, {len(knobs)} config fields\n")

    if args.only != "c":
        signature_a(bodies, knobs)
    if args.only != "a":
        signature_c(bodies, knobs)
    return 0


if __name__ == "__main__":
    sys.exit(main())
