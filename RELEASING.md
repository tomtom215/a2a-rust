<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Releasing

This document describes the release process for the `a2a-rust` workspace.

## Prerequisites

- Commit access to `main`
- `CARGO_REGISTRY_TOKEN` secret configured in the `crates-io` GitHub environment
- All CI checks passing on `main`
- **`protoc`** installed locally (required for `--all-features` builds that enable the `grpc` feature). Install via `apt-get install protobuf-compiler` (Debian/Ubuntu), `brew install protobuf` (macOS), or download from the [protobuf releases page](https://github.com/protocolbuffers/protobuf/releases)

## Workspace crate dependency order

Publishing must happen in topological order of **all** dependency edges —
including dev-dependencies, because `cargo publish` keeps versioned
`path + version` dev-dependencies in the published manifest and resolves
them against the registry:

1. `a2a-protocol-types` — no workspace dependencies
2. `a2a-protocol-server` — depends on `a2a-protocol-types`
3. `a2a-protocol-client` — depends on `a2a-protocol-types`; **dev-depends on
   `a2a-protocol-server`** (integration tests), so server must already be on
   crates.io
4. `a2a-protocol-sdk` — depends on all three

This matches the order used by `.github/workflows/release.yml`. Publishing
client before server fails: the client's versioned dev-dependency on the
not-yet-published server cannot be resolved from the index.

## Release checklist

### 1. Prepare the release

```bash
# Create a release branch
git checkout -b release/vX.Y.Z main

# Update version in all 4 crate Cargo.toml files (must all match)
# crates/a2a-protocol-types/Cargo.toml
# crates/a2a-protocol-client/Cargo.toml
# crates/a2a-protocol-server/Cargo.toml
# crates/a2a-protocol-sdk/Cargo.toml

# Update CHANGELOG.md: move [Unreleased] content to [X.Y.Z] with date
# (the heading must be `## [X.Y.Z] - YYYY-MM-DD` — the release workflow
# rejects undated headings). Add new empty [Unreleased] section.

# Update CITATION.cff: set `version` and `date-released` to the new release
# (validated against the tag by the release workflow)

# Update SECURITY.md: make sure the Supported Versions table covers the
# new minor line (validated by the release workflow)

# Verify everything builds and passes
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Verify packaging (mirror the exclude list in ci.yml / release.yml exactly)
cargo package --workspace --exclude echo-agent --exclude agent-team --exclude multi-lang-team --exclude rig-a2a-agent --exclude genai-a2a-agent --exclude incident-response --exclude a2a-tck --exclude a2a-tck-sut --exclude a2a-benchmarks
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

## Path to 1.0.0

All four crates are pre-1.0 despite a multi-release history (`a2a-protocol-server`
is at 0.8.0; the rest at 0.7.0 as of this writing). Nothing below is a promise
about timing — it exists so "are we ready for 1.0" has a checklist instead of
a feeling, and so this is answered before, not during, any external review
(donation, security audit, or otherwise) that asks for it.

### What 1.0.0 commits to

Per [Semantic Versioning](https://semver.org/spec/v2.0.0.html), reaching
1.0.0 is a promise: **no breaking change to public API, wire format, or
documented behavior without a major version bump.** Pre-1.0, this project
already tries to avoid gratuitous breaks (see the deliberate 0.8.0 bump on
`a2a-protocol-server` for a real semver break, rather than folding it into a
patch release) — 1.0.0 is where that stops being best-effort and starts being
the contract.

### Criteria to reach 1.0.0

All of the following, not some:

- **Official TCK: no unresolved MUST-level failures**, and the SKIPPED/NOT
  TESTED gap is understood and documented (not necessarily zero — see
  `docs/official-tck-findings.md` §16 — some of it is a suite limitation,
  not this project's to close). This bar is already met as of this writing;
  keeping it met through 1.0.0 is the requirement, not reaching it.
- **Coverage does not regress** below its current measured floor on
  `crates/*/src` (94% lines / 94% regions / 92% functions at the time of
  writing — see `codecov.yml`'s `project` status, which already gates on
  this at PR time).
- **Mutation score**: the weekly full sweep (`mutants.yml`) is clean —
  zero surviving mutants workspace-wide — for at least one full sweep
  immediately before tagging, not just the incremental per-PR gate.
- **No known `P0`/`P1` open issues** against any of the four published
  crates.
- **API surface review**: a deliberate pass over every `pub` item in all
  four crates asking "do we want to support this shape forever" — not just
  "does it compile and have a doc comment." This is the one criterion that
  is inherently a judgment call, not a metric; it should be its own PR,
  reviewable on its own.
- **This section itself has been re-read and still describes the actual
  bar** — a 1.0 criteria list nobody revisits is exactly the kind of stale
  claim this project treats as a bug elsewhere (see the correction notices
  in `docs/official-tck-findings.md`).

### Deprecation policy (post-1.0)

Once 1.0.0 ships, removing or changing public API follows this sequence —
this section takes effect at that point, not before (pre-1.0, breaking
changes ship in a minor bump with a CHANGELOG entry, as today):

1. **Mark it.** `#[deprecated(since = "X.Y.0", note = "...")]` on the item,
   pointing at its replacement if one exists. Ship in a minor release.
2. **Document it.** A CHANGELOG entry under `Deprecated`, and a note in the
   relevant book page if the item is covered there.
3. **Keep it working.** A deprecated item must not change behavior or be
   removed for at least **one minor version** after the release that
   deprecated it — long enough that `cargo update` alone does not surface a
   compile error, only a warning.
4. **Remove it in a major bump.** Deletion is a breaking change by
   definition and only ships in the next `X.0.0`.

Security fixes are the one exception: a vulnerability in a deprecated (or
any) API can require immediate removal or behavior change outside this
sequence, per `SECURITY.md`. Being deprecated does not make something
exempt from a security fix, and a security fix is not required to preserve
a deprecated API's old behavior.

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
