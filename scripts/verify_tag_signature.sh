#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)
#
# Verifies that a release tag is signed by a key this repository trusts.
#
# Until 2026-10-08 the release workflow required an *annotated* tag and
# nothing more: a tagger name and a date, neither of which anyone has to prove.
# SECURITY.md called that "a known gap". This closes it. A release tag must now
# carry an SSH or OpenPGP signature that verifies against a key committed under
# .github/release-signers/:
#
#   allowed_signers        SSH keys, in git's `gpg.ssh.allowedSignersFile`
#                          format (`<email> namespaces="git" <key-type> <key>`)
#   gpg-fingerprints.txt   the OpenPGP primary-key fingerprints trusted to sign
#   *.asc                  the OpenPGP public keys those fingerprints name
#
# An OpenPGP key is trusted by fingerprint, not by being present: importing a
# key file proves nothing about whose it is, so an .asc with no matching line
# in gpg-fingerprints.txt cannot sign a release.
#
# **Fail closed.** No trusted key configured, a tag with no signature, a
# signature by any other key, an expired or revoked key: every one is a
# refusal. There is no override flag. Adding a key is a reviewed commit.
#
# Usage:
#   scripts/verify_tag_signature.sh <tag>        verify a tag in this repository
#   scripts/verify_tag_signature.sh --self-test  prove the verifier can fail
#
# Env: SIGNERS_DIR overrides .github/release-signers (the self-test uses it).
#
# Exit: 0 verified; 1 refused; 2 usage or configuration error.

set -Eeuo pipefail

die() { printf 'verify_tag_signature: %s\n' "$*" >&2; exit 2; }
refuse() { printf '::error::verify_tag_signature: %s\n' "$*" >&2; exit 1; }

# Prints the trusted fingerprints, one per line, upper-case, comments dropped.
trusted_fingerprints() {
    local f="$1/gpg-fingerprints.txt"
    [ -f "$f" ] || return 0
    sed -e 's/#.*//' -e 's/[[:space:]]//g' "$f" | tr '[:lower:]' '[:upper:]' | grep -E '^[0-9A-F]{40}$' || true
}

# Prints the non-comment lines of allowed_signers.
ssh_signers() {
    local f="$1/allowed_signers"
    [ -f "$f" ] || return 0
    grep -vE '^[[:space:]]*(#|$)' "$f" || true
}

