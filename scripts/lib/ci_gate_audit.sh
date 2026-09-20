#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Completeness guards over .github/workflows/ci.yml. Sourced by
# scripts/lib/ci_gates.sh, so every caller of that file gets these too; not
# executable on its own.
#
# `ci_gates.sh` answers "what does CI run?". This file answers the question
# that makes that answer worth anything: "is that all of it?" A gate inventory
# nobody checks for completeness reports "N of N proven" over whatever subset
# it happens to know about, which is the defect class this repository keeps
# finding — a gate that runs, goes green, and measures nothing.
#
# Split out of ci_gates.sh rather than appended to it because that file reached
# the 500-line limit `scripts/check_file_lengths.sh` enforces, the same reason
# `scripts/lib/preflight_prechecks.sh` is its own file.
#
# The caller must set CI_YML before sourcing ci_gates.sh.

# Bidirectional drift guard.
#
# `require_ci_gate` in preflight.sh catches one direction: a tier naming a
# command CI no longer runs. It cannot catch the other, and that is how two
# real gates went uncovered for as long as they did — `test-postgres` and
# `package` were simply jobs nothing had been told about, so nothing anywhere
# noticed they were missing. A guard that only fails on staleness is half a
# guard.
#
# This asserts every job in ci.yml is classified. A new job is either a gate or
# an explicit exemption; it cannot be neither, and it cannot be silence.
#
# Lived in preflight.sh until it was moved here, for the reason at the top of
# this file. preflight.sh runs in no workflow — `grep -rn preflight
# .github/workflows/` finds only comments — so the only completeness check over
# ci.yml's job list ran exclusively on whichever laptop chose to run it, while
# `prove_gates_fail.sh` reported "N of N proven" over a job set nothing had
# confirmed was the whole set. It is now called by both, and by
# `scripts/check_gate_inventory.sh`, which ci.yml runs.
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
        cat >&2 <<MSG
