<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Releasing

This document describes the release process for the `a2a-rust` workspace.

## Prerequisites

- Commit access to `main`
- **crates.io Trusted Publishing** configured for each of the four crates
  (one-time, per crate, by a crate owner): on crates.io open the crate →
  *Settings* → *Trusted Publishing* → *Add a new GitHub publisher* with
  repository owner `tomtom215`, repository `a2a-rust`, workflow filename
  `release.yml`, environment `crates-io`. The `publish` job then exchanges
  its GitHub OIDC token for a short-lived crates.io token
  (`rust-lang/crates-io-auth-action`); no long-lived secret is involved.
- Until every crate has a trusted publisher, the `CARGO_REGISTRY_TOKEN`
  secret in the `crates-io` GitHub environment is the fallback. The job
  prints a warning when it falls back and fails if neither credential is
  available. Delete the secret once the fallback has not been used for a
  release.
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
#
# ...and the inter-crate *dependency pins*, which are eight further version
# strings in those same four files and are NOT what release.yml checks — it
# reads only the first `^version` line per manifest. Leaving them stale does
# not fail the release and does not fail the build, because `version =
# "X.Y.Z"` means `^X.Y.Z`. What it does is publish, say, an sdk X.Y.Z+1 that
# declares a dependency on server X.Y.Z, so a consumer who bumps only the sdk
# against an existing lockfile keeps the old server and never receives the
# fix. On a patch release whose whole content is a server fix, that defeats
# the release. Find them all with:
#     git grep -n 'a2a-protocol-\(types\|client\|server\|sdk\)\s*=' -- crates
#
# Then refresh the THREE lockfiles that carry the crate versions:
#     cargo metadata --format-version 1 >/dev/null          # root workspace
#     (cd bindings/a2a-protocol-slimrpc && cargo metadata --format-version 1 >/dev/null)
#     (cd itk && cargo metadata --format-version 1 >/dev/null)
# This said "BOTH" and named two until 2026-09-24, by which point
# itk/Cargo.lock pinned 0.11.0 against 0.13.0 manifests and the upstream
# a2a-itk harness (which builds with --locked) could not build our agent.
# scripts/check_lockfiles.sh checks every tracked lockfile and runs in CI.

# On a MINOR release, the dependency snippets in prose move too — every
# `a2a-protocol-* = "X.Y"` a reader is told to copy, in the root README, the
# crates README, the book and two module docs. They name the release *line*,
# not the patch, so a patch release changes none of them. This one is checked:
#     python3 scripts/check_doc_versions.py
# It fails CI until they match and names every site, so it is a to-do list
# rather than something to remember. It went unchecked until 0.12.1, by which
# point 28 snippets across 14 files named 0.7, 0.8 or 0.11.

# Update ROADMAP.md's "Current release:" line, and add a section to
# book/src/reference/changelog.md — neither is checked by anything, and both
# have rotted before.

# Update CHANGELOG.md: move [Unreleased] content to [X.Y.Z] with date
# (the heading must be `## [X.Y.Z] - YYYY-MM-DD` — the release workflow
# rejects undated headings). Add new empty [Unreleased] section.

# Update CITATION.cff: set `version` and `date-released` to the new release
# (validated against the tag by the release workflow)

# Update SECURITY.md: make sure the Supported Versions table covers the
# new minor line (validated by the release workflow)

# Regenerate docs/provenance-manifest.md and commit it. THIS IS A HARD GATE:
#     scripts/provenance_manifest.sh HEAD
#
# `.github/workflows/release.yml:100` runs
# `./scripts/check_provenance_manifest.py "$GITHUB_SHA"`, and that check
# passes only when the tagged tree differs from the manifest's pinned commit
# by *nothing but the manifest itself*. `ci.yml` does not run it, so a stale
# manifest never reddens a content pull request — it fails the tag, after the
# tag exists. Check it before you tag:
#     python3 scripts/check_provenance_manifest.py "$(git rev-parse HEAD)"
#
# ORDERING CONSTRAINT — this is the part that bites. Regenerating the manifest
# must be the LAST substantive change before the tag. Anything committed after
# it (another fix, a doc touch, a bot pushing benchmark results to `main`)
# re-breaks the gate, because that file now differs from the pinned commit too.
# In practice that forces content and release prep into two pull requests in
# that order, prep second, with the manifest regenerated in the prep commit —
# which is exactly how 0.12.1 shipped (docs/handoff.md, "0.12.1"). The
# v0.11.0 tag died on 2026-08-30 to the other half of the same rule: the
# manifest was regenerated before a benchmark bot push landed, so it pinned a
# tree the release no longer had. Wait for automation to settle, regenerate,
# then tag.

