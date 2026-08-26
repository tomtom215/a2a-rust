#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Runs this repository's CI quality gates locally, before you push.
#
# Why this exists as a script rather than a list in CONTRIBUTING.md: a list of
# commands in prose gets partially run. In August 2026 two commits landed with
# unformatted test modules because `cargo clippy` and `cargo test` were run by
# hand and `cargo fmt --all -- --check` was not, leaving CI's Format job red
# across two pushes. One command that runs every gate and reports each one
# removes the chance to skip a step by accident.
#
# The gate list is READ FROM .github/workflows/ci.yml rather than restated
# here, so a gate added to CI cannot silently go unrun locally. The tiers below
# name specific commands, and the script refuses to run if a named command is
# no longer present in the workflow — drift is a hard error, not a surprise in
# CI three pushes later.
#
# Usage:
#   scripts/preflight.sh              # default tier: fmt + clippy + tests
#   scripts/preflight.sh --fmt        # formatting only (seconds; used by the hook)
#   scripts/preflight.sh --full       # every gate in the fmt/clippy/test/doc jobs
#   scripts/preflight.sh --list       # show the CI gate inventory and exit
#   scripts/preflight.sh --fail-fast  # stop at the first failing gate
#   scripts/preflight.sh --force      # skip the free-space precheck
#
# What `--full` needs that a laptop may not have, all measured 2026-08-19 and
# all reported by the precheck before any gate runs:
#
#   ~17 GB of target/ space    16.6 GB measured across both target dirs; up to
#                              29 GB if the tree also holds non-CI artifacts.
#                              Warns below 20 GB free, refuses below 8.
#   PostgreSQL on :5432        4 gates set A2A_TEST_POSTGRES_URL, per ci.yml
#   SPIRE binaries             1 gate runs the binding's SPIFFE suites
#
# The two services are *not* optional in the sense of being skipped: those
# gates fail without them, exactly as they should — `tests/common/spire.rs`
# panics rather than passing quietly, and it is right to. What was missing is
# any warning before ninety minutes of compiling, after which the summary said
# "Do not push" to somebody whose only problem was an uninstalled service.
#
# Every gate runs even after one fails, so a single run tells you everything
# that is broken rather than only the first thing.
#
# Note on the first run: this exports CI's environment (see apply_ci_env), and
# RUSTFLAGS/CARGO_PROFILE_DEV_DEBUG are part of cargo's fingerprint. If you have
# been building without them, the first preflight rebuilds the workspace and
# keeps a second set of artifacts in target/. That is the cost of testing what
# CI tests; `cargo clean` if disk is tight.
#
# "If disk is tight" was the whole of that warning until 2026-08-19, when a
# --full run on a 30 GB allowance died with `No space left on device` 19 gates
# in, having grown target/ to 29 GB over a tree already built without CI's
# flags. Learning a number from an ENOSPC inside an unrelated cargo invocation
# forty minutes in is expensive, so the run measures first.

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
CI_YML="$REPO_ROOT/.github/workflows/ci.yml"

# The ci.yml parser, the job/step classification lists, and the guards that
# keep them honest. Shared with scripts/prove_gates_fail.sh rather than copied
# into it: the two carried byte-identical parsers kept in step by a comment
# until 2026-08-18, and a convention that has to be remembered is not a guard.
# shellcheck source=lib/ci_gates.sh
. "$REPO_ROOT/scripts/lib/ci_gates.sh"
LOG_DIR=$(mktemp -d "${TMPDIR:-/tmp}/a2a-preflight.XXXXXX")

TIER=default
FAIL_FAST=0
FORCE=0

# ── CI workflow parsing ──────────────────────────────────────────────────────

# Prints every single-line `run:` step belonging to a job whose name matches
# the given extended regex.
#
# This used to match `run: cargo ` only. That was a silent under-cover waiting
# to happen, and it happened the first time a gate job grew a step that was not
# a cargo invocation (`./scripts/check_proto_copies.sh` in `fmt`): CI enforced
# it, preflight did not list it, and nothing anywhere said so — the same defect
# as the two missing jobs, one level down. A gate is whatever CI runs in a gate
# job, not whatever CI runs that happens to start with `cargo`.
#
# Steps whose `run:` is a block scalar are still invisible here; that is what
# `note_skipped_steps` reports.
#
# Each command is prefixed with the environment CI gives it — the job's `env:`
# block and then the step's, so a step-level value wins. Until 2026-08-11 only
# the *top-level* `env:` was applied (`apply_ci_env` below), and the gap was not
# theoretical: `example-surface` sets `INCIDENT_EXIT_WHEN_DONE` on the
# incident-response demo step, and without it the demo parks on Ctrl+C — so
# `--full` hung forever rather than running the gate. Two more steps set
# `A2A_TEST_POSTGRES_URL` and `INCIDENT_REQUIRE_ALL`, and without those the
# local run exercised strictly less than CI while reporting PASS. A local gate
# that does not reproduce the CI gate is worse than no local gate.

