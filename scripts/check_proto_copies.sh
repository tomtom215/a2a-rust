#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Asserts every vendored copy of the canonical A2A proto is byte-identical.
#
# Why copies exist at all: `build.rs` resolves protos relative to
# CARGO_MANIFEST_DIR, and a published crate can only ship files inside its own
# directory. Each crate that runs protoc therefore vendors its own tree. That
# is the standard arrangement and it is fine — as long as something checks the
# copies still agree.
#
# Nothing did. Five copies of a 34 KiB interface definition sat in the tree
# with no guard, so a fix applied to one and not the others would produce a
# client and a server that compile cleanly, pass their own tests, and disagree
# on the wire. That failure is invisible in every gate the project already
# runs, which is exactly the shape of defect worth a dedicated check.
#
# Exit codes: 0 all copies agree, 1 drift, 2 a copy is missing.

set -Eeuo pipefail

REPO_ROOT=$(git rev-parse --show-toplevel)
cd "$REPO_ROOT"

# The reference copy. `a2a-protocol-types` owns the message types the rest of
# the workspace re-exports, so its tree is the natural source of truth.
REFERENCE="crates/a2a-protocol-types/proto"

# Every other tree that must match it, and why it exists.
#   crates/a2a-protocol-client/proto — tonic client stubs (feature `grpc`)
#   crates/a2a-protocol-server/proto — tonic server stubs (feature `grpc`)
#   tck/proto                        — the TCK's own independent codegen, so a
#                                      conformance verdict does not depend on
#                                      the implementation's conversion layer
#   proto                            — repo-root copy kept for external readers
COPIES=(
    "crates/a2a-protocol-client/proto"
    "crates/a2a-protocol-server/proto"
    "tck/proto"
    "proto"
)

fail=0
missing=0

# Compare the whole a2a_v1 tree, not just a2a.proto: the google/api/*.proto
# imports are part of the compiled interface, and a drifted `http.proto`
# changes generated code just as surely as a drifted `a2a.proto`.
mapfile -t REFERENCE_FILES < <(find "$REFERENCE/a2a_v1" -type f -name '*.proto' | sort)

if [ "${#REFERENCE_FILES[@]}" -eq 0 ]; then
    printf 'check_proto_copies: no .proto files under %s — wrong path?\n' \
        "$REFERENCE/a2a_v1" >&2
    exit 2
fi

for copy in "${COPIES[@]}"; do
    for ref in "${REFERENCE_FILES[@]}"; do
        rel=${ref#"$REFERENCE/"}
        other="$copy/$rel"
        if [ ! -f "$other" ]; then
            printf 'MISSING  %s (present in %s)\n' "$other" "$REFERENCE" >&2
            missing=1
            continue
        fi
        if ! cmp -s "$ref" "$other"; then
            printf 'DRIFT    %s differs from %s\n' "$other" "$ref" >&2
            fail=1
        fi
    done

    # The other direction: a copy carrying a file the reference does not. An
    # orphan .proto in one crate is drift too — it compiles into that crate's
    # generated code and nowhere else.
    while IFS= read -r extra; do
        rel=${extra#"$copy/"}
        if [ ! -f "$REFERENCE/$rel" ]; then
            printf 'EXTRA    %s has no counterpart in %s\n' "$extra" "$REFERENCE" >&2
            fail=1
        fi
    done < <(find "$copy/a2a_v1" -type f -name '*.proto' 2>/dev/null | sort)
done

if [ "$missing" -ne 0 ]; then
    printf '\ncheck_proto_copies: a vendored copy is incomplete. Copy the missing\n' >&2
    printf 'file(s) from %s, or drop the tree from COPIES if it is gone.\n' "$REFERENCE" >&2
    exit 2
fi

if [ "$fail" -ne 0 ]; then
    printf '\ncheck_proto_copies: vendored protos disagree.\n' >&2
    printf 'These compile independently, so drift here produces a client and a\n' >&2
    printf 'server that both build, both pass their tests, and disagree on the\n' >&2
    printf 'wire. Sync every copy from %s.\n' "$REFERENCE" >&2
    exit 1
fi

printf 'check_proto_copies: %d file(s) x %d cop(ies) — all identical\n' \
    "${#REFERENCE_FILES[@]}" "${#COPIES[@]}"