${0##*/}: unknown CI job(s) — gate coverage cannot be trusted.

  ci.yml defines job(s) scripts/lib/ci_gates.sh has never been told about:
$(printf '      %s\n' $unknown)

  Add each to GATE_JOBS (so it is run and proven) or to NON_GATE_JOBS with a
  reason (so the exemption is visible). Refusing to run rather than report a
  green that silently skips a gate.
MSG
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
        cat >&2 <<MSG
${0##*/}: GATE_JOBS names job(s) that contribute no gate.

  Listed as gates, but no runnable step was extracted from them:
$(printf '      %s\n' $empty)

  A job whose steps are all \`uses:\` (a marketplace action) has no \`run:\`
  line to copy, so it is filed as a gate and then silently skipped. Move it to
  NON_GATE_JOBS with a reason, or teach the parser to reach its steps.
MSG
        exit 2
    fi
}

# Emits one line per `uses:` step in ci.yml as
# "<job>\t<step name>\t<owner/repo>". The step name is empty for a step written
# as a bare `- uses:` with no `name:` key; `require_registered_actions` rejects
# those rather than guessing, because a step with no name cannot be registered
# and cannot be spoken about in a failure message.
#
# The action is emitted without its `@<sha>`: the registry is about what a step
# *does*, and re-pinning an action is not a change of decision.
action_steps() {
    awk '
        /^[[:space:]]*#/      { next }
        /^jobs:[[:space:]]*$/ { in_jobs = 1; next }
        /^[^[:space:]#]/      { in_jobs = 0 }
        in_jobs && /^  [a-z0-9_-]+:[[:space:]]*$/ {
            job = $1; sub(/:$/, "", job); in_step = 0; step = ""
        }
        # A list item starts a new step and clears the previous name. No
        # `next`: `- uses: actions/checkout@...` is both the item and the
        # `uses:`, and the rule below has to see the same line.
        /^[[:space:]]*-[[:space:]]/ {
            in_step = 1
            step = ""
            line = $0
            if (line ~ /^[[:space:]]*-[[:space:]]+name:[[:space:]]*/) {
                sub(/^[[:space:]]*-[[:space:]]+name:[[:space:]]*/, "", line)
                sub(/[[:space:]]+$/, "", line)
                gsub(/^"|"$/, "", line)
                step = line
            }
        }
        # A step-level `name:` on its own line. Guarded by in_step so the job
        # header`s own `name:` (four spaces, before `steps:`) is not read as a
        # step name. A `name:` inside a `with:` block comes *after* the `uses:`
        # it belongs to, so it cannot overwrite the name already emitted.
        in_step && /^[[:space:]]+name:[[:space:]]*[^[:space:]]/ {
            line = $0
            sub(/^[[:space:]]+name:[[:space:]]*/, "", line)
            sub(/[[:space:]]+$/, "", line)
            gsub(/^"|"$/, "", line)
            step = line
            next
        }
        /[[:space:]]uses:[[:space:]]*[^[:space:]]/ {
            line = $0
            sub(/^.*[[:space:]]uses:[[:space:]]*/, "", line)
            sub(/[[:space:]].*$/, "", line)
            sub(/@.*$/, "", line)
            gsub(/^"|"$/, "", line)
            printf "%s\t%s\t%s\n", job, step, line
        }
    ' "$CI_YML"
}

# Every `uses:` step in ci.yml is either infrastructure (SETUP_ACTIONS) or a
# registered exemption carrying a reason.
#
# `gates_for_jobs` emits one gate per `run:` step, so a `uses:` step contributes
# nothing and the unregistered-gate guard in prove_gates_fail.sh has nothing to
# complain about. That is fine for `actions/checkout`, and it is not fine for
# `EmbarkStudios/cargo-deny-action`, which is a verdict: the binding's copy of
# it is the only thing auditing 379 transitive dependencies including
# `aws-lc-sys`, a native C crypto build shipped to users.
#
# Proving a marketplace action can fail is not expressible in
# `prove_gates_fail.sh` and this function does not pretend otherwise. That
# harness runs a step's own command against an injected defect; a `uses:` step
# has no command, only an action reference resolved by GitHub's runner, and the
# nearest local approximation — invoking a `cargo deny` that happens to be on
# PATH — would be a *different* gate at a different version against a different
# config. The repository's own rule about that is explicit in
# `prove_workflow_gates_fail.py`: "a harness that runs the block under a
# stricter shell than CI does is not reproducing the gate, it is inventing a
# different one."
#
# So the omission is made explicit and checked instead, in the shape this
# repository already uses for a thing it has decided not to prove: a named
# entry, a required reason, and a hard failure when the entry stops naming
# something real. That is `EXEMPT` in prove_workflow_gates_fail.py, `skip` in
# deny.toml, and SKIP_STEPS here.
require_registered_actions() {
    local line body reason job step action key
    local registered="" used="" unregistered="" unnamed="" stale="" seen=""
    local pat n_exempt=0 n_setup=0

    if [ ! -f "$ACTION_EXEMPTIONS" ]; then
        printf '%s: %s is missing — every `uses:` exemption is recorded there.\n' \
            "${0##*/}" "$ACTION_EXEMPTIONS" >&2
        exit 2
    fi

    while IFS= read -r line; do
        case "$line" in ''|'#'*) continue ;; esac
        case "$line" in
            *'#'*) ;;
            *)
                printf '%s: %s: entry has no reason: %s\n' \
                    "${0##*/}" "${ACTION_EXEMPTIONS##*/}" "$line" >&2
                exit 2 ;;
        esac
        body=${line%%#*}
        reason=${line#*#}
        body=${body%"${body##*[![:space:]]}"}
        reason=${reason#"${reason%%[![:space:]]*}"}
        if [ -z "$reason" ]; then
            printf '%s: %s: entry has no reason: %s\n' \
                "${0##*/}" "${ACTION_EXEMPTIONS##*/}" "$line" >&2
            exit 2
        fi
        case "$body" in
            *'::'*) ;;
            *)
                printf '%s: %s: expected `JOB::STEP NAME  # reason`, got: %s\n' \
                    "${0##*/}" "${ACTION_EXEMPTIONS##*/}" "$line" >&2
                exit 2 ;;
        esac
        registered="$registered$body"$'\n'
    done < "$ACTION_EXEMPTIONS"

    # Split by parameter expansion rather than `IFS=$'\t' read -r job step
    # action`. A tab is IFS *whitespace*, so bash folds a run of them into one
    # delimiter: the two adjacent tabs of an unnamed step (`job\t\taction`)
    # collapse, `action` comes back empty, and every setup step is silently
    # skipped. Written the other way first, and the SETUP_ACTIONS rot check
    # duly reported that ci.yml no longer uses `actions/checkout`.
    while IFS= read -r line; do
        [ -n "$line" ] || continue
        job=${line%%$'\t'*}
        step=${line#*$'\t'}
        action=${step#*$'\t'}
        step=${step%%$'\t'*}
        [ -n "$action" ] || continue
        if printf '%s' "$action" | grep -Eq -- "$SETUP_ACTIONS"; then
            seen="$seen$action"$'\n'
            n_setup=$((n_setup + 1))
            continue
        fi
        if [ -z "$step" ]; then
            unnamed="$unnamed      job $job: uses $action"$'\n'
            continue
        fi
        key="$job::$step"
        if printf '%s' "$registered" | grep -Fxq -- "$key"; then
            used="$used$key"$'\n'
            n_exempt=$((n_exempt + 1))
        else
            unregistered="$unregistered      $key  (uses $action)"$'\n'
        fi
    done < <(action_steps)

    if [ -n "$unnamed" ]; then
        printf '%s: ci.yml has an unnamed `uses:` step that is not infrastructure:\n' \
            "${0##*/}" >&2
        printf '%s' "$unnamed" >&2
        printf '  A step that renders a verdict needs a `name:` so it can be registered\n' >&2
        printf '  in %s and named when it fails.\n' "${ACTION_EXEMPTIONS##*/}" >&2
        exit 2
    fi

    if [ -n "$unregistered" ]; then
        printf '%s: unregistered `uses:` step(s) in ci.yml:\n' "${0##*/}" >&2
        printf '%s' "$unregistered" >&2
        cat >&2 <<MSG

  These steps can block a merge and nothing audits them. \`gates_for_jobs\`
  emits a gate per \`run:\` step, so a \`uses:\` step contributes none and the
  unregistered-gate guard never sees it.

  Either add the action to SETUP_ACTIONS in scripts/lib/ci_gates.sh (it
  prepares the runner and renders no verdict about this code), or add an entry
  to ${ACTION_EXEMPTIONS##*/} saying why its failure cannot be proven here.
MSG
        exit 2
    fi

    while IFS= read -r key; do
        [ -n "$key" ] || continue
        if ! printf '%s' "$used" | grep -Fxq -- "$key"; then
            stale="$stale      $key"$'\n'
        fi
    done < <(printf '%s' "$registered")
    if [ -n "$stale" ]; then
        printf '%s: %s names step(s) ci.yml no longer has:\n' \
            "${0##*/}" "${ACTION_EXEMPTIONS##*/}" >&2
        printf '%s' "$stale" >&2
        printf '  An exemption for a step that does not exist covers nothing. Remove it,\n' >&2
        printf '  or correct it to the new job/step name.\n' >&2
        exit 2
    fi

    # SETUP_ACTIONS rot check, the same rule `require_known_skips` applies to
    # SKIP_STEPS: an alternative that matches no `uses:` is a classification
    # that covers nothing, and the next action added under that name would
    # inherit a waiver nobody decided to give it.
    while IFS= read -r pat; do
        [ -n "$pat" ] || continue
        if ! printf '%s' "$seen" | grep -Fxq -- "$pat"; then
            stale="$stale      $pat"$'\n'
        fi
    done < <(printf '%s\n' "$SETUP_ACTIONS" | tr -d '^$()' | tr '|' '\n')
    if [ -n "$stale" ]; then
        printf '%s: SETUP_ACTIONS names action(s) ci.yml no longer uses:\n' \
            "${0##*/}" >&2
        printf '%s' "$stale" >&2
        printf '  Remove the entry, or correct it to the action that replaced it.\n' >&2
        exit 2
    fi

    printf '%s: %d setup action step(s), %d registered non-gate action step(s)\n' \
        "${0##*/}" "$n_setup" "$n_exempt"
}
