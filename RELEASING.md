<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Releasing

This document describes the release process for the `a2a-rust` workspace.

## Prerequisites

- Commit access to `main`
- `CARGO_REGISTRY_TOKEN` secret configured in the `crates-io` GitHub environment
- All CI checks passing on `main`

## Workspace crate dependency order

Publishing must happen in this order (each crate depends on the ones above it):

1. `a2a-protocol-types` — no workspace dependencies
2. `a2a-protocol-client` — depends on `a2a-protocol-types`
3. `a2a-protocol-server` — depends on `a2a-protocol-types`
4. `a2a-protocol-sdk` — depends on all three

## Release checklist

### 1. Prepare the release

```bash
# Create a release branch
git checkout -b release/vX.Y.Z main

# Update version in all 4 crate Cargo.toml files (must all match)
# crates/a2a-types/Cargo.toml
# crates/a2a-client/Cargo.toml
# crates/a2a-server/Cargo.toml
# crates/a2a-sdk/Cargo.toml

# Update CHANGELOG.md: move [Unreleased] content to [X.Y.Z] with date
# Add new empty [Unreleased] section

# Verify everything builds and passes
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Verify packaging
cargo package --workspace --exclude echo-agent --exclude agent-team --exclude multi-lang-team --exclude rig-a2a-agent --exclude genai-a2a-agent --exclude a2a-tck
```

### 2. Merge to main

```bash
git add -A && git commit -m "chore: prepare release vX.Y.Z"
# Open PR, get review, merge to main
```

### 3. Tag and push

```bash
git checkout main && git pull
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

This triggers the release workflow (`.github/workflows/release.yml`) which:

1. **Validates** that all 4 crate versions match the tag and CHANGELOG entry exists
2. **Runs CI** (fmt, clippy, test, doc, MSRV check) and **security audit** (cargo-deny)
3. **Packages** all crates with SLSA build provenance attestation
4. **Runs a publish dry run** to verify packages are publishable
5. **Creates a GitHub Release** with notes extracted from CHANGELOG.md and attached `.crate` artifacts
6. **Publishes to crates.io** in dependency order with index propagation delays (requires `crates-io` environment approval)

### 4. Post-release

- Verify all 4 crates appear on [crates.io](https://crates.io)
- Verify docs build on [docs.rs](https://docs.rs)
- Announce release if appropriate

## Versioning

This project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

All four workspace crates share the same version number and are always released
together. The example crates (`echo-agent`, `agent-team`, `multi-lang-team`,
`rig-a2a-agent`, `genai-a2a-agent`) and the `a2a-tck` binary are `publish = false`
and are never published.

## Troubleshooting

### Publish fails mid-way

If publishing fails after some crates are already published:

1. Fix the issue
2. Bump the patch version for all crates
3. Update CHANGELOG.md
4. Tag and push the new version

You cannot re-publish the same version to crates.io.

### Version mismatch

The release workflow validates that all 4 crate versions match the Git tag.
If they don't match, the workflow fails immediately. Fix the versions and re-tag.
