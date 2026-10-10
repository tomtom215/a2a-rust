<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Release signers

The keys trusted to sign release tags. `release.yml` runs
`scripts/verify_tag_signature.sh` on every pushed `v*` tag and refuses the
release unless the tag's signature verifies against a key listed here.

| File | Holds |
|---|---|
| `allowed_signers` | SSH public keys, one per line, in git's `gpg.ssh.allowedSignersFile` format: `<email> namespaces="git" <key-type> <base64-key>` |
| `gpg-fingerprints.txt` | The 40-hex-digit primary-key fingerprints of trusted OpenPGP keys |
| `*.asc` | The armoured OpenPGP public keys those fingerprints name |

An OpenPGP key file here is not trusted unless its fingerprint is also listed:
presence proves nothing about whose key it is.

**No key is configured yet**, so the gate refuses every release until the
maintainer commits one. That is deliberate: the alternative is a gate that
passes unsigned tags while it waits, which is the gap it exists to close.

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

## Verifying a release yourself

```sh
git fetch origin tag v0.15.0
scripts/verify_tag_signature.sh v0.15.0
```

## What this does not protect against

`release.yml` reads these keys from `main` and refuses a tag whose commit is
not on `main`, so a commit cannot add its author's key and tag itself. But the
workflow runs from the tagged commit, so someone who can push a `v*` tag onto
an unreviewed commit can also edit the workflow that would have refused it.
Close that in the repository settings, not here: a tag ruleset on `v*` that
restricts creation, update and deletion to the maintainer, and, for
publishing, the required reviewer on the crates.io environment.

Changing this directory changes who can release. It goes through a pull
request like anything else, and a reviewer should check the key against one
the maintainer publishes elsewhere (their GitHub profile's signing keys).
