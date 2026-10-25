<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Release signers

The keys trusted to sign release tags. `release.yml` runs on every pushed
release tag (`vX.Y.Z` or `vX.Y.Z-*`); its `Validate release tag` job runs
`scripts/verify_tag_signature.sh` and refuses the release unless the tag's
signature verifies against a key listed here, as this directory stands on
`main`.

| File | Holds |
|---|---|
| `allowed_signers` | SSH public keys, one per line, in git's `gpg.ssh.allowedSignersFile` format: `<email> namespaces="git" <key-type> <base64-key>` |
| `gpg-fingerprints.txt` | The 40-hex-digit primary-key fingerprints of trusted OpenPGP keys |
| `*.asc` | The armoured OpenPGP public keys those fingerprints name |

An OpenPGP key file here is not trusted unless its fingerprint is also listed:
presence proves nothing about whose key it is.

One key is configured: the maintainer's SSH key, `SHA256:ziO4yysBwOPEyekQKir3ahIqzkzciF+a7u/Xh7jaDpM`
(ed25519, principal `tomf@tomtomtech.net`; compare with `ssh-keygen -lf` on the
`.pub` file and with the signing keys on the maintainer's GitHub profile). The
gate accepts a tag signed by it and refuses every other. Until it was added the
gate refused every release, deliberately: the alternative is a gate that passes
unsigned tags while it waits, which is the gap it exists to close. No release
has passed the gate yet.

## Adding the maintainer's SSH key

```sh
# The key you sign with (an ed25519 key is fine; a hardware-backed sk- key is better)
printf '%s namespaces="git" %s\n' "tomf@tomtomtech.net" "$(cut -d' ' -f1,2 ~/.ssh/id_ed25519.pub)" \
  >> .github/release-signers/allowed_signers

# Sign tags with it from now on
git config gpg.format ssh
git config user.signingkey ~/.ssh/id_ed25519.pub
git config gpg.ssh.allowedSignersFile "$(git rev-parse --show-toplevel)/.github/release-signers/allowed_signers"
```

## Adding an OpenPGP key

```sh
gpg --armor --export <KEYID> > .github/release-signers/maintainer.asc
gpg --with-colons --fingerprint <KEYID> | awk -F: '$1=="fpr"{print $10; exit}' \
  >> .github/release-signers/gpg-fingerprints.txt
```

## Signing a release tag

With git set up to sign with one of the keys above (for OpenPGP, `user.signingkey` set to it):

```sh
git tag -s vX.Y.Z -m "Release vX.Y.Z"    # -s signs, and implies -a
scripts/verify_tag_signature.sh vX.Y.Z   # the check release.yml will run
```

The rest of the release procedure is in `RELEASING.md`.

## Verifying a release yourself

```sh
git fetch origin tag vX.Y.Z
scripts/verify_tag_signature.sh vX.Y.Z
```

By default the script checks against `.github/release-signers/` in your
working tree. To check against the keys `release.yml` trusts, the ones on
`main`, extract that copy and point `SIGNERS_DIR` at it:

```sh
git fetch origin main
signers=$(mktemp -d)
git archive origin/main .github/release-signers | tar -x -C "$signers"
SIGNERS_DIR="$signers/.github/release-signers" scripts/verify_tag_signature.sh vX.Y.Z
```

Only tags cut after the signing gate (2026-10-08) can pass. Every earlier
tag, v0.14.1 and before, is lightweight or annotated but unsigned, and the
script refuses it.

## What this does not protect against

`release.yml` reads these keys from `main` and refuses a tag whose commit is
not on `main`, so a commit cannot add its author's key and tag itself. But the
workflow runs from the tagged commit, so someone who can push a `v*` tag onto
an unreviewed commit can also edit the workflow that would have refused it.
Close that in the repository settings, not here: a tag ruleset on `v*` that
restricts creation, update and deletion to the maintainer, and, for
publishing, a required reviewer on the `crates-io` environment that
`release.yml`'s `publish` job runs in.

Changing this directory changes who can release. It goes through a pull
request like anything else, and a reviewer should check the key against one
the maintainer publishes elsewhere (their GitHub profile's signing keys).