# Block-scalar steps (`run: |`) are invisible to the parser above. If one shows
# up in a gate job, say so rather than quietly under-covering.
note_skipped_steps() {
    if [ -z "$SKIP_STEPS" ]; then return; fi
    printf 'preflight: note — step(s) exempted from local running: %s\n' \
        "$(printf '%s' "$SKIP_STEPS" | tr -d '^$()' | tr '|' ',')" >&2
    printf '           see SKIP_STEPS in this script for why; CI still runs them.\n' >&2
}




# Every SKIP_STEPS alternative must still name a step that exists. An exemption
# for a step that has been renamed or deleted is an exemption that silently
# covers nothing, which is the same defect as an unlisted gate — this file has
# now had that shape three times (test-postgres and package unknown to
# preflight, slimrpc-binding unknown to both, deny and semver listed but empty).
# A folded block scalar (`run: >`) joins its lines with spaces; a literal one
# (`run: |`) keeps the newlines. `gates_for_jobs` treats both alike, so for `>`
# it would emit a *different command than CI runs*: YAML reads
# `run: >` / `echo one` / `two` as `echo one two`, and this parser would make
# `two` a command of its own. ci.yml uses only `|` today, so this refuses
# rather than implementing a fold nobody needs. A gate that runs the wrong
# command is worse than one that is missing, and refusing means a folded block
# cannot be introduced without someone deciding what it should mean.


