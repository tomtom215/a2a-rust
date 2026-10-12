#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Detects drift between the vendored A2A proto and the A2A releases upstream.
#
# Why this exists. `check_proto_copies.sh` keeps the six vendored copies of
# `a2a.proto` identical to each other. It could not say whether they were
# identical to the specification. They were not: A2A v1.0.1 (2026-05-28)
# rewrote the `tenant` comments, the normative text "clients MUST include this
# value in the `tenant` field of all request messages", and every copy here
# stayed on v1.0.0 until 2026-10-10. The SLIMRPC spec had this check
# (`check_slimrpc_spec.sh`) and the proto every binding compiles from did not.
#
# What it checks, against `proto/UPSTREAM`:
#   1. the pinned tag still resolves to the pinned commit (a moved tag is drift);
#   2. the file at that commit still hashes to the pinned sha256, and so does
#      the vendored reference copy;
#   3. no later A2A release (a `vX.Y.Z` tag, pre-releases excluded) carries a
#      different proto. A newer release with an identical proto is reported and
#      passes; the pin is only stale when the bytes moved.
# It also reports, without failing, how upstream `main` differs from the pin:
# `main` is where unreleased edits wait, and a check that went red on every
# unreleased comment would be red permanently.
#
# Network. This needs it, which is why it runs in `official-tck.yml` beside
# `check_slimrpc_spec.sh`. With no network it exits 3, "could not check",
# rather than reporting agreement it did not verify.
#
# Exit codes:
#   0  the vendored proto is the latest release's proto
#   1  drift: tag moved, hash mismatch, or a newer release changed the proto
#   3  upstream unreachable (nothing was verified)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIN_FILE="$REPO_ROOT/proto/UPSTREAM"
VENDORED="$REPO_ROOT/crates/a2a-protocol-types/proto/a2a_v1/a2a.proto"

pin() { sed -n "s/^$1=//p" "$PIN_FILE"; }
REPOSITORY=$(pin repository)
TAG=$(pin tag)
COMMIT=$(pin commit)
FILE_PATH=$(pin path)
SHA256=$(pin sha256)
for v in REPOSITORY TAG COMMIT FILE_PATH SHA256; do
    if [ -z "${!v}" ]; then
        printf 'check_a2a_proto_upstream: %s has no value for %s\n' "$PIN_FILE" "$v" >&2
        exit 1
    fi
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ! git ls-remote --tags --refs "$REPOSITORY" >"$tmp/tags" 2>"$tmp/err"; then
    printf 'check_a2a_proto_upstream: upstream unreachable (network or proxy)\n' >&2
    sed 's/^/  /' "$tmp/err" >&2
    printf '  nothing was compared, so nothing is being reported as agreeing\n' >&2
    exit 3
fi

drift=0

# ── 1. The pinned tag has not moved ──────────────────────────────────────────
# `--refs` drops the peeled `^{}` lines, so an annotated tag would show its tag
# object here rather than the commit. A2A's release tags are lightweight; if
# that ever changes this comparison fails loudly rather than passing wrongly.
tag_commit=$(awk -v ref="refs/tags/$TAG" '$2 == ref { print $1 }' "$tmp/tags")
if [ -z "$tag_commit" ]; then
    printf 'check_a2a_proto_upstream: tag %s no longer exists upstream\n' "$TAG" >&2
    drift=1
elif [ "$tag_commit" != "$COMMIT" ]; then
    printf 'check_a2a_proto_upstream: tag %s now points at %s; %s pins %s\n' \
        "$TAG" "$tag_commit" "proto/UPSTREAM" "$COMMIT" >&2
    drift=1
fi

# One shallow fetch per ref, into a bare scratch repository: no history, no
# checkout, a few hundred KiB each.
git init --quiet --bare "$tmp/up"
fetch_file() { # <ref> <out>
    git -C "$tmp/up" fetch --quiet --depth 1 "$REPOSITORY" "$1" 2>"$tmp/err" || {
        printf 'check_a2a_proto_upstream: could not fetch %s\n' "$1" >&2
        sed 's/^/  /' "$tmp/err" >&2
        exit 3
    }
    git -C "$tmp/up" show "FETCH_HEAD:$FILE_PATH" >"$2"
}
sha_of() { sha256sum "$1" | cut -d' ' -f1; }

# ── 2. The pinned file is what we vendor ─────────────────────────────────────
fetch_file "$COMMIT" "$tmp/pinned.proto"
if [ "$(sha_of "$tmp/pinned.proto")" != "$SHA256" ]; then
    printf 'check_a2a_proto_upstream: %s at %s hashes to %s; proto/UPSTREAM pins %s\n' \
        "$FILE_PATH" "$COMMIT" "$(sha_of "$tmp/pinned.proto")" "$SHA256" >&2
    drift=1
fi
if ! cmp -s "$tmp/pinned.proto" "$VENDORED"; then
    printf 'check_a2a_proto_upstream: the vendored proto is not %s at %s\n' "$FILE_PATH" "$TAG" >&2
    diff -u "$VENDORED" "$tmp/pinned.proto" | head -60 >&2 || true
    drift=1
fi

# ── 3. No newer release changed the proto ────────────────────────────────────
latest=$(awk '{ sub("refs/tags/", "", $2); print $2 }' "$tmp/tags" \
    | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n 1)
if [ -z "$latest" ]; then
    printf 'check_a2a_proto_upstream: upstream has no vX.Y.Z release tag at all\n' >&2
    printf '  the tagging scheme changed; this check needs updating before it can mean anything\n' >&2
    exit 1
fi
if [ "$latest" != "$TAG" ]; then
    if [ "$(printf '%s\n%s\n' "$TAG" "$latest" | sort -V | tail -n 1)" != "$latest" ]; then
        printf 'check_a2a_proto_upstream: pinned %s is newer than the latest release %s\n' "$TAG" "$latest" >&2
        drift=1
    else
        fetch_file "refs/tags/$latest" "$tmp/latest.proto"
        if cmp -s "$tmp/latest.proto" "$VENDORED"; then
            printf 'check_a2a_proto_upstream: %s is out; its proto is byte-identical to %s\n' "$latest" "$TAG"
            printf '  move the pin to %s (tag, commit) when convenient; nothing on the wire changed\n' "$latest"
        else
            printf 'check_a2a_proto_upstream: A2A %s changed the proto; this repository vendors %s\n' "$latest" "$TAG" >&2
            diff -u "$VENDORED" "$tmp/latest.proto" | head -80 >&2 || true
            printf '\n  1. Read the diff above and the release notes.\n' >&2
            printf '  2. Copy the new file over all six vendored copies (check_proto_copies.sh lists them).\n' >&2
            printf '  3. Update tag, commit and sha256 in proto/UPSTREAM in the same commit.\n' >&2
            drift=1
        fi
    fi
fi

# ── 4. Unreleased edits on main: reported, never failing ─────────────────────
fetch_file "refs/heads/main" "$tmp/main.proto"
if cmp -s "$tmp/main.proto" "$VENDORED"; then
    main_note="upstream main carries the same proto"
else
    # `diff` exits 1 on a difference, which `pipefail` would turn into a
    # silent exit of the whole script; that happened on this line's first run.
    changed=$( { diff "$VENDORED" "$tmp/main.proto" || true; } | grep -c '^[<>]' )
    main_note="upstream main differs by $changed line(s), unreleased (informational)"
fi

if [ "$drift" -ne 0 ]; then
    exit 1
fi
printf 'check_a2a_proto_upstream: vendored a2a.proto is %s (%s); %s\n' "$TAG" "$SHA256" "$main_note"
