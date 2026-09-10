#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Fails on a configurable bound that some implementations of a trait honour
and the rest silently do not — unless that pair is allowlisted with a reason.

What problem this solves
------------------------
`docs/v0.9.0-post-release-review.md` names a shape it hit five times across
five passes: **a knob whose default masks its own absence.** A configurable
bound is dropped, half-applied, or never read, and nothing notices, because
the fallback that takes over has the same value as the default. It is inert
until somebody *tightens* it — and the person who tightens a limit is the
person who decided the default was wrong for them. They then hold a false
belief instead of a bound. `TaskStoreConfig::max_page_size` was read by the
in-memory store and by no other; `ClientConfig::preferred_bindings` documented
an ordered preference the builder never consulted. Every one was found by a
person reading code on purpose.

`scripts/audit/find_inert_bounds.py` turned that into two signatures, and it
finds the shape — but it reports 20 candidates for 5 worth reading and always
exits 0, and a check that cries fifteen times is a check people learn to skip
(backlog item B21). This is the gate: the same two signatures, with the audit
script imported unchanged, minus `scripts/inert_bounds_allowlist.txt`. The
allowlist is the audit: every entry carries a reason a person verified by
reading the code, so the gate fails only on a *new* partial bound.

What this checks, and what it does not
--------------------------------------
* **Signature A** — a config field nothing reads: every mention is a
  declaration, a `Default` initialiser, a `with_*` setter or a test.
* **Signature C** — a field whose name says it is a bound (`max_*`,
  `*_timeout`, `*_capacity`, …) that is read by some of the types implementing
  one of this repository's traits and not by the others.

An allowlist entry names the family, the knob and the exact set of members
that do not honour it. Exact, because a new implementation that ignores an
allowlisted knob is a new partial bound and not a covered one: the entry stops
matching, and whoever added the type gets to say why it is right not to. An
entry that matches no candidate at all is itself a failure — the knob became
universal, was renamed, or the family changed — because an exemption for
something that is not happening is how a skip list rots into a blanket waiver.
`deny.toml` says exactly that of its `skip` list, and
`scripts/prove_gates_fail.sh` refuses to run with a `SKIP_STEPS` pattern that
names no step; this gate follows the same rule.

