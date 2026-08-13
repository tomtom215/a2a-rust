#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Asserts the per-PR mutation gate can actually see every library source file.
#
# On 2026-08-12 it could not. `mutants.yml`'s `Build PR source diff` step
# scoped with `git diff -M ... -- 'crates/*/src/**/*.rs'`, and a git pathspec is
# matched by fnmatch WITHOUT FNM_PATHNAME unless it carries `:(glob)` magic — so
# `*` crosses `/` freely and the `**/` in the middle degenerates into "one or
# more directories", because the literal slash after it still has to match
# something. The pattern therefore reached only files in a *subdirectory* of
# `src/` and was blind to all 41 files sitting directly in
# `crates/<crate>/src/`: rate_limit.rs, serve.rs, builder.rs, executor.rs,
# method.rs, signing.rs, client.rs, retry.rs and all four lib.rs among them.
#
# The consequence was not a gate that failed to fail. It was worse: when the
# scoped diff came back empty the job set `skip=true`, its own `if:` skipped the
# mutation step, the check went green, and the summary announced "No Rust source
# files changed in `crates/*/src/` — nothing to mutate", which was untrue. Nine
# of the 120 commits before 6ebf821 changed sources *only* in the invisible set;
# one of them (e6aa9e1) carried 376 added lines that were never mutated.
#
# Why this is a separate gate rather than a probe in
# scripts/prove_workflow_gates_fail.py: that harness proves a step can fail on
# bad *input*. This step's defect was not in its input. It read its input
# correctly and asked for the wrong files, and it reported success while doing
# so. "Can this gate fail?" and "is this gate pointed at everything it claims to
# cover?" are different questions, and only the second one catches a pathspec.
#
# The pathspec is READ FROM the workflow rather than restated here, for the
# reason scripts/preflight.sh reads its gate list from ci.yml: a copy in this
# file could drift from the real one, and then this check would be verifying a
# string nothing runs.
#
# Exit codes: 0 the pathspec reaches exactly the intended set, 1 it does not,
# 2 the pathspec could not be located (the step was renamed or restructured).

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

WORKFLOW=.github/workflows/mutants.yml

# The scoping line, verbatim from the workflow:
#     git diff -M "${BASE}...HEAD" -- '<pathspec>' > pr-src.diff
PATHSPEC=$(sed -n "s/^[[:space:]]*git diff -M .* -- '\(.*\)' > pr-src\.diff[[:space:]]*$/\1/p" "$WORKFLOW")

if [ -z "$PATHSPEC" ]; then
    cat >&2 <<EOF
check_mutation_scope: could not find the mutation scoping pathspec.

  Expected a line in $WORKFLOW of the form:
      git diff -M "\${BASE}...HEAD" -- '<pathspec>' > pr-src.diff

  The step was renamed or restructured. Point this check at the new line
  rather than deleting it — refusing to run is better than reporting that a
  scope nothing reads is correct.
EOF
    exit 2
fi

if [ "$(printf '%s\n' "$PATHSPEC" | wc -l)" -ne 1 ]; then
    printf 'check_mutation_scope: expected exactly one scoping pathspec, found %s\n' \
        "$(printf '%s\n' "$PATHSPEC" | wc -l)" >&2
    exit 2
fi

# What the gate is *meant* to cover: every tracked Rust source under any
# crate's src/. Spelled with an explicit anchored regex rather than another
# pathspec, so this side of the comparison cannot inherit the same bug.
INTENDED=$(git ls-files -- crates | grep -E '^crates/[^/]+/src/.+\.rs$' | sort)

# What it actually covers.
ACTUAL=$(git ls-files -- "$PATHSPEC" | sort)

MISSING=$(comm -23 <(printf '%s\n' "$INTENDED") <(printf '%s\n' "$ACTUAL"))
EXTRA=$(comm -13 <(printf '%s\n' "$INTENDED") <(printf '%s\n' "$ACTUAL"))

n_intended=$(printf '%s\n' "$INTENDED" | grep -c . || true)
n_actual=$(printf '%s\n' "$ACTUAL" | grep -c . || true)
n_missing=$(printf '%s\n' "$MISSING" | grep -c . || true)
n_extra=$(printf '%s\n' "$EXTRA" | grep -c . || true)

status=0

if [ "$n_missing" -gt 0 ]; then
    cat >&2 <<EOF
check_mutation_scope: MUTATION SCOPE GAP — the per-PR gate cannot see $n_missing file(s).

  pathspec in $WORKFLOW:
      $PATHSPEC

  It matches $n_actual of $n_intended tracked sources under crates/*/src/.
  A pull request changing only the files below produces an empty diff, so the
  gate sets skip=true, mutates nothing, and reports success.

$(printf '%s\n' "$MISSING" | sed 's/^/      /')

  If the pathspec uses \`**\`, it almost certainly needs the \`:(glob)\` prefix —
  without it git matches with fnmatch and \`*\` crosses \`/\`. See the header of
  this script for the 2026-08-12 occurrence.
EOF
    status=1
fi

if [ "$n_extra" -gt 0 ]; then
    cat >&2 <<EOF
check_mutation_scope: the pathspec reaches $n_extra file(s) outside crates/*/src/.

  Over-matching is a defect too: it pulls unrelated files into --in-diff and
  can balloon a PR run past its timeout, which fails the job for a reason that
  has nothing to do with the code under review.

$(printf '%s\n' "$EXTRA" | sed 's/^/      /')
EOF
    status=1
fi

if [ "$status" -eq 0 ]; then
    printf 'check_mutation_scope: %s of %s tracked sources under crates/*/src/ are reachable by the PR gate\n' \
        "$n_actual" "$n_intended"
fi

exit "$status"
