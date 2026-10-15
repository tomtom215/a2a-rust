<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Packaging drafts for the `a2a` CLI

**Status: drafts, not submitted.** Nothing here is published anywhere. The
maintainer decides whether to distribute the CLI beyond `cargo build`, and
through which channels (decision of 2026-10-08, recorded in
`docs/handoff.md`).

| File | Channel | What has to happen first |
|---|---|---|
| `homebrew/a2a.rb` | A Homebrew tap the maintainer owns (no tap exists) | Release binaries attached to a GitHub release |
| `winget/*.yaml` | `microsoft/winget-pkgs`, by pull request | The same, for `x86_64-pc-windows-msvc` |

The formula covers Apple-silicon macOS (`aarch64-apple-darwin`) and Linux
on x86_64 and arm64. It has no Intel-macOS entry, because `release.yml`
builds no `x86_64-apple-darwin` binary.

No release can produce these binaries yet. `release.yml` refuses any `v*`
tag not signed by a key committed under `.github/release-signers/`, and
none is committed (see that directory's README).

## Turning the drafts into releases

1. **Commit a release-signing key** to `.github/release-signers/`.
2. **Turn on the binaries.** Set the repository variable
   `PUBLISH_CLI_BINARIES=true`. The next release tag then builds `a2a` for
   four targets in `release.yml` (`cli-binaries`): `x86_64-unknown-linux-gnu`,
   `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin` and
   `x86_64-pc-windows-msvc`. Each archive (`.zip` for Windows, `.tar.gz`
   otherwise) gets a `.sha256` file and SLSA build provenance, and is
   attached to the GitHub release.
3. **Fill in the version and checksums** from that release's `.sha256`
   files. Each placeholder is written `@NAME@`.
4. **Verify an archive before trusting its checksum:**
   `gh attestation verify a2a-vX.Y.Z-<target>.tar.gz -R tomtom215/a2a-rust`
   (`.zip` for the Windows archive).
5. **Publish.**
   - **Homebrew:** create `tomtom215/homebrew-tap` and add the formula as
     `Formula/a2a.rb`.
   - **winget:** open a pull request to `microsoft/winget-pkgs` with the
     three manifests under `manifests/t/TomF/A2ACli/X.Y.Z/`.
     `wingetcreate` can generate and submit them from the release URL
     instead.

## What these drafts were checked against

They were written against the schemas named in each file. Neither has been
installed or validated by `brew audit` or `winget validate`. Neither tool
runs in the environment these drafts were written in.
