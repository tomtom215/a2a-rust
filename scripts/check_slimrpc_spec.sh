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
# What the first version could not see, and why this one clones. It fetched two
# named files by URL and hash-compared them. That verifies the two documents it
# already knows about and is silent about everything else — so a *new* spec file
# upstream, or a spec developed on a branch, was invisible to it by
# construction. Both existed and neither was noticed: upstream carries
# `spec/v1/slimrpc-collaborative-channel.md` on
# `feat/slimrpc-collaborative-channel` and `spec/v1/slimrpc-channel-moderator.md`
# on `feat/slimrpc-channel-moderator`, and the official `a2a-slimrpc` crate had
# already implemented the first. A check that reports agreement about the two
# files it was told to look at, while a third goes unmentioned, is the exact
# class of defect this repository documents everywhere else.
#
# So the inventory is now upstream's, not ours: every `spec/**/*.md` on `main`
# must be vendored, and every vendored file must still exist on `main`. There is
# no list here to forget to update.
#
# Branches are surveyed rather than enforced. Upstream is experimental and its
# branches come and go; failing on someone's work in progress would be noise.
# But silence is what let the collaborative-channel spec sit unnoticed, so a
# branch-only spec file must be named in KNOWN_BRANCH_SPECS below with a
# one-line disposition. A new one fails until somebody triages it.
#
# Network. This needs it, which is why it runs in `official-tck.yml` (already
# cloning upstream) rather than in the offline Format job. With no network it
# exits 3 — "could not check" — instead of reporting agreement it did not
# verify, because a green tick for a check that never ran is worse than no check
# at all.
#
# Usage:
#   ./scripts/check_slimrpc_spec.sh             # compare, report drift
#   ./scripts/check_slimrpc_spec.sh --update    # re-vendor from upstream main
#
# Exit codes:
#   0  vendored copies match upstream, and no untriaged spec exists
#   1  drift: content changed, a file was added or removed on main, or a branch
#      carries a spec file nobody has triaged
#   3  upstream unreachable (nothing was verified)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR_DIR="$REPO_ROOT/spec/slimrpc_v1"
UPSTREAM_URL="https://github.com/a2aproject/experimental-cpb-slimrpc"

# Spec files that exist only on an upstream branch, and what was decided about
# each. Format: <branch>:<path>:<disposition>. Removing an entry is how you say
# the branch is gone; adding one is how you say you have read the spec.
KNOWN_BRANCH_SPECS=(
    "feat/slimrpc-collaborative-channel:spec/v1/slimrpc-broadcast-live.md:broadcast SendLiveMessage over a SLIM group channel. Replaced spec/v1/slimrpc-collaborative-channel.md on this branch at 0c38776 (2026-09-03), which is why the previous entry stopped matching. Not followed by this binding, and that is a fact rather than a scope preference: its own section 3 requires A2A 1.1 and the SendLiveMessage method, and neither exists in any released A2A specification — a2aproject/A2A is tagged v1.0.1, SendLiveMessage appears nowhere in its docs, and this SDK implements the ratified 11-method v1.0 surface that scripts/check_method_denominator.py holds it to. Re-triage if A2A 1.1 ships."
    "feat/slimrpc-channel-moderator:spec/v1/slimrpc-channel-moderator.md:invite-to-channel extension; implemented by neither this binding nor the official crate."
    "feat/slimrpc-multicast-spec:spec/slimrpc-multicast.md:pre-versioning layout of the multicast spec, superseded by spec/v1/ on main."
    "feat/slimrpc-multicast-spec:spec/slimrpc.md:pre-versioning layout of the base spec, superseded by spec/v1/ on main."
    "fix/slimrpc-spec-myorg-to-mydomain:spec/slimrpc.md:pre-versioning layout; a naming fix against the superseded path."
)

UPDATE=0
[ "${1:-}" = "--update" ] && UPDATE=1

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
UP="$tmp/upstream"

# One clone, all branch tips, no history. `--no-single-branch` is what makes the
# branch survey possible at all; without it only `main` is fetched and the blind
# spot this rewrite exists to close would simply move.
if ! git clone --quiet --depth 1 --no-single-branch "$UPSTREAM_URL" "$UP" 2>"$tmp/clone.err"; then
    printf 'check_slimrpc_spec: upstream unreachable (network or proxy)\n' >&2
    sed 's/^/  /' "$tmp/clone.err" >&2
    printf '  nothing was compared, so nothing is being reported as agreeing\n' >&2
    exit 3
fi

spec_files_on() { git -C "$UP" ls-tree -r --name-only "$1" | grep -E '^spec/.*\.md$' || true; }

mapfile -t MAIN_SPECS < <(spec_files_on origin/main)
if [ "${#MAIN_SPECS[@]}" -eq 0 ]; then
    printf 'check_slimrpc_spec: upstream main has no spec/**/*.md at all\n' >&2
    printf '  the layout moved; this check needs updating before it can mean anything\n' >&2
    exit 1
fi

drift=0

# Computed once. Deriving it inside a test with `... | grep -q` instead cost a
# false positive on the first run of this rewrite: `grep -q` exits at the first
# match, the producer ahead of it dies of SIGPIPE, and `pipefail` reports the
# whole pipeline as failed — so a vendored file that *was* present read as
# withdrawn upstream. A check that fails on a correct tree is worse than none.
MAIN_BASENAMES=()
for path in "${MAIN_SPECS[@]}"; do MAIN_BASENAMES+=("$(basename "$path")"); done

