#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Detects drift between the vendored SLIMRPC specification and upstream.
#
# Why this exists. `bindings/a2a-protocol-slimrpc` claims to implement every
# method in the SLIMRPC specification's inventory. That claim referenced a URL
# for its whole life: nothing in this repository could check it, and nothing
# would have noticed upstream adding a method, renaming one, or changing the
# wire format underneath the binding.
#
# The gRPC binding never had that problem — `proto/a2a_v1/a2a.proto` is
# vendored and `check_proto_copies.sh` keeps every copy byte-identical. This is
# the missing half of that symmetry.
#
# Upstream is experimental and may change without ceremony, so a failure here
# is information rather than an accusation: read the diff, decide whether the
# binding must follow, then re-vendor and record the decision.
#
# Network. This needs it, which is why it runs in `official-tck.yml` (already
# cloning upstream) rather than in the offline Format job. With no network it
# exits 3 — "could not check" — instead of reporting agreement it did not
# verify, because a green tick for a check that never ran is worse than no
# check at all.
#
# Usage:
#   ./scripts/check_slimrpc_spec.sh             # compare, report drift
#   ./scripts/check_slimrpc_spec.sh --update    # re-vendor from upstream
#
# Exit codes:
#   0  vendored copies match upstream
#   1  drift: upstream changed, or a vendored file is missing
#   3  upstream unreachable (nothing was verified)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR_DIR="$REPO_ROOT/spec/slimrpc_v1"
RAW_BASE="https://raw.githubusercontent.com/a2aproject/experimental-cpb-slimrpc/main"

# Vendored basename : upstream path
FILES=(
    "slimrpc.md:spec/v1/slimrpc.md"
    "slimrpc-multicast.md:spec/v1/slimrpc-multicast.md"
)

UPDATE=0
[ "${1:-}" = "--update" ] && UPDATE=1

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

drift=0
missing=0

for entry in "${FILES[@]}"; do
    name="${entry%%:*}"
    path="${entry#*:}"
    local_file="$VENDOR_DIR/$name"

    code="$(curl -sS -L --max-time 30 -w '%{http_code}' -o "$tmp/$name" \
        "$RAW_BASE/$path" 2>/dev/null || echo "000")"

    if [ "$code" = "000" ]; then
        printf 'check_slimrpc_spec: upstream unreachable (network or proxy)\n' >&2
        printf '  nothing was compared, so nothing is being reported as agreeing\n' >&2
        exit 3
    fi

    if [ "$code" != "200" ]; then
        printf 'check_slimrpc_spec: upstream returned HTTP %s for %s\n' "$code" "$path" >&2
        printf '  the file may have been moved or renamed upstream\n' >&2
        drift=1
        continue
    fi

    if [ "$UPDATE" = "1" ]; then
        mkdir -p "$VENDOR_DIR"
        cp "$tmp/$name" "$local_file"
        printf '  updated %s\n' "$name"
        continue
    fi

    if [ ! -f "$local_file" ]; then
        printf 'check_slimrpc_spec: %s is not vendored\n' "$name" >&2
        missing=1
        continue
    fi

    if ! cmp -s "$local_file" "$tmp/$name"; then
        printf 'check_slimrpc_spec: %s differs from upstream\n' "$name" >&2
        printf '  vendored: %s\n' "$(sha256sum "$local_file" | cut -d' ' -f1)"
        printf '  upstream: %s\n' "$(sha256sum "$tmp/$name" | cut -d' ' -f1)"
        printf '  diff (vendored -> upstream):\n' >&2
        diff -u "$local_file" "$tmp/$name" | head -60 || true
        drift=1
    fi
done

if [ "$UPDATE" = "1" ]; then
    printf '\nRe-vendored. Update the hashes in %s and review the diff before committing.\n' \
        "spec/slimrpc_v1/README.md"
    exit 0
fi

if [ "$missing" = "1" ] || [ "$drift" = "1" ]; then
    printf '\nThe SLIMRPC binding claims to implement this specification. Upstream has\n' >&2
    printf 'moved, so that claim is now unverified.\n\n' >&2
    printf '  1. Read the diff above.\n' >&2
    printf '  2. Decide whether bindings/a2a-protocol-slimrpc must follow.\n' >&2
    printf '  3. ./scripts/check_slimrpc_spec.sh --update, refresh the hashes in\n' >&2
    printf '     spec/slimrpc_v1/README.md, and record the decision in the same commit.\n' >&2
    exit 1
fi

printf 'check_slimrpc_spec: %d vendored file(s) match upstream\n' "${#FILES[@]}"
