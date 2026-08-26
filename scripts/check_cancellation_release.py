#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
"""Asserts that a claimed guard flag is released by `Drop`, not by falling off
the end of a function.

Why this exists
---------------
On 2026-08-19 three shipped defects in two crates turned out to be one shape:
**state claimed on one code path and released on another, with an `.await` in
between.** Dropping the future runs neither path, so the state is claimed
forever.

The worst of them, `InMemoryTaskStore::maybe_evict`, is the pattern this script
recognises exactly:

    if self.flag.compare_exchange(false, true, ...).is_err() { return; }
    let mut store = self.data.write().await;    // <-- cancelled here
    Self::evict(&mut store, ...);
    self.flag.store(false, ...);                // <-- never runs

Measured: one cancelled sweep left the flag set for the life of the process,
and both of that store's memory bounds ran through it, so the default store had
no memory ceiling from the first cancellation onward.

`clippy::await_holding_lock` and friends cover the inverse case — holding a
guard *across* an await. Nothing covers releasing a flag *after* one.

What it checks
--------------
A file that **claims** a guard slot — `compare_exchange(false, true, ..)` — must
also contain an `impl Drop` that **releases** one — `store(false, ..)`.

File-scoped, not name-scoped, and that is the interesting part. The first
version of this script matched the claim's field name against the release's, and
it flagged the *correct* code: the whole point of a guard is that it holds a
borrowed reference, so `EvictionSlot::claim(flag)` claims through `flag` and
`EvictionSlot::drop` releases through `self.0`. A check that fails on the
canonical fix is worse than no check, so the invariant is the weaker,
true one — this shape and its release live together.

Deliberately narrow. A heuristic over every `insert`/`remove` pair would cry
wolf on every cache in the workspace, and a check people learn to override is
not a check — the same argument `check_file_lengths.sh` makes for its ratchet.
This one fires on a shape that has exactly one correct form.

What it is not
--------------
Two approximations, stated because a checker that reads as exact and is not is
the failure this whole exercise is about:

* **A file with no `.await` anywhere is skipped.** A `compare_exchange` guard
  released at the end of a synchronous function is perfectly correct — there is
  no cancellation point for it to be interrupted at — and flagging it would be
  a false alarm on right code. The cost is a false negative if a synchronous
  module later grows its first `.await` *and* a claim in the same edit.
* **It does not know which flag a `Drop` releases.** A file holding a claim on
  one flag and an unrelated `Drop` that releases a second one passes. Matching
  them by name is exactly what the first version did, and it flagged the
  canonical fix, because a guard releases through the reference it borrowed
  (`self.0`) and not through the name the claim used (`flag`).

Both are deliberate: this check is a tripwire for one specific mistake that has
been made three times here, not a static analyser.

Exit codes: 0 every claiming file releases from `Drop`, 1 one does not,
2 not run from the repository root.
"""

from __future__ import annotations

import pathlib
import re
import sys

# `compare_exchange(false, true, ..)` — claiming a guard slot.
#
# The `\s*` between every token is load-bearing, not decoration: rustfmt splits
# the four arguments one-per-line as soon as they do not fit, which is what the
# real one does. (`re.DOTALL` would add nothing — there is no `.` here, and
# `\s` matches a newline either way.)
CLAIM = re.compile(r"\.\s*compare_exchange\s*\(\s*false\s*,\s*true\b")
# `store(false, ..)` — releasing one.
RELEASE = re.compile(r"\.\s*store\s*\(\s*false\b")

# `impl Drop for X {` … matched by brace depth from the opening brace.
IMPL_DROP = re.compile(r"\bimpl\s+Drop\s+for\s+[^{]+\{")

# An inline `#[cfg(test)] mod … { … }`. Only the *inline* form: `#[cfg(test)]
# mod fixtures;` declares a separate file and must not truncate anything, which
# the first version of this script did — it cut at the first `#[cfg(test)]` it
# saw and so read only the first 29 lines of the very file this check exists
# for, reporting zero claims and passing.
CFG_TEST_MOD = re.compile(r"#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{")


def _matching_brace(src: str, open_at: int) -> int:
    """Index one past the `}` closing the `{` that ends at `open_at`."""
    depth, i = 1, open_at
    while i < len(src) and depth:
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
        i += 1
    return i


def drop_body_spans(src: str) -> list[tuple[int, int]]:
    """Character ranges of every `impl Drop for … { … }` body."""
    return [(m.end(), _matching_brace(src, m.end())) for m in IMPL_DROP.finditer(src)]


def without_test_modules(src: str) -> str:
    """`src` with every inline `#[cfg(test)] mod … { … }` blanked out.

    Blanked rather than deleted so byte offsets — and therefore reported line
    numbers — still point at the real file.
    """
    out = src
    for m in CFG_TEST_MOD.finditer(src):
        end = _matching_brace(src, m.end())
        span = out[m.start() : end]
        out = out[: m.start()] + re.sub(r"[^\n]", " ", span) + out[end:]
    return out


def main() -> int:
    root = pathlib.Path(".")
    if not (root / "Cargo.toml").exists():
        print("check_cancellation_release: run me from the repository root", file=sys.stderr)
        return 2

    sources = sorted(
        p
        for p in root.glob("crates/*/src/**/*.rs")
        if "/tests/" not in p.as_posix()
    ) + sorted(
        p
        for p in root.glob("bindings/*/src/**/*.rs")
        if "/tests/" not in p.as_posix()
    )

    offenders: list[tuple[pathlib.Path, int]] = []
    claim_count = 0

    for path in sources:
        # A `#[cfg(test)]` module may legitimately poke at a flag by hand.
        body = without_test_modules(path.read_text(encoding="utf-8"))

        claims = list(CLAIM.finditer(body))
        if not claims:
            continue
        claim_count += len(claims)

        # No cancellation point, no cancellation bug. See "What it is not".
        if ".await" not in body:
            continue

        spans = drop_body_spans(body)
        releases_from_drop = any(
            any(a <= m.start() < b for a, b in spans) for m in RELEASE.finditer(body)
        )
        if not releases_from_drop:
            line = body.count("\n", 0, claims[0].start()) + 1
            offenders.append((path, line))

    if offenders:
        print(
            "check_cancellation_release: a guard slot is claimed in a file with "
            "no `Drop` that releases it:\n",
            file=sys.stderr,
        )
        for path, line in offenders:
            print(f"  {path}:{line}", file=sys.stderr)
        print(
            "\nA `store(false, ..)` at the end of the function does not run when "
            "the future\nis dropped, and anything awaiting between the claim and "
            "that line is a place\nthe future can be dropped. Release it from a "
            "`Drop` impl instead: see\n"
            "`EvictionSlot` in "
            "crates/a2a-protocol-server/src/store/task_store/in_memory/eviction/mod.rs.\n"
            "\nIf the release genuinely lives in another file, move the guard type "
            "next to\nthe claim — that is where a reader looks for it.",
            file=sys.stderr,
        )
        return 1

    print(
        f"check_cancellation_release: {claim_count} claimed guard slot(s), "
        "each in a file that releases from `Drop`."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
