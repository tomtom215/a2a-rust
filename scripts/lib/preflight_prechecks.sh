#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
# Machine prechecks for scripts/preflight.sh. Sourced by it; not executable
# on its own.
#
# Split out of preflight.sh on 2026-08-19 rather than recorded as a 500-line
# exemption: CONTRIBUTING's rule for a script that grows past the limit is to
# move helpers into something it sources, and scripts/lib/ci_gates.sh already
# established where that goes.
#
# Neither function here is a gate. They report facts about the machine that
# decide whether the gates can mean anything, and they run before the first
# compile rather than after it.
#
# The caller must set REPO_ROOT, TIER, FORCE and ALL_GATES before sourcing.

# Free space, in GB. Measured 2026-08-19 on this repository, 4 vCPU,
# rustc 1.94.1, by sampling `du` through two complete `--full` runs:
#
#   --full from a single-fingerprint tree   target/ peaked at 10.2 GB, and the
#                                           binding's own target/ at 6.4 GB —
#                                           16.6 GB of artifacts in total
#   --full over a tree also built without   target/ alone reached 29 GB, then
#     CI's RUSTFLAGS (the common case)      ENOSPC, 19 gates into 53
#
# Re-measured on a third run the same day, which is why ADVISE went from 20 to
# 24: starting at 20 GB free, `--full` finished with **2.8 GB left**. It passed
# the advisory without printing anything and then consumed 17.2 GB, which is
# 16.6 plus the churn of rebuilding the binding's target from scratch. A
# threshold that a completing run clears by three gigabytes is not advice, it is
# a coincidence — so the advisory is now the measured consumption plus about
# 40%, and the floor stays where it is because it answers a different question
# (the binding's 379-dependency build alone cannot land under it).
#
# Which of the two you are in is not knowable from outside target/, so this
# **warns and continues** above a hard floor rather than refusing at the
# advisory figure. A precheck that fires on a warm tree teaches people to pass
# --force, and a --force people always pass is not a precheck — the same
# argument check_file_lengths.sh makes for not blocking growth of a file that
# is already listed.
#
# The floor is where continuing is close to pointless: below it, the binding's
# 379-dependency build alone cannot land.
ADVISE_GB_full=24
ADVISE_GB_default=10
ADVISE_GB_fmt=0
FLOOR_GB_full=8
FLOOR_GB_default=4
FLOOR_GB_fmt=0

check_free_disk() {
    [ "$FORCE" -eq 1 ] && return 0

    local advise floor avail_kb avail_gb where
    eval "advise=\${ADVISE_GB_$TIER:-0}"
    eval "floor=\${FLOOR_GB_$TIER:-0}"
    [ "$advise" -eq 0 ] && return 0

    # Measure the filesystem that will hold the artifacts, not $PWD's: target/
    # can be redirected with CARGO_TARGET_DIR and is a separate mount on some
    # setups.
    where=${CARGO_TARGET_DIR:-$REPO_ROOT/target}
    [ -d "$where" ] || where=$REPO_ROOT

    # `df -Pk` is the POSIX form; GNU and BSD df agree on nothing else.
    avail_kb=$(df -Pk "$where" 2>/dev/null | awk 'NR==2 {print $4}')
    if [ -z "$avail_kb" ]; then
        printf 'preflight: could not measure free space at %s; continuing\n' "$where" >&2
        return 0
    fi
    avail_gb=$(( avail_kb / 1024 / 1024 ))
    [ "$avail_gb" -ge "$advise" ] && return 0

    if [ "$avail_gb" -lt "$floor" ]; then
        cat >&2 <<EOF

preflight: ${avail_gb} GB free at ${where}; the ${TIER} tier cannot finish under ${floor} GB.

  Refusing rather than failing forty minutes in with "No space left on device"
  from inside an unrelated cargo invocation, which is how this figure was
  measured. Free space:

      cargo clean                       # one fingerprint instead of two
      rm -rf target/debug/incremental   # cheaper, often enough

  Or run it anyway:

      scripts/preflight.sh --${TIER} --force

EOF
        exit 2
    fi

    cat >&2 <<EOF

preflight: ${avail_gb} GB free at ${where}; the ${TIER} tier usually wants ${advise} GB.

  Continuing — a warm target/ already holds most of what it needs. But this
  exports CI's RUSTFLAGS and CARGO_PROFILE_DEV_DEBUG, which are part of cargo's
  fingerprint, so a tree also built without them keeps two sets of artifacts:
  measured, that reached 29 GB and then ENOSPC. \`cargo clean\` if this run dies
  in an unexpected place.

EOF
}

# Says which external services the selected tier needs and whether they are
# here. Does not skip anything and does not exit: the gates that need these
# must still fail without them, because `tests/common/spire.rs` is right that a
# suite which silently passes when its dependency is absent is worse than one
# that fails. What this adds is knowing at second 0 instead of minute 90.
report_external_prerequisites() {
    [ "$TIER" = "full" ] || return 0

    local pg_url pg_state spire_state
    # The URL comes from ci.yml, the same place the gates get it, so this
    # cannot drift from what will actually be attempted.
    pg_url=$(printf '%s\n' "$ALL_GATES" \
        | sed -n "s/.*A2A_TEST_POSTGRES_URL='\([^']*\)'.*/\1/p" | head -n 1)
    [ -n "$pg_url" ] || pg_url='postgres://postgres:postgres@localhost:5432/postgres'

    if command -v pg_isready >/dev/null 2>&1 && pg_isready -d "$pg_url" >/dev/null 2>&1; then
        pg_state='present'
    else
        pg_state="MISSING — 4 gate(s) will fail; start one at $pg_url"
    fi

    if [ -n "${SPIRE_BIN_DIR-}" ] \
        && [ -x "${SPIRE_BIN_DIR}/spire-server" ] \
        && [ -x "${SPIRE_BIN_DIR}/spire-agent" ]; then
        spire_state='present'
    elif command -v spire-server >/dev/null 2>&1 && command -v spire-agent >/dev/null 2>&1; then
        spire_state='present'
    else
        spire_state='MISSING — 1 gate will fail; see ci.yml "Install SPIRE"'
    fi

    printf 'preflight: PostgreSQL %s\n' "$pg_state"
    printf 'preflight: SPIRE      %s\n' "$spire_state"
    case "$pg_state$spire_state" in
        *MISSING*)
            printf 'preflight: those gate failures are the machine, not the diff.\n' ;;
    esac
}