contains() {
    local needle="$1"; shift
    local item
    for item in "$@"; do [ "$item" = "$needle" ] && return 0; done
    return 1
}

# Two upstream paths sharing a basename would silently overwrite each other in
# the flat vendor directory, so refuse rather than vendor one over the other.
if [ "$(printf '%s\n' "${MAIN_BASENAMES[@]}" | sort -u | wc -l)" -ne "${#MAIN_SPECS[@]}" ]; then
    printf 'check_slimrpc_spec: upstream main has two spec files with one basename\n' >&2
    printf '%s\n' "${MAIN_SPECS[@]}" | sed 's/^/  /' >&2
    exit 1
fi

if [ "$UPDATE" = "1" ]; then
    mkdir -p "$VENDOR_DIR"
    for path in "${MAIN_SPECS[@]}"; do
        name="$(basename "$path")"
        git -C "$UP" show "origin/main:$path" > "$VENDOR_DIR/$name"
        printf '  updated %s\n' "$name"
    done
    printf '\nRe-vendored %d file(s) from upstream main. Update the hashes in %s\n' \
        "${#MAIN_SPECS[@]}" "spec/slimrpc_v1/README.md"
    printf 'and review the diff before committing.\n'
    exit 0
fi

# ── 1. Every spec file on main is vendored, and matches ──────────────────────
for path in "${MAIN_SPECS[@]}"; do
    name="$(basename "$path")"
    local_file="$VENDOR_DIR/$name"

    if [ ! -f "$local_file" ]; then
        printf 'check_slimrpc_spec: upstream main carries %s, which is NOT vendored\n' "$path" >&2
        printf '  this is the case the previous version of this check could not see\n' >&2
        drift=1
        continue
    fi

    git -C "$UP" show "origin/main:$path" > "$tmp/$name"
    if ! cmp -s "$local_file" "$tmp/$name"; then
        printf 'check_slimrpc_spec: %s differs from upstream\n' "$name" >&2
        printf '  vendored: %s\n' "$(sha256sum "$local_file" | cut -d' ' -f1)" >&2
        printf '  upstream: %s\n' "$(sha256sum "$tmp/$name" | cut -d' ' -f1)" >&2
        printf '  diff (vendored -> upstream):\n' >&2
        diff -u "$local_file" "$tmp/$name" | head -60 >&2 || true
        drift=1
    fi
done

# ── 2. Nothing vendored has been dropped upstream ────────────────────────────
for local_file in "$VENDOR_DIR"/*.md; do
    name="$(basename "$local_file")"
    [ "$name" = "README.md" ] && continue
    if ! contains "$name" "${MAIN_BASENAMES[@]}"; then
        printf 'check_slimrpc_spec: %s is vendored but no longer exists on upstream main\n' "$name" >&2
        printf '  it was renamed, moved or withdrawn; the binding may be implementing a\n' >&2
        printf '  document that upstream has retracted\n' >&2
        drift=1
    fi
done

# ── 3. Branch-only spec files must have been triaged ─────────────────────────
untriaged=0
for ref in $(git -C "$UP" for-each-ref --format='%(refname:short)' refs/remotes/origin \
             | grep -v '^origin/HEAD$' | grep -v '^origin/main$'); do
    branch="${ref#origin/}"
    for path in $(spec_files_on "$ref"); do
        # Only files main does not have; a branch tracking main is not news.
        contains "$path" "${MAIN_SPECS[@]}" && continue
        known=0
        for entry in "${KNOWN_BRANCH_SPECS[@]}"; do
            [ "${entry%%:*}" = "$branch" ] || continue
            rest="${entry#*:}"
            [ "${rest%%:*}" = "$path" ] && { known=1; break; }
        done
        if [ "$known" = "0" ]; then
            printf 'check_slimrpc_spec: untriaged spec upstream — %s on %s\n' "$path" "$branch" >&2
            untriaged=1
        fi
    done
done

if [ "$untriaged" = "1" ]; then
    printf '\nUpstream is developing a specification this repository has never looked at.\n' >&2
    printf 'That is how spec/v1/slimrpc-collaborative-channel.md went unnoticed while the\n' >&2
    printf 'official a2a-slimrpc crate implemented it.\n\n' >&2
    printf '  1. Read the spec on that branch.\n' >&2
    printf '  2. Decide whether bindings/a2a-protocol-slimrpc should follow it.\n' >&2
    printf '  3. Add it to KNOWN_BRANCH_SPECS in this script with that decision, and\n' >&2
    printf '     record the reasoning where a user would look for it.\n' >&2
    drift=1
fi

if [ "$drift" = "1" ]; then
    printf '\nThe SLIMRPC binding claims to implement this specification. Upstream has\n' >&2
    printf 'moved, so that claim is now unverified.\n\n' >&2
    printf '  1. Read the diff above.\n' >&2
    printf '  2. Decide whether bindings/a2a-protocol-slimrpc must follow.\n' >&2
    printf '  3. ./scripts/check_slimrpc_spec.sh --update, refresh the hashes in\n' >&2
    printf '     spec/slimrpc_v1/README.md, and record the decision in the same commit.\n' >&2
    exit 1
fi

printf 'check_slimrpc_spec: %d file(s) on upstream main, all vendored and matching; ' \
    "${#MAIN_SPECS[@]}"
printf '%d branch-only spec(s), all triaged\n' "${#KNOWN_BRANCH_SPECS[@]}"
