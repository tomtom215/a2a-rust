#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Enforces CONTRIBUTING.md's 500-line rule as a ratchet.
#
# The rule as written is: "No **new** file exceeds 500 lines, and no file you
# touched crosses it." Nothing enforced it. It sat in a PR checklist, which
# means it held exactly as often as a reviewer counted lines by hand — and the
# count in that same checklist ("46 of 139 existing sources") had drifted to
# 77 of 310 without anyone noticing, because a number nothing recomputes is a
# number that decays.
#
# This is a ratchet, not a cliff. Existing long files are recorded in
# `.file-length-baseline` and stay legal: CONTRIBUTING is explicit that some
# files exceed the guideline where splitting would harm cohesion, and that the
# rule is "for new work, not a claim about the tree". What the ratchet forbids
# is the list getting longer.
#
# Growth of an already-listed file is deliberately allowed. The documented rule
# is about *crossing* 500, not about staying still; blocking growth would make
# adding a test to a long file a CI failure and teach people to route around
# the check.
#
# A listed file that drops to 500 or fewer lines is a failure, for the same
# reason a passing `--skip`ped conformance test is: an entry that no longer
# describes anything reads as a live exemption while exempting nothing. The
# list has to shrink to match reality.
#
# Usage:
#   scripts/check_file_lengths.sh            # check
#   scripts/check_file_lengths.sh --update   # rewrite the baseline
#
# Exit codes: 0 clean, 1 a file crossed the limit or an entry went stale.

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

LIMIT=500
BASELINE=".file-length-baseline"

# Only files this repository authors. Generated output (target/), vendored
# JS dependencies, and anything git does not track are not ours to shorten.
current_over_limit() {
    git ls-files '*.rs' \
        | grep -v '^itk/agents/.*/node_modules/' \
        | while IFS= read -r f; do
            [ -f "$f" ] || continue
            n=$(wc -l <"$f")
            if [ "$n" -gt "$LIMIT" ]; then
                printf '%s\n' "$f"
            fi
        done | sort
}

total_tracked() {
    git ls-files '*.rs' | grep -cv '^itk/agents/.*/node_modules/'
}

if [ "${1-}" = "--update" ]; then
    {
        printf '# Files over %d lines, recorded so the list can only shrink.\n' "$LIMIT"
        printf '# Regenerate with scripts/check_file_lengths.sh --update.\n'
        printf '# See CONTRIBUTING.md "500-line maximum per file".\n'
        current_over_limit
    } >"$BASELINE"
    printf 'wrote %s (%d entries of %d tracked .rs files)\n' \
        "$BASELINE" "$(current_over_limit | wc -l)" "$(total_tracked)"
    exit 0
fi

if [ ! -f "$BASELINE" ]; then
    printf 'check_file_lengths: %s is missing. Create it with --update.\n' "$BASELINE" >&2
    exit 1
fi

recorded=$(grep -v '^#' "$BASELINE" | grep -v '^[[:space:]]*$' | sort)
actual=$(current_over_limit)

# `comm` needs both sides sorted; both are.
crossed=$(comm -13 <(printf '%s\n' "$recorded") <(printf '%s\n' "$actual"))
stale=$(comm -23 <(printf '%s\n' "$recorded") <(printf '%s\n' "$actual"))

status=0

if [ -n "$crossed" ]; then
    status=1
    printf '\nOver the %d-line limit and not recorded as pre-existing:\n\n' "$LIMIT" >&2
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        printf '    %6s  %s\n' "$(wc -l <"$f")" "$f" >&2
    done <<<"$crossed"
    cat >&2 <<EOF

Split it into focused sub-modules with a thin mod.rs, as CONTRIBUTING.md
describes. If splitting would genuinely harm cohesion, say so in the PR and
run scripts/check_file_lengths.sh --update to record the exemption — which
makes it a visible decision instead of a silent one.
EOF
fi

if [ -n "$stale" ]; then
    status=1
    printf '\nRecorded as over the limit but no longer are:\n\n' >&2
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        if [ -f "$f" ]; then
            printf '    %6s  %s\n' "$(wc -l <"$f")" "$f" >&2
        else
            printf '    %6s  %s\n' "gone" "$f" >&2
        fi
    done <<<"$stale"
    cat >&2 <<EOF

Good news, and the list has to shrink to match or it stops meaning anything —
the same reason a passing skipped test fails the TCK runner. Run
scripts/check_file_lengths.sh --update.
EOF
fi

if [ "$status" -eq 0 ]; then
    printf 'check_file_lengths: %d of %d tracked .rs files exceed %d lines, all recorded\n' \
        "$(printf '%s\n' "$recorded" | grep -c .)" "$(total_tracked)" "$LIMIT"
fi

exit "$status"