# Verify everything builds and passes
cargo fmt --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Verify packaging (mirror the exclude list in ci.yml / release.yml exactly)
cargo package --workspace --exclude a2a-example-harness --exclude hello-agent --exclude deploy-agent --exclude a2a-book-tests --exclude echo-agent --exclude agent-team --exclude multi-lang-team --exclude rig-a2a-agent --exclude mcp-a2a-agent --exclude a2a-mcp-bridge --exclude genai-a2a-agent --exclude incident-response --exclude resilient-agent --exclude a2a-tck --exclude a2a-tck-sut --exclude a2a-benchmarks --exclude a2a-cli
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

1. **Validates** that all 4 crate versions match the tag and CHANGELOG entry
   exists, and that the tag publishes what its notes describe (below)
2. **Runs CI** (fmt, clippy, test, doc, MSRV check) and **security audit** (cargo-deny)
3. **Packages** all crates with SLSA build provenance attestation
4. **Runs a publish dry run** to verify packages are publishable
5. **Creates a GitHub Release** with notes extracted from CHANGELOG.md and attached `.crate` artifacts
6. **Publishes to crates.io** in dependency order with index propagation delays (requires `crates-io` environment approval; authenticates with Trusted Publishing, falling back to the environment secret)

### The tag must be the release-preparation commit

`v0.13.0` was tagged on the merge of #138 instead of on its release
preparation, and shipped nineteen changes — one breaking — that the tagged
`CHANGELOG.md` still listed under `[Unreleased]` and the release notes never
mentioned (`docs/adopter-audit-2026-09-22.md`, N7). Four checks in
`release.yml`, all in `scripts/check_release_tree.py`, now refuse that:

| Step | Refuses |
|---|---|
| `Nothing is left under [Unreleased] in the tagged tree` | any entry under `## [Unreleased]`; the placeholder `Nothing yet.` is allowed |
| `The tag is the release-preparation commit` | any file packaged into the four crates (their directories, and the root `Cargo.toml` they inherit from) that differs between the tag and the last commit that edited the release's own `## [X.Y.Z]` section — apart from the crates' version strings and pins |
| `Breaking releases keep the STABILITY.md cadence` | a `### Breaking Changes` section in a patch release, or in a second release in the same calendar month as another breaking one |
| `Packaged crates were built from the tagged commit` (package job) | a `.crate` whose `.cargo_vcs_info.json` names another commit, or a dirty tree |

What that asks of the process: **write the release notes last.** Anything that
changes packaged source after the `## [X.Y.Z]` section was last edited — a
fix merged during release preparation, a test added to kill a mutant — fails
the second check until the notes are edited again after it. That is the
point: whoever changes shipped code after the notes were written has to open
the notes, which is exactly the step 0.13.0 skipped. Bumping versions and pins
after the notes is allowed, since 0.12.1 was prepared in that order.

Before tagging, run all three tree checks on the commit you intend to tag:

```bash
for c in unreleased prep cadence; do
  python3 scripts/check_release_tree.py "$c" vX.Y.Z HEAD || echo "FAILED: $c"
done
```

`python3 scripts/check_release_tree.py history` runs them over every existing
tag. Measured 2026-09-23: `prep` fails eight of the seventeen tags (`v0.2.0`,
`v0.6.0` to `v0.10.0`, `v0.12.0`, `v0.13.0`), `unreleased` fails `v0.3.0` and
`v0.13.0`, and `cadence` fails `v0.13.0`. None of that is retroactively
fixable; it is the measurement of how often the gap was used.

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

It is versioned independently — currently `0.5.0` against the SDK's `0.13.0`. Numbering it
to match would claim API stability it has not earned and force a bump on every
SDK release even when nothing in it changed.

Independence covers the numbers, not the schedule, and this is the part that is
easy to get wrong:

> `SlimRpcServer::builder` takes `Arc<RequestHandler>` and `agent_interface()`
> returns an `AgentInterface`, so `a2a-protocol-server` and
> `a2a-protocol-types` are **public dependencies**. Its requirement on them is
> therefore a tight `0.13`, not a range — allow two and cargo links both, and
> callers get `expected RequestHandler, found RequestHandler`.

So **every SDK minor release requires a follow-up release of the binding**:

1. Publish the four SDK crates as normal.
2. Bump the binding's `a2a-protocol-*` requirements to the new minor.
3. Bump the binding's own version (minor, since its supported SDK changed).
4. From the repository root, package and verify the binding with the script
   that exists for it — **not** plain `cargo package`:

   ```sh
   python3 scripts/package_binding.py
   ```

   It packages with a config-level `[patch]` supplying the SDK crates from
   the tree, so the tarball is *built*, not merely listed, and it refuses a
   pin that does not name the in-tree version. See "The window between step 1
   and step 2" below for why the plain command cannot work here. Then, from
   `bindings/a2a-protocol-slimrpc/`, `cargo publish`.

