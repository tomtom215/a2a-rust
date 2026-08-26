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

# Free space, in GB, per tier. Measured 2026-08-19 on this repository,
# 4 vCPU, rustc 1.94.1:
#
#   --full from an empty target/            peaked under 20 GB
#   --full over a tree built without CI's   reached 29 GB, then ENOSPC
#     RUSTFLAGS (the common case)             19 gates into 53
#
# The threshold is the second measurement plus headroom, because the second is
# what most people will be running: anyone who has typed `cargo test` has a
# target/ full of artifacts with a different fingerprint, and this script adds
# a second set beside them rather than replacing them.
REQUIRED_GB_full=32
REQUIRED_GB_default=12
REQUIRED_GB_fmt=1

check_free_disk() {
    [ "$FORCE" -eq 1 ] && return 0

    local need avail_kb avail_gb where
    eval "need=\${REQUIRED_GB_$TIER:-1}"

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
    [ "$avail_gb" -ge "$need" ] && return 0

    cat >&2 <<EOF

preflight: ${avail_gb} GB free at ${where}; the ${TIER} tier wants ${need} GB.

  This exports CI's RUSTFLAGS and CARGO_PROFILE_DEV_DEBUG, which are part of
  cargo's fingerprint, so it builds a second set of artifacts beside anything
  compiled without them. Measured 2026-08-19: a --full run over such a tree
  grew target/ to 29 GB and died with "No space left on device" 19 gates in,
  inside a cargo invocation that had nothing to do with the real problem.

  Free space first:

      cargo clean                       # one fingerprint instead of two
      rm -rf target/debug/incremental   # cheaper, often enough

  Or run it anyway:

      scripts/preflight.sh --${TIER} --force

EOF
    exit 2
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

