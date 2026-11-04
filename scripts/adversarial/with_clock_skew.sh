#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Run a command with its wall clock shifted, so a test can stand in for a
# replica whose clock is wrong while the database's is right.
#
#   scripts/adversarial/with_clock_skew.sh '+1d' target/debug/deps/<test-binary> \
#       --include-ignored store::lease::tests::postgres_keeps_the_contract_on_the_database_clock
#
# The first argument is a libfaketime offset ('+1d', '-90s', '+2h'). The
# monotonic clock is left alone, so timers and sleeps still take the time they
# say. Run a test BINARY, not `cargo test`: under the shim cargo's own mtime
# comparisons would be skewed too.
#
# Needs libfaketime (`apt-get install libfaketime`). Linux only.
# Exit status is the command's, or 2 when libfaketime is missing or the clock
# did not move, so a skewed run that silently ran unskewed cannot pass.

set -Eeuo pipefail

if [ "$#" -lt 2 ]; then
    echo "usage: $0 OFFSET COMMAND [ARGS...]" >&2
    exit 2
fi
offset=$1
shift

lib=""
for candidate in \
    /usr/lib/x86_64-linux-gnu/faketime/libfaketime.so.1 \
    /usr/lib/aarch64-linux-gnu/faketime/libfaketime.so.1 \
    /usr/lib/faketime/libfaketime.so.1; do
    if [ -e "$candidate" ]; then
        lib=$candidate
        break
    fi
done
if [ -z "$lib" ]; then
    echo "libfaketime not found; install it (apt-get install libfaketime)" >&2
    exit 2
fi

export LD_PRELOAD="$lib${LD_PRELOAD:+:$LD_PRELOAD}"
export FAKETIME="$offset"
export FAKETIME_DONT_FAKE_MONOTONIC=1

# The shim must be working: the shifted clock and the real one differ.
shifted=$(date +%s)
real=$(env -u LD_PRELOAD -u FAKETIME date +%s)
if [ "$shifted" -eq "$real" ]; then
    echo "the clock did not move under FAKETIME=$offset; refusing to run unskewed" >&2
    exit 2
fi

exec "$@"