Skipping step 4 leaves the newest binding on crates.io pinned to a superseded
SDK, which is the failure mode this note exists to prevent.

> **The first publish has to be manual and token-authenticated.** The four SDK
> crates are published by `release.yml` using a crates.io token obtained
> through Trusted Publishing, and the obvious instinct is to extend that job
> to the binding. It cannot cover the *first* publish: a trusted publisher is
> configured per crate, on that crate's own settings page on crates.io, which
> presupposes the crate exists and is owned — and this one does not exist on
> the index at all (see the note below, dated 2026-09-01). So version one goes
> up by hand with `CARGO_REGISTRY_TOKEN`, and only after that can a trusted
> publisher be configured and the step automated. **Not re-verified against
> crates.io** — re-checked 2026-09-20, the index still answers HTTP 403 through
> the sandbox proxy this file is edited behind, so confirm the Trusted
> Publishing half against crates.io's own documentation before acting on it.
> The crate's absence from the index is the dated observation below, not a
> fresh one.

> **Step 4 has never actually been run.** As of 2026-09-01 the crate does not
> exist on crates.io at all:
>
> ```sh
> curl -s https://crates.io/api/v1/crates/a2a-protocol-slimrpc
> # {"errors":[{"detail":"crate `a2a-protocol-slimrpc` does not exist"}]}
> ```
>
> Steps 1–3 have been kept up: the four SDK crates are published at `0.12.1`
> with `0.13.0` prepared, the binding's requirements read `0.13`, and its own
> version has moved `0.1.0` → `0.2.0` → `0.3.0` → `0.4.0` → `0.5.0` alongside
> them. Only the publish has never happened, through five SDK releases — the
> binding's `a2a-protocol-server` requirement has tracked `0.9` → `0.10` →
> `0.11` → `0.12` → `0.13`, which is where that count comes from.
>
> Note that on a *minor* the binding's requirement cannot lag behind: it
> depends on the workspace by `path` as well as by version, so a requirement
> of `0.12` stops resolving the moment the crates read `0.13.0`. Step 2 is
> therefore done in the release-prep commit, before publishing, and only
> step 4 waits for the SDK crates to reach crates.io. A *patch* moves
> neither, since `version = "0.12"` means `^0.12`.
>
> Every number in this section was refreshed on 2026-09-12 against the
> manifests, because they had rotted: the section said `0.3.0` against
> `0.11.0` and quoted the requirement as `0.10` while the tree held `0.4.0`,
> `0.12.0` and `0.12`. That has happened at each of the last four bumps — the
> prose ran a minor behind the manifest every time. Nothing checks it, which
> is the whole reason it rots; a check comparing these four numbers to the
> manifests would end it. The crates.io observation above was **not** re-run
> on that date (crates.io is unreachable from the sandbox this was edited in,
> HTTP 403 via its proxy), so it stands as dated: 2026-09-01.
>
> This is recorded here rather than only in
> `docs/v0.9.0-post-release-review.md`, where it was first observed at 0.9.0
> and where a release checklist reader would never see it. The failure mode the
> note above describes — "the newest binding on crates.io pinned to a
> superseded SDK" — understates it: there is no binding on crates.io to be
> pinned to anything, so nobody outside this repository can depend on it, and
> the `0.4.0` in its manifest is a number no consumer has ever seen.
>
> Two things to settle before the first publish, neither of which blocks it:
>
> * **The crate's `rust-version` is declared but unmeasured.** This used to
>   read "the crate declares no `rust-version`"; that is no longer true —
>   `bindings/a2a-protocol-slimrpc/Cargo.toml:21` reads
>   `rust-version = "1.88"`, a literal, since the crate is its own workspace
>   and inherits nothing from the root. Verify with
>   `grep -n 'rust-version' bindings/a2a-protocol-slimrpc/Cargo.toml`.
>
>   What has *not* changed is the substance of the note: 1.88 matches the
>   four SDK crates, but nothing has built this crate against 1.88 to confirm
>   it. Its true MSRV is at least 1.88 and may be higher, because the
>   `agntcy-slim-*` dependencies have their own floors. A declared MSRV that
>   has not been built against is a claim, not a fact — and a declared one is
>   worse than an absent one, because it looks checked.
> * **It has no changelog of its own**, though the comment in its manifest
>   about independent versioning assumes one. The root `CHANGELOG.md` covers
>   the four SDK crates.

#### The window between step 1 and step 2, and why CI stays green across it