It inherits the audit script's blind spots, each measured and documented
there. It matches *names*, so a knob renamed at a boundary
(`ClientConfig::request_timeout` becomes `GrpcTransportConfig::timeout`) or
applied from a sibling file (`max_capacity` reaches a tenant's partition
through `TenantAwareInMemoryTaskStore`'s delegation) reads as "not honoured".
Those are allowlisted, with the rename or the delegation named in the reason.
It cannot tell a bound that is honoured from one honoured *wrongly*: a read is
a read. And it takes the family from `impl Trait for Type` blocks, so a type
whose `impl` lives in one file and whose bound lives in another is invisible
to signature C — that split has been made once already, to satisfy
`check_file_lengths.sh`.

The candidate logic here mirrors the audit script's two loops rather than
parsing its printed output — a format change would otherwise read as zero
candidates, and zero is the one number this repository's gates have learned
not to trust. To keep the mirror honest the gate also runs the script's own
`signature_a` and `signature_c` over the same sources and refuses (exit 2)
if their counts disagree with its own.

Usage
-----
    python3 scripts/check_inert_bounds.py            # the gate
    python3 scripts/check_inert_bounds.py --explain  # every candidate: family,
                                                     # who honours it, who does
                                                     # not, and the entry that
                                                     # covers it

Exit codes: 0 every candidate is allowlisted with a reason and every entry
matches a candidate; 1 a candidate is not allowlisted, or an entry is stale;
2 not run from the repository root, the audit script or the allowlist cannot
be read or imported, the allowlist is malformed, or this file's copy of the
signatures has drifted from the audit script's.
"""

from __future__ import annotations

import argparse
import contextlib
import importlib.util
import io
import pathlib
import re
import sys
from dataclasses import dataclass

NAME = "check_inert_bounds"
AUDIT = pathlib.Path("scripts/audit/find_inert_bounds.py")
ALLOWLIST = pathlib.Path("scripts/inert_bounds_allowlist.txt")

# A signature-A candidate has no family and no non-honouring members. The
# allowlist spells both as `-`.
NO_FAMILY = "-"


@dataclass(frozen=True)
class Candidate:
    family: str  # trait name, or NO_FAMILY for signature A
    knob: str
    honoured: frozenset[str]  # empty for signature A
    missing: frozenset[str]  # empty for signature A

    @property
    def key(self) -> tuple[str, str, frozenset[str]]:
        return (self.family, self.knob, self.missing)

    @property
    def ratio(self) -> str:
        if self.family == NO_FAMILY:
            return "read by nothing"
        return f"{len(self.honoured)}/{len(self.honoured) + len(self.missing)}"

    def describe(self, indent: str = "    ") -> list[str]:
        lines = [f"{self.family}  {self.knob}  [{self.ratio}]"]
        if self.family != NO_FAMILY:
            lines.append(f"{indent}honoured by : {', '.join(sorted(self.honoured))}")
            lines.append(f"{indent}NOT in      : {', '.join(sorted(self.missing))}")
        return lines


@dataclass(frozen=True)
class Entry:
    line_no: int
    family: str
    knob: str
    missing: frozenset[str]
    reason: str

    @property
    def key(self) -> tuple[str, str, frozenset[str]]:
        return (self.family, self.knob, self.missing)

    @property
    def text(self) -> str:
        not_in = NO_FAMILY if self.family == NO_FAMILY else ",".join(sorted(self.missing))
        return f"{self.family}  {self.knob}  {not_in}"


def load_audit():
    """Imports scripts/audit/find_inert_bounds.py without modifying it.

    The script computes `REPO` from its own `__file__`, which
    `spec_from_file_location` sets, so its globs resolve exactly as they do
    when it is run by hand.
    """
    spec = importlib.util.spec_from_file_location("find_inert_bounds", AUDIT)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot build an import spec for {AUDIT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    for needed in ("sources", "knob_names", "signature_a", "signature_c", "BOUND", "IMPL", "TRAIT_DECL"):
        if not hasattr(module, needed):
            raise ImportError(f"{AUDIT} no longer defines `{needed}`")
    return module


# ── The two signatures, mirrored from the audit script ───────────────────────
#
# Same regexes, same filters, same order. If these and the script's own loops
# ever disagree on a count, `main` exits 2 rather than trusting either.


def mention(knob: str) -> re.Pattern[str]:
    return re.compile(rf"(?:\.{knob}\b|\b{knob}\s*[:,=])")


def candidates_a(bodies: dict, knobs: set[str]) -> list[Candidate]:
    """Signature A: a knob whose every mention is a declaration, a default, a
    setter, or a test."""
    out = []
    for knob in sorted(knobs):
        seen = mention(knob)
        declaration = re.compile(rf"pub (?:const )?{knob}\s*:")
        initialiser = re.compile(rf"{knob}\s*:")
        setter = re.compile(rf"self\.\w*\.?{knob}\s*=(?![=>])")
        read = False
        for body in bodies.values():
            for line in body.splitlines():
                if not seen.search(line):
                    continue
                stripped = line.strip()
                if declaration.match(stripped) or initialiser.match(stripped) or setter.search(stripped):
                    continue
                read = True
                break
            if read:
                break
        if not read:
            out.append(Candidate(NO_FAMILY, knob, frozenset(), frozenset()))
    return out


def candidates_c(audit, bodies: dict, knobs: set[str]) -> list[Candidate]:
    """Signature C: a bound honoured by some members of a trait's
    implementations, not all."""
    families: dict[str, dict[str, set]] = {}
    for path, body in bodies.items():
        for m in audit.IMPL.finditer(body):
            families.setdefault(m.group(1), {}).setdefault(m.group(2), set()).add(path)

    own_traits: set[str] = set()
    for body in bodies.values():
        own_traits |= {m.group(1) for m in audit.TRAIT_DECL.finditer(body)}

    out = []
    for trait, impls in sorted(families.items()):
        if len(impls) < 2 or trait not in own_traits:
            continue
        for knob in sorted(knobs):
            if not audit.BOUND.search(knob):
                continue
            seen = mention(knob)
            have = {ty for ty, paths in impls.items() if any(seen.search(bodies[p]) for p in paths)}
            missing = set(impls) - have
            if not have or not missing:
                continue
            out.append(Candidate(trait, knob, frozenset(have), frozenset(missing)))
    return out


def audit_counts(audit, bodies: dict, knobs: set[str]) -> tuple[int, int]:
    """What the audit script itself counts, with its printing swallowed."""
    with contextlib.redirect_stdout(io.StringIO()):
        return audit.signature_a(bodies, knobs), audit.signature_c(bodies, knobs)


# ── The allowlist ────────────────────────────────────────────────────────────


def parse_allowlist(path: pathlib.Path) -> tuple[list[Entry], list[str]]:
    """`FAMILY  KNOB  NOT_IN  # reason`, one per line; see the file's header."""
    entries: list[Entry] = []
    problems: list[str] = []
    seen: dict[tuple, int] = {}
    for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        head, hash_sign, reason = line.partition("#")
        reason = reason.strip()
        fields = head.split()
        if len(fields) != 3 or not hash_sign or not reason:
            problems.append(
                f"{path}:{line_no}: expected `FAMILY  KNOB  NOT_IN  # reason` "
                f"(three columns, then a reason after `#`), got: {line}"
            )
            continue
        family, knob, not_in = fields
        if (family == NO_FAMILY) != (not_in == NO_FAMILY):
            problems.append(
                f"{path}:{line_no}: a signature-A entry (a field nothing reads) has "
                f"`{NO_FAMILY}` in both FAMILY and NOT_IN; a signature-C entry has neither: {line}"
            )
            continue
        missing = frozenset() if not_in == NO_FAMILY else frozenset(not_in.split(","))
        if any(not m for m in missing):
            problems.append(f"{path}:{line_no}: NOT_IN has an empty member (a stray comma?): {line}")
            continue
        entry = Entry(line_no, family, knob, missing, reason)
        if entry.key in seen:
            problems.append(
                f"{path}:{line_no}: duplicates line {seen[entry.key]} — two reasons for one "
                f"pair means nobody knows which is the real one"
            )
            continue
        seen[entry.key] = line_no
        entries.append(entry)
    return entries, problems


# ── Reporting ────────────────────────────────────────────────────────────────


def print_explain(candidates: list[Candidate], covering: dict[tuple, Entry]) -> None:
    print(f"{len(candidates)} candidate(s); the allowlist is {ALLOWLIST}\n")
    for cand in candidates:
        for line in cand.describe():
            print(line)
        entry = covering.get(cand.key)
        if entry is None:
            print("    allowlist   : NONE — this fails the gate")
        else:
            print(f"    allowlist   : line {entry.line_no} — {entry.reason}")
        print()


def report_uncovered(uncovered: list[Candidate]) -> None:
    err = sys.stderr
    print(
        f"{NAME}: {len(uncovered)} configurable bound(s) with no allowlist entry:\n",
        file=err,
    )
    for cand in uncovered:
        lines = cand.describe(indent="      ")
        print(f"  {lines[0]} — not allowlisted", file=err)
        for line in lines[1:]:
            print(line, file=err)
        print(file=err)
    if any(c.family != NO_FAMILY for c in uncovered):
        example = next(c for c in uncovered if c.family != NO_FAMILY)
        print(
            "A bound some of a family honour and the rest do not is a cap half the\n"
            "implementations ignore, which is not a cap. Either make every member honour\n"
            f"it, or explain why the ones that do not are right not to — one line in\n"
            f"{ALLOWLIST} naming the family, the knob, the members that do\n"
            "not honour it, and the reason:\n\n"
            f"  {example.family}  {example.knob}  {','.join(sorted(example.missing))}  # <why>\n\n"
            "A reason is something you verified by reading the code: which member honours\n"
            "it, and why the others legitimately do not (an in-memory capacity that a SQL\n"
            "store bounds through its schema; a sibling that honours it under another name;\n"
            "a sibling that delegates to a member that does). If a sibling SHOULD honour it\n"
            "and does not, begin the reason with `KNOWN-GAP:` so the entry reads as a debt\n"
            "and not as a verdict.",
            file=err,
        )
    if any(c.family == NO_FAMILY for c in uncovered):
        print(
            "\nA config field nothing reads is either dead — remove it — or a documented\n"
            "behaviour with no implementation, which is worse than none: the person who\n"
            "sets it holds a false belief. Allowlist one only with a reason that says why\n"
            "a field exists that nothing consults (`#[deprecated]` and kept for semver, say):\n\n"
            f"  {NO_FAMILY}  <knob>  {NO_FAMILY}  # <why>",
            file=err,
        )


def report_stale(stale: list[Entry], candidates: list[Candidate]) -> None:
    err = sys.stderr
    print(f"\n{NAME}: {len(stale)} allowlist entr{'y' if len(stale) == 1 else 'ies'} match no candidate:\n", file=err)
    for entry in stale:
        print(f"  {ALLOWLIST}:{entry.line_no}  {entry.text} — stale", file=err)
        # The likeliest cause is a family that changed shape; say what it is now.
        near = [c for c in candidates if c.family == entry.family and c.knob == entry.knob]
        for cand in near:
            print(f"      today that pair is: {cand.describe()[0]}", file=err)
            if cand.family != NO_FAMILY:
                print(f"      NOT in : {', '.join(sorted(cand.missing))}", file=err)
        if not near:
            print("      no candidate has that family and knob at all", file=err)
    print(
        "\nThe knob is now honoured everywhere, or was renamed, or a member joined or\n"
        "left the family. Remove the entry, or correct it to what the tree does now\n"
        "(`--explain` prints every candidate in allowlist form). An exemption for\n"
        "something that is not happening is how a skip list rots into a blanket\n"
        "waiver — deny.toml says so of its `skip` list, and prove_gates_fail.sh\n"
        "refuses a SKIP_STEPS pattern that names no step.",
        file=err,
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--explain",
        action="store_true",
        help="print every candidate with its family, honouring and non-honouring "
        "members, and the allowlist entry that covers it",
    )
    args = parser.parse_args()

    if not pathlib.Path("Cargo.toml").exists():
        print(f"{NAME}: run me from the repository root", file=sys.stderr)
        return 2
    for needed in (AUDIT, ALLOWLIST):
        if not needed.is_file():
            print(f"{NAME}: cannot read {needed}", file=sys.stderr)
            return 2

    try:
        audit = load_audit()
    except (ImportError, OSError, SyntaxError) as exc:
        print(f"{NAME}: cannot import {AUDIT}: {exc}", file=sys.stderr)
        return 2

    try:
        bodies = audit.sources()
    except SystemExit as exc:  # the script exits itself when it finds no sources
        print(f"{NAME}: {AUDIT} refused: {exc}", file=sys.stderr)
        return 2
    knobs = audit.knob_names(bodies)

    sig_a = candidates_a(bodies, knobs)
    sig_c = candidates_c(audit, bodies, knobs)
    want_a, want_c = audit_counts(audit, bodies, knobs)
    if (len(sig_a), len(sig_c)) != (want_a, want_c):
        print(
            f"{NAME}: this gate's copy of the signatures has drifted from {AUDIT}:\n"
            f"  signature A: gate {len(sig_a)}, audit script {want_a}\n"
            f"  signature C: gate {len(sig_c)}, audit script {want_c}\n"
            "Bring the mirrored loops in this file back in line with the script's "
            "before trusting either.",
            file=sys.stderr,
        )
        return 2
    candidates = sig_a + sig_c

    entries, problems = parse_allowlist(ALLOWLIST)
    if problems:
        print(f"{NAME}: {ALLOWLIST} is malformed:\n", file=sys.stderr)
        for problem in problems:
            print(f"  {problem}", file=sys.stderr)
        return 2

    by_key = {entry.key: entry for entry in entries}
    covering = {cand.key: by_key[cand.key] for cand in candidates if cand.key in by_key}
    uncovered = [cand for cand in candidates if cand.key not in by_key]
    used = {entry.key for entry in covering.values()}
    stale = [entry for entry in entries if entry.key not in used]

    if args.explain:
        print_explain(candidates, covering)

    if uncovered:
        report_uncovered(uncovered)
    if stale:
        report_stale(stale, candidates)
    if uncovered or stale:
        return 1

    known_gaps = sum(1 for entry in entries if entry.reason.startswith("KNOWN-GAP:"))
    print(
        f"{NAME}: {len(sig_a)} signature-A and {len(sig_c)} signature-C candidate(s), "
        f"each allowlisted with a reason ({len(entries)} entries, none stale, "
        f"{known_gaps} marked KNOWN-GAP)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
