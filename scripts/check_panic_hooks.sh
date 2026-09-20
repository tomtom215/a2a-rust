#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Forbids process-global panic-hook installation in this workspace's sources.
#
# `std::panic::set_hook` replaces the panic hook for the entire process.
# libtest runs a test binary's tests as parallel threads in one process, so a
# test that installs a no-op hook to silence its own expected panic silences
# every other thread for as long as it holds that window. A genuinely failing
# test that races it loses its message: cargo lists the test under `failures:`
# with no `---- stdout ----` section at all, and CI goes red with no reason in
# the log, non-deterministically.
#
# Not hypothetical. Three tests in two crates did exactly this, and the cost
# was paid twice over. It cost three gates in `scripts/prove_gates_fail.sh`
# their verdicts — `cargo test -p a2a-protocol-server --features {sqlite,
# postgres,auth-jwt}` each returned INCONCLUSIVE, because the injected defect
# failed correctly and its panic message never reached the output the prover
# greps. Those three were only the symptom that happened to be looked at; any
# failing test in either crate could lose its message the same way.
#
# Measured on the pattern in isolation, an unrelated failing test in the same
# binary lost its marker in 3 of 3 parallel runs with the swap present, and
# kept it in 3 of 3 with the swap removed.
#
# Nothing was bought with it. `catch_unwind` returns the payload an
# expected-panic test asserts on, and libtest already captures panic output per
# test and discards it when the test passes — so removing the swap prints
# nothing extra, which is the other half of the same measurement: the expected
# panic's own text appeared in 0 of 3 runs either way. The silencing was
# cosmetic, and the cosmetics were already handled by libtest.
#
# A binary installing a real hook at startup — a crash reporter, say — would be
# a legitimate use this gate does not distinguish, because there are none in
# the tree to distinguish from. Give it an allowance here, with its reason,
# rather than deleting the check.
#
# Known limit: a line comment is stripped at its first `//`, so a commented-out
# call is correctly ignored, while a call inside a block comment (`/* */`) or a
# string literal would still be reported. Neither exists in the tree.
#
# Exit codes: 0 clean, 1 a hook installation was found, 2 nothing was scanned.

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

scanned=$(git ls-files '*.rs' | grep -c . || true)
if [ "$scanned" -eq 0 ]; then
    printf 'check_panic_hooks: no tracked .rs files — wrong repository root?\n' >&2
    exit 2
fi

# One grep over every tracked source, then strip comments from the handful of
# candidate lines. Doing it in that order keeps the common case to a single
# pass while still ruling out the comments at the sites this gate exists for,
# which name the call in prose.
findings=""
while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    text=${hit#*:}          # drop the file
    text=${text#*:}         # drop the line number
    case "${text%%//*}" in
        *set_hook*) findings+="$hit"$'\n' ;;
    esac
done < <(git ls-files -z '*.rs' | xargs -0 -r grep -n 'set_hook[[:space:]]*(' || true)

if [ -n "$findings" ]; then
    printf 'check_panic_hooks: process-global panic hook installed\n\n' >&2
    printf '%s' "$findings" >&2
    printf '\nlibtest runs tests as parallel threads in one process, so this hook\n' >&2
    printf 'applies to every other test in the binary for as long as it is held.\n' >&2
    printf 'A failing test that races it is reported with no message at all.\n\n' >&2
    printf 'For an expected panic, delete the swap: `catch_unwind` returns the\n' >&2
    printf 'payload to assert on, and libtest already discards captured panic\n' >&2
    printf 'output when the test passes, so nothing extra is printed.\n' >&2
    exit 1
fi

printf 'check_panic_hooks: %d source(s) scanned — no process-global panic hooks\n' \
    "$scanned"