The version bump that must precede the tag is the same commit that puts the
binding out of registry resolution. Without a fix there is no pin value the
binding can hold between the release-prep commit and publication:

| pin | in-tree | build / clippy / test | plain `cargo package` |
| --- | --- | --- | --- |
| `0.13` | 0.13.0 | pass | fails — 0.13.0 not on the index yet |
| `0.12` | 0.13.0 | **fails** — didn't match 0.13.0 | fails |

Locally the `path` wins, so the binding builds and tests against the in-tree
crates either way; plain `cargo package` strips the path, and the requirement
then resolves against crates.io. Reverting the pin does not rescue it — it
breaks the build instead — and a range is refused for the public-dependency
reason above.

**`ci.yml` therefore runs `scripts/package_binding.py` rather than
`cargo package` directly, and that script closes the window with Cargo's
`[patch]`, applied at the config level so the manifest is untouched:**

```sh
cargo package --allow-dirty \
  --config 'patch.crates-io.a2a-protocol-types.path="/abs/crates/a2a-protocol-types"' \
  --config 'patch.crates-io.a2a-protocol-client.path="…"' \
  --config 'patch.crates-io.a2a-protocol-server.path="…"'
```

A patch supplies a version of a crates.io crate from a path, *including a
version the index does not have* — Cargo's own "prepublishing a breaking
change" case. So the tarball is verified by building it against the in-tree
SDK, exactly what Build and Test compile against, and **the release window
stops being a state at all**: the pin names the in-tree version, the patch
supplies it, and the index is never asked. There is no skip and no warning;
the row above marked "fails" is what the *unpatched* command does, not what
the gate does.

What the patch does not do, and the script still must:

* **Refuse a pin that does not name the in-tree version, before cargo runs.**
  A patch is used only when its version satisfies the requirement, so a stale
  `0.12` against an in-tree `0.13.0` leaves the patch unused; cargo merely
  *warns*, resolves `0.12.x` from crates.io, and verification then builds
  against the published SDK — passing or failing on the wrong crate either
  way.
* **Treat an unused patch as a failure, not a warning**, so a mismatch the
  script's own semver arithmetic did not predict is red rather than a line in
  the log.

Check the rules without touching cargo:

```sh
python3 scripts/package_binding.py --self-test
# package_binding --self-test: 11 caret cases, 7 pin cases, the patch
# arguments and the unused-patch warning all pass
```

> **Superseded, recorded so it is not reinvented.** Until 2026-09-10 this
> section described a different mechanism: the script "skipped registry
> resolution alone" when every pin named the in-tree version and that version
> was absent from the index, proved the rest with `cargo package --list`, and
> annotated the skip as a warning on the job. That skip, its
> `--list --no-verify` fallback and the crates.io index query behind it were
> all **removed** on 2026-09-10 (see the module docstring of
> `scripts/package_binding.py`, and `docs/v0.9.0-post-release-review.md` B23
> and §2.5). They existed to tell a release window from a broken manifest
> when the window could not be verified; with the patch it can be, so nothing
> was left for them to cover. The cost of the old shape was that during the
> window the tarball was listed but never *built*, so any API the binding
> used from the same change went unchecked until the release shipped.

Nothing about this changes the order: step 2 still follows step 1. What it
changes is that the release-prep commit is now a commit CI can pass, and that
the gate's pass means the tarball compiled — not that a check was skipped.

## Path to 1.0.0

All four crates are pre-1.0 despite a multi-release history — they share a
single version, which `release.yml` requires to match across all four
manifests, and it is still `0.x`. Read it from the tree rather than from this
sentence, which has rotted before (it said `0.10.0` while the tree held
`0.13.0`):

```sh
grep -n '^version' crates/*/Cargo.toml
```

Nothing below is a promise
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
  `crates/*/src` — 94% lines / 94% regions / 92% functions. Those three
  figures are **undated and unverified**: they were written without a
  measurement date and nothing in the repository re-derives them, so
  re-measure before citing them.

  **Nothing enforces that floor.** `codecov.yml`'s `project.default` is
  `target: auto` with `threshold: 1%` — a *relative* check against the base
  commit, which fails only when coverage drops more than one point below
  wherever it already is. There is no absolute floor anywhere in that file,
  so a slow slide below 94% passes every PR status, one point at a time.
  (`patch.default` *is* absolute — `target: 75%`, `threshold: 5%` — but it
  grades the new and changed lines in a diff, not the project floor.) Turning
  this criterion into a gate means replacing `target: auto` with an explicit
  `target:` percentage; that is a decision nobody has taken, and until
  somebody does, this bullet is an aspiration the release process checks by
  hand or not at all.
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