# Bidirectional drift guard.
#
# `require_ci_gate` catches one direction: a tier naming a command CI no longer
# runs. It cannot catch the other, and that is how two real gates went
# uncovered for as long as they did — `test-postgres` and `package` were simply
# jobs the script had never been told about, so nothing anywhere noticed they
# were missing. A guard that only fails on staleness is half a guard.
#
# This asserts the script knows about every job in ci.yml. A new job is either
# a gate or an explicit exemption; it cannot be neither, and it cannot be
# silence.
require_known_jobs() {
    local unknown
    # Only names under the top-level `jobs:` key. Without that anchor this also
    # collects `push:` and `pull_request:` from the `on:` block, which are
    # triggers, not jobs.
    unknown=$(awk '
        /^jobs:[[:space:]]*$/ { in_jobs = 1; next }
        /^[^[:space:]#]/      { in_jobs = 0 }
        in_jobs && /^  [a-z][a-z0-9_-]*:[[:space:]]*$/ {
            job = $1; sub(/:$/, "", job); print job
        }
    # `|| true`: the success case is grep matching nothing, which exits 1 and
    # would take the script down under `set -e`. An empty result is the good
    # outcome here, not a failure.
    ' "$CI_YML" | grep -Ev "$GATE_JOBS" | grep -Ev "$NON_GATE_JOBS" | sort -u || true)
    if [ -n "$unknown" ]; then
        cat >&2 <<EOF
preflight: unknown CI job(s) — gate coverage cannot be trusted.

  ci.yml defines job(s) this script has never been told about:
$(printf '      %s\n' $unknown)

  Add each to GATE_JOBS (so preflight runs it) or to NON_GATE_JOBS with a
  reason (so the exemption is visible). Refusing to run rather than report a
  green that silently skips a gate.
EOF
        exit 2
    fi
}

# The other half of `require_known_jobs`. That one asserts every ci.yml job is
# *classified*; this asserts the classification is true — that a job filed under
# GATE_JOBS actually yields a gate to run. `deny` and `semver` sat in GATE_JOBS
# and yielded none, because `gates_for_jobs` reads `run:` steps and both jobs
# are pure `uses:`. Listing a job you cannot run is the same defect as not
# listing it, minus the error message, and it is the more dangerous of the two:
# `require_known_jobs` prints a refusal, this one printed a green.
require_nonempty_gate_jobs() {
    local job empty=""
    for job in $(printf '%s' "$GATE_JOBS" | tr -d '^$()' | tr '|' ' '); do
        if [ -z "$(gates_for_jobs "^${job}\$")" ]; then
            empty="$empty $job"
        fi
    done
    if [ -n "$empty" ]; then
        cat >&2 <<EOF
preflight: GATE_JOBS names job(s) that contribute no gate.

  Listed as gates, but no runnable step was extracted from them:
$(printf '      %s\n' $empty)

  A job whose steps are all \`uses:\` (a marketplace action) has no \`run:\`
  line to copy, so it is filed as a gate and then silently skipped. Move it to
  NON_GATE_JOBS with a reason, or teach the parser to reach its steps.
EOF
        exit 2
    fi
}

require_known_jobs
require_nonempty_gate_jobs
require_known_skips
ALL_GATES=$(gates_for_jobs "$GATE_JOBS" | sed $'s/\t//')

# Same reasoning as the gate list: copy CI's environment rather than restate it.
# Matching the commands but not the environment is not parity — CI sets
# RUSTFLAGS=-D warnings (so a warning CI denies would pass locally) and
# CARGO_PROFILE_DEV_DEBUG=0 (without which the all-features link can die with
# SIGBUS on a constrained machine, which reads as a test failure but is not).
# Reads the top-level `env:` block; per-job `env:` blocks are indented deeper
# and deliberately not picked up.
apply_ci_env() {
    local line key value
    while IFS= read -r line; do
        key=${line%%=*}
        value=${line#*=}
        # Never clobber something the caller set deliberately.
        if [ -z "${!key-}" ]; then
            export "$key=$value"
        fi
    done < <(awk '
        /^env:[[:space:]]*$/ { in_env = 1; next }
        /^[^[:space:]#]/     { in_env = 0 }
        in_env && /^  [A-Za-z_][A-Za-z0-9_]*:/ {
            line = $0
            sub(/^  /, "", line)
            key = line; sub(/:.*$/, "", key)
            val = line; sub(/^[^:]*:[[:space:]]*/, "", val)
            gsub(/^"|"$/, "", val)
            print key "=" val
        }
    ' "$CI_YML")
}

# Fails if a tier names a command CI no longer runs.
require_ci_gate() {
    if ! printf '%s\n' "$ALL_GATES" | grep -Fxq -- "$1"; then
        cat >&2 <<EOF
preflight: gate drift detected.

  This script's tier definitions name a command that ci.yml no longer runs:
      $1

  Either CI changed and the tiers in $0 need updating, or the parser above
  stopped matching. Refusing to run rather than report a green that means less
  than it used to.
EOF
        exit 2
    fi
    printf '%s\n' "$1"
}

# ── Gate execution ───────────────────────────────────────────────────────────

RESULTS=()
FAILED=0

run_gate() {
    local cmd="$1"
    local log="$LOG_DIR/gate-${#RESULTS[@]}.log"
    local start=$SECONDS status

    printf '\n\033[1m▶ %s\033[0m\n' "$cmd"
    # Exit status is captured directly. Never pipe a gate through tee/head —
    # the pipeline's status is the last command's, which is how a failing gate
    # reports success.
    if (cd "$REPO_ROOT" && eval "$cmd") >"$log" 2>&1; then
        status=0
    else
        status=$?
    fi
    local elapsed=$((SECONDS - start))

    if [ "$status" -eq 0 ]; then
        printf '  \033[32mPASS\033[0m  (%ss)\n' "$elapsed"
        RESULTS+=("PASS|${elapsed}s|$cmd")
    else
        printf '  \033[31mFAIL\033[0m  (%ss, exit %s)\n' "$elapsed" "$status"
        printf '  ── output ──\n'
        sed 's/^/  /' "$log" | tail -40
        printf '  ── full log: %s ──\n' "$log"
        RESULTS+=("FAIL|${elapsed}s|$cmd")
        FAILED=$((FAILED + 1))
        if [ "$FAIL_FAST" -eq 1 ]; then
            summarise
            exit 1
        fi
    fi
}

summarise() {
    printf '\n\033[1m── preflight summary (%s tier) ──\033[0m\n' "$TIER"
    local row verdict timing cmd
    for row in "${RESULTS[@]-}"; do
        [ -n "$row" ] || continue
        verdict=${row%%|*}
        timing=${row#*|}; timing=${timing%%|*}
        cmd=${row#*|*|}
        if [ "$verdict" = PASS ]; then
            printf '  \033[32m%-4s\033[0m %6s  %s\n' "$verdict" "$timing" "$cmd"
        else
            printf '  \033[31m%-4s\033[0m %6s  %s\n' "$verdict" "$timing" "$cmd"
        fi
    done

    local total covered uncovered
    total=$(printf '%s\n' "$ALL_GATES" | grep -c . || true)
    covered=${#RESULTS[@]}
    uncovered=$((total - covered))
    printf '\n  %s of %s CI gate commands run locally' "$covered" "$total"
    if [ "$uncovered" -gt 0 ]; then
        printf ' — %s still only checked in CI (see --full / --list)' "$uncovered"
    fi
    printf '\n'

    # A run that executed nothing has not passed, it has not run. Reporting
    # green over an empty denominator is how a gate ends up unable to fail.
    if [ "$covered" -eq 0 ]; then
        printf '\n\033[31mNo gates ran — refusing to report a pass.\033[0m\n'
        return 1
    fi
    if [ "$FAILED" -gt 0 ]; then
        printf '\n\033[31m%s gate(s) failed. Do not push.\033[0m\n' "$FAILED"
        return 1
    fi
    printf '\n\033[32mAll gates passed.\033[0m\n'
    return 0
}

# ── Tiers ────────────────────────────────────────────────────────────────────

# Named explicitly, and each verified to still exist in ci.yml. The default
# tier is the subset worth paying for on every push: it is what catches the
# mistakes that have actually reached this repository's CI.
tier_fmt() {
    require_ci_gate 'cargo fmt --all -- --check'
}

tier_default() {
    tier_fmt
    require_ci_gate 'cargo clippy --workspace --all-targets -- -D warnings'
    require_ci_gate 'cargo clippy --workspace --all-targets --all-features -- -D warnings'
    require_ci_gate 'cargo test --workspace --all-features'
}

tier_full() {
    printf '%s\n' "$ALL_GATES"
}

# ── Prechecks ────────────────────────────────────────────────────────────────
#
# `check_free_disk` and `report_external_prerequisites`, in their own file for
# the same reason ci_gates.sh is: this one crossed 500 lines when they were
# added, and the ratchet's answer for a script is helpers it sources.
# shellcheck source=lib/preflight_prechecks.sh
. "$REPO_ROOT/scripts/lib/preflight_prechecks.sh"

# ── Entry point ──────────────────────────────────────────────────────────────

while [ $# -gt 0 ]; do
    case "$1" in
        --fmt)       TIER=fmt ;;
        --full)      TIER=full ;;
        --fail-fast) FAIL_FAST=1 ;;
        --force)     FORCE=1 ;;
        --list)
            note_skipped_steps
            # Derived from GATE_JOBS, not restated. The literal that stood
            # here read "fmt, clippy, test, doc" and had not mentioned
            # test-postgres, package, dogfood or example-surface since they
            # were added — the header under-reported this script's own
            # coverage by half, in the output a reader consults to find out
            # what it covers.
            printf 'CI gate commands in %s (jobs: %s):\n\n' \
                "${CI_YML#"$REPO_ROOT"/}" \
                "$(printf '%s' "$GATE_JOBS" | tr -d '^$()' | tr '|' ' ' \
                    | sed 's/  */, /g')"
            printf '%s\n' "$ALL_GATES" | sed 's/^/  /'
            printf '\nTiers:\n'
            printf '  --fmt      %s command(s)\n'  "$(tier_fmt | grep -c . || true)"
            printf '  (default)  %s command(s)\n'  "$(tier_default | grep -c . || true)"
            printf '  --full     %s command(s)\n'  "$(tier_full | grep -c . || true)"
            exit 0
            ;;
        -h|--help)
            sed -n '4,30p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            printf 'preflight: unknown option %s (try --help)\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

if [ ! -f "$CI_YML" ]; then
    printf 'preflight: cannot find %s\n' "$CI_YML" >&2
    exit 2
fi

check_free_disk
report_external_prerequisites
note_skipped_steps
apply_ci_env

printf '\033[1mpreflight: %s tier\033[0m  (logs in %s)\n' "$TIER" "$LOG_DIR"

# Resolve the gate list in THIS shell, not in a process substitution. A drift
# failure inside `tier_*` calls `exit`, and from a subshell that only kills the
# subshell — leaving the loop with nothing to read and the summary scoring an
# empty run as a pass. Assigning first makes that exit status observable here.
if ! GATES=$("tier_$TIER"); then
    exit 2
fi

while IFS= read -r gate; do
    [ -n "$gate" ] && run_gate "$gate"
done <<<"$GATES"

summarise
