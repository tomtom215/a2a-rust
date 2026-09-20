#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Asserts the CI gate inventory is complete.
#
# Every other gate in this repository answers "is this thing right?".  This one
# answers the question that decides what those answers are worth: "is that all
# of them?"  A gate inventory that is missing an entry does not report a gap —
# it reports a smaller total, in green, and `prove_gates_fail.sh` prints "N of
# N proven" over a set nobody has confirmed is the whole set.
#
# Four assertions, all defined in scripts/lib/ci_gate_audit.sh so that
# preflight.sh and prove_gates_fail.sh run the same ones:
#
#   require_known_jobs          every job in ci.yml is classified as a gate job
#                               or an explicit non-gate.
#   require_nonempty_gate_jobs  every job classified as a gate actually yields
#                               a gate — the classification is true, not just
#                               present.
#   require_known_skips         every SKIP_STEPS exemption still names a step
#                               ci.yml has.
#   require_registered_actions  every `uses:` step is either infrastructure or
#                               a named exemption with a reason, and every
#                               exemption still names a real step.
#
# The first two were defined in preflight.sh and nowhere else, and preflight.sh
# runs in no workflow — `grep -rn preflight .github/workflows/` finds only
# comments.  So the completeness check over ci.yml's job list ran exclusively
# on whichever laptop chose to run it.  This script is the call site that puts
# it in CI, which is the only place it can catch a job somebody adds.
#
# The fourth is new, and it closes the hole that made this script worth
# writing: `gates_for_jobs` emits one gate per `run:` step, so a `uses:` step
# contributes zero gates and the unregistered-gate guard in
# `prove_gates_fail.sh` sees nothing to complain about.  `cargo-deny (the
# binding's own dependency tree)` is such a step.  It is the only thing
# auditing the SLIMRPC binding's 379 transitive dependencies — `aws-lc-sys`, a
# native C crypto build, among them — and no inventory in this repository knew
# it existed.
#
# Usage:
#   scripts/check_gate_inventory.sh
#
# Exit codes: 0 the inventory is complete; 2 drift — an unclassified job, a
# gate job that yields no gate, or an exemption that names nothing.  There is
# no exit 1: every finding here is a configuration error rather than a
# judgement about the code.

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"
CI_YML="$REPO_ROOT/.github/workflows/ci.yml"

if [ ! -f "$CI_YML" ]; then
    printf 'check_gate_inventory: %s is missing\n' "$CI_YML" >&2
    exit 2
fi

# shellcheck source=lib/ci_gates.sh
. "$REPO_ROOT/scripts/lib/ci_gates.sh"

require_known_jobs
require_nonempty_gate_jobs
require_known_skips
require_registered_actions

jobs=$(awk '
    /^jobs:[[:space:]]*$/ { in_jobs = 1; next }
    /^[^[:space:]#]/      { in_jobs = 0 }
    in_jobs && /^  [a-z][a-z0-9_-]*:[[:space:]]*$/ { n++ }
    END { print n + 0 }
' "$CI_YML")
gates=$(gates_for_jobs "$GATE_JOBS" | grep -c . || true)

# A count, printed on success, for the reason every other gate here prints one:
# a green with no number cannot be told from a green that measured nothing.
printf 'check_gate_inventory: %s job(s) in ci.yml, all classified; %s gate(s) extracted from %s\n' \
    "$jobs" "$gates" "$(printf '%s' "$GATE_JOBS" | tr -d '^$()' | tr '|' ' ' | sed 's/  */, /g')"

if [ "$gates" -eq 0 ]; then
    printf 'check_gate_inventory: zero gates extracted — the parser is broken.\n' >&2
    exit 2
fi