verify() {
    local tag="$1" dir="$2" kind body
    kind=$(git cat-file -t "refs/tags/$tag" 2>/dev/null || echo missing)
    [ "$kind" = "tag" ] || refuse "'$tag' is not an annotated tag (git cat-file -t says '$kind')"
    body=$(git cat-file tag "refs/tags/$tag")

    if grep -q -- '-----BEGIN SSH SIGNATURE-----' <<<"$body"; then
        [ -n "$(ssh_signers "$dir")" ] ||
            refuse "'$tag' is SSH-signed, but $dir/allowed_signers lists no key"
        # git picks the verifier from the armour; the allowed-signers file is
        # the only trust root. ssh-keygen also checks the key's validity window.
        git -c gpg.ssh.allowedSignersFile="$dir/allowed_signers" verify-tag "$tag" >/dev/null 2>&1 ||
            refuse "'$tag' carries an SSH signature that does not verify against $dir/allowed_signers"
        printf "verify_tag_signature: '%s' is SSH-signed by a key in %s\n" "$tag" "$dir/allowed_signers"
        return 0
    fi

    if grep -q -- '-----BEGIN PGP SIGNATURE-----' <<<"$body"; then
        local trusted gnupg status fpr primary
        trusted=$(trusted_fingerprints "$dir")
        [ -n "$trusted" ] || refuse "'$tag' is OpenPGP-signed, but $dir/gpg-fingerprints.txt lists no fingerprint"
        gnupg=$(mktemp -d)
        # shellcheck disable=SC2064
        trap "rm -rf '$gnupg'" RETURN
        shopt -s nullglob
        local keys=("$dir"/*.asc)
        shopt -u nullglob
        [ "${#keys[@]}" -gt 0 ] || refuse "'$tag' is OpenPGP-signed, but $dir has no *.asc public key"
        GNUPGHOME="$gnupg" gpg --batch --quiet --import "${keys[@]}" 2>/dev/null ||
            die "could not import the public keys in $dir"
        # --raw gives gpg's machine-readable status lines. VALIDSIG appears only
        # for a good signature by a key that is neither expired nor revoked.
        status=$(GNUPGHOME="$gnupg" git verify-tag --raw "$tag" 2>&1) ||
            refuse "'$tag' carries an OpenPGP signature that does not verify"
        if grep -qE '^\[GNUPG:\] (EXPKEYSIG|REVKEYSIG|EXPSIG|BADSIG|ERRSIG)' <<<"$status"; then
            refuse "'$tag': gpg reports $(grep -oE '(EXPKEYSIG|REVKEYSIG|EXPSIG|BADSIG|ERRSIG)' <<<"$status" | head -1)"
        fi
        # VALIDSIG <signing-key-fpr> <date> <ts> <expire> <ver> <res> <pk-algo>
        #          <hash-algo> <class> <primary-key-fpr>
        fpr=$(awk '$2 == "VALIDSIG" { print $3 }' <<<"$status" | head -1)
        primary=$(awk '$2 == "VALIDSIG" { print $12 }' <<<"$status" | head -1)
        [ -n "$fpr" ] || refuse "'$tag': gpg reported no VALIDSIG"
        if grep -qxF "${primary:-$fpr}" <<<"$trusted" || grep -qxF "$fpr" <<<"$trusted"; then
            printf "verify_tag_signature: '%s' is OpenPGP-signed by %s, listed in %s\n" "$tag" "${primary:-$fpr}" "$dir/gpg-fingerprints.txt"
            return 0
        fi
        refuse "'$tag' is signed by ${primary:-$fpr}, which is not in $dir/gpg-fingerprints.txt"
    fi

    refuse "'$tag' is annotated but not signed. Sign it: git tag -s $tag -m \"Release $tag\" (RELEASING.md)"
}

# ── self-test ────────────────────────────────────────────────────────────────
#
# A verifier that cannot fail proves nothing. This builds a throwaway
# repository, signs tags with throwaway SSH and OpenPGP keys, and checks the
# verdict on every case the release gate depends on.

expect() {
    local want="$1" name="$2"; shift 2
    local got=0
    ( "$@" ) >/dev/null 2>&1 || got=$?
    if [ "$got" -eq "$want" ]; then
        printf '  ok    %-55s exit %s\n' "$name" "$got"
    else
        printf '  FAIL  %-55s exit %s, wanted %s\n' "$name" "$got" "$want"
        SELF_TEST_FAILED=1
    fi
}

self_test() {
    command -v ssh-keygen >/dev/null || die "self-test needs ssh-keygen"
    command -v gpg >/dev/null || die "self-test needs gpg"
    local work script
    script=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")
    work=$(mktemp -d)
    # shellcheck disable=SC2064
    trap "rm -rf '$work'" EXIT
    SELF_TEST_FAILED=0

    export GNUPGHOME="$work/gnupg" GIT_CONFIG_GLOBAL="$work/gitconfig" GIT_CONFIG_NOSYSTEM=1
    mkdir -m 700 "$GNUPGHOME"
    : > "$GIT_CONFIG_GLOBAL"
    git init -q "$work/repo"
    cd "$work/repo"
    git config user.name "Release Tester"
    git config user.email "release@example.test"
    git commit -q --allow-empty -m init

    ssh-keygen -q -t ed25519 -N '' -C trusted -f "$work/ssh_trusted"
    ssh-keygen -q -t ed25519 -N '' -C other -f "$work/ssh_other"
    for who in trusted other; do
        gpg --batch --quiet --passphrase '' --quick-gen-key "$who <$who@example.test>" ed25519 sign 1y 2>/dev/null
    done
    local fpr_trusted fpr_other
    fpr_trusted=$(gpg --batch --with-colons --list-keys trusted@example.test | awk -F: '$1=="fpr"{print $10; exit}')
    fpr_other=$(gpg --batch --with-colons --list-keys other@example.test | awk -F: '$1=="fpr"{print $10; exit}')

    local good="$work/signers" empty="$work/empty" alien="$work/alien"
    mkdir "$good" "$empty" "$alien"
    printf 'release@example.test namespaces="git" %s\n' "$(cut -d' ' -f1,2 "$work/ssh_trusted.pub")" > "$good/allowed_signers"
    printf '# the trusted release key\n%s\n' "$fpr_trusted" > "$good/gpg-fingerprints.txt"
    gpg --batch --armor --export trusted@example.test > "$good/trusted.asc"
    # Both keys present as files, only one trusted by fingerprint.
    cp "$good/allowed_signers" "$alien/allowed_signers"
    printf '%s\n' "$fpr_trusted" > "$alien/gpg-fingerprints.txt"
    gpg --batch --armor --export other@example.test > "$alien/other.asc"

    git tag -a v0-unsigned -m unsigned
    git tag v0-lightweight
    git -c gpg.format=ssh -c user.signingkey="$work/ssh_trusted" tag -s v0-ssh-trusted -m ssh
    git -c gpg.format=ssh -c user.signingkey="$work/ssh_other" tag -s v0-ssh-other -m ssh
    git -c user.signingkey="$fpr_trusted" tag -s v0-gpg-trusted -m gpg
    git -c user.signingkey="$fpr_other" tag -s v0-gpg-other -m gpg

    printf 'verify_tag_signature --self-test\n'
    expect 0 "SSH tag by the trusted key"             env SIGNERS_DIR="$good" "$script" v0-ssh-trusted
    expect 0 "OpenPGP tag by the trusted key"         env SIGNERS_DIR="$good" "$script" v0-gpg-trusted
    expect 1 "SSH tag by another key"                 env SIGNERS_DIR="$good" "$script" v0-ssh-other
    expect 1 "OpenPGP tag by another key"             env SIGNERS_DIR="$good" "$script" v0-gpg-other
    expect 1 "OpenPGP key imported but not trusted"   env SIGNERS_DIR="$alien" "$script" v0-gpg-other
    expect 1 "annotated but unsigned tag"             env SIGNERS_DIR="$good" "$script" v0-unsigned
    expect 1 "lightweight tag"                        env SIGNERS_DIR="$good" "$script" v0-lightweight
    expect 1 "no trusted keys configured (SSH)"       env SIGNERS_DIR="$empty" "$script" v0-ssh-trusted
    expect 1 "no trusted keys configured (OpenPGP)"   env SIGNERS_DIR="$empty" "$script" v0-gpg-trusted
    expect 1 "tag that does not exist"                env SIGNERS_DIR="$good" "$script" v0-missing
    [ "$fpr_other" != "$fpr_trusted" ] || { echo "  FAIL  the two test keys share a fingerprint"; SELF_TEST_FAILED=1; }

    if [ "$SELF_TEST_FAILED" -ne 0 ]; then
        printf 'verify_tag_signature: self-test FAILED\n' >&2
        exit 1
    fi
    printf 'verify_tag_signature: self-test passed (10 cases)\n'
}

case "${1:-}" in
    --self-test) self_test ;;
    ""|-h|--help) sed -n '2,/^set -E/p' "${BASH_SOURCE[0]}" | sed '$d'; [ -n "${1:-}" ] || exit 2 ;;
    -*) die "unknown option '$1'" ;;
    *)
        repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || die "not inside a git repository"
        verify "$1" "${SIGNERS_DIR:-$repo_root/.github/release-signers}"
        ;;
esac
