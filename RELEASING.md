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

### What this means for a local pre-flight

Use `cargo package --workspace`, **not** `cargo package -p <crate>`. The
workspace form resolves sibling `path + version` dependencies against the local
crates; the per-crate form resolves them against the crates.io index, which does
not yet carry the new version, so it fails with:

```
failed to select a version for the requirement `a2a-protocol-types = "^0.9.0"`
candidate versions found which didn't match: 0.8.0, 0.7.0, ...
```

That is not a broken manifest — it is the per-crate form asking the registry a
question only the registry can answer after publication. `ci.yml` and
`release.yml` both use the workspace form for exactly this reason.

Every `publish = false` member must be `--exclude`d, because such crates depend
on their siblings by bare `path` with no version, and packaging rejects that.
The list is duplicated across `ci.yml`, `release.yml` and this file, so adding a
new example silently breaks packaging — which is what
`scripts/check_package_excludes.py` now prevents.

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
cargo package --workspace --exclude a2a-example-harness --exclude hello-agent --exclude deploy-agent --exclude a2a-book-tests --exclude echo-agent --exclude agent-team --exclude multi-lang-team --exclude rig-a2a-agent --exclude genai-a2a-agent --exclude incident-response --exclude a2a-tck --exclude a2a-tck-sut --exclude a2a-benchmarks
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

> **Known gap — the first ten tags do not match this step.** Ten release
> tags (`v0.2.0` … `v0.7.0`) are *lightweight*: bare refs to a commit,
> with no tagger, no date, and no signature. `git cat-file -t v0.7.0` prints
> `commit`, not `tag`. The `-a` above was documented but not applied in
> practice — creating a release through the GitHub UI produces a lightweight
> tag, which is the likely cause.
>
> Consequences, so nobody assumes more than is true:
> * A tag alone does not attest who cut the release, or when.
> * Nothing here is GPG/SSH-signed, so `git tag -v` cannot verify any release.
> * Adopters needing a verifiable link from a version to this repository must
>   use the build provenance attestations in [`PROVENANCE.md`](PROVENANCE.md),
>   which *are* signed, rather than the tag.
>
> Using `-a` as written fixes this for future releases; it does not
> retroactively fix the ten existing tags, and re-tagging published releases
> would move refs that downstreams may already pin. Adopting signed tags
> (`git tag -s`) is a separate, unmade decision — it needs a maintainer key
> and a documented way for adopters to obtain it. Tracked in
> [`ROADMAP.md`](ROADMAP.md).
>
> **Enforced since 2026-08-10.** The `-a` above was an instruction with
> nothing behind it, which is how ten lightweight tags got pushed past a
> documented step. `release.yml`'s validate job now runs `git cat-file -t` on
> the pushed tag and fails the release if it is not a `tag` object, with the
> delete-and-recreate commands in the error. Creating the release through the
> GitHub UI will now stop the workflow rather than quietly produce an
> eleventh lightweight tag.
>
> **And it worked.** The two releases cut since — `v0.8.0` and `v0.9.0` — are
> both annotated tag objects, the first two in this project's history that
> record a tagger and a date. Neither is signed; that half is still open.
>
> That check deliberately does **not** require a signature. A gate for a key
> that does not exist could never fail, and would read as signing coverage
> this project does not have. When the key decision above is made, tightening
> this check to `git tag -v` is the one-line follow-up.

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
together. The example crates (`hello-agent`, `echo-agent`, `agent-team`,
`multi-lang-team`, `rig-a2a-agent`, `genai-a2a-agent`), the `a2a-book-tests`
crate and the `a2a-tck` binary are `publish = false` and are never published.

### `a2a-protocol-slimrpc` releases separately

The SLIMRPC binding is publishable but is **not** part of the tagged release
above, and `release.yml` does not touch it. It lives outside the workspace with
its own `Cargo.lock`, so it needs its own `cargo package` and `cargo publish`
run from `bindings/a2a-protocol-slimrpc/`.

It is versioned independently — `0.3.0` against the SDK's `0.11.0`. Numbering it
to match would claim API stability it has not earned and force a bump on every
SDK release even when nothing in it changed.

Independence covers the numbers, not the schedule, and this is the part that is
easy to get wrong:

> `SlimRpcServer::builder` takes `Arc<RequestHandler>` and `agent_interface()`
> returns an `AgentInterface`, so `a2a-protocol-server` and
> `a2a-protocol-types` are **public dependencies**. Its requirement on them is
> therefore a tight `0.11`, not a range — allow two and cargo links both, and
> callers get `expected RequestHandler, found RequestHandler`.

So **every SDK minor release requires a follow-up release of the binding**:

1. Publish the four SDK crates as normal.
2. Bump the binding's `a2a-protocol-*` requirements to the new minor.
3. Bump the binding's own version (minor, since its supported SDK changed).
4. From `bindings/a2a-protocol-slimrpc/`: `cargo package` then `cargo publish`.

Skipping step 4 leaves the newest binding on crates.io pinned to a superseded
SDK, which is the failure mode this note exists to prevent.

#### The window between step 1 and step 2, and why CI stays green across it

The version bump that must precede the tag is the same commit that puts the
binding out of resolution, so between the release-prep commit and publication
there is no pin value the binding can hold:

| pin | in-tree | build / clippy / test | `cargo package` |
| --- | --- | --- | --- |
| `0.10` | 0.10.0 | pass | fails — 0.10.0 not on the index yet |
| `0.9` | 0.10.0 | **fails** — didn't match 0.10.0 | fails |

Locally the `path` wins, so the binding builds and tests against the in-tree
crates either way; `cargo package` strips the path, and the requirement then
resolves against crates.io. Reverting the pin does not rescue it — it breaks the
build instead — and a range is refused for the public-dependency reason above.

`ci.yml` therefore runs `scripts/package_binding.py` rather than `cargo package`
directly. It skips **registry resolution alone** when every pin names the
version that is in the tree and that version is absent from the index, proves
the rest of packaging with `cargo package --list`, and fails on everything else
— including a pin naming a version that is neither in-tree nor published. The
skip is annotated as a warning on the job, not hidden, and it closes by itself
once step 4 publishes.

Nothing about this changes the order: step 2 still follows step 1. What it
changes is that the release-prep commit is now a commit CI can pass.

## Path to 1.0.0

All four crates are pre-1.0 despite a multi-release history (all four are at
0.11.0 as of this writing). Nothing below is a promise
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
