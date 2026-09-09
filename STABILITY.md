<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# API stability policy

Adopted 2026-09-09. This document says what an adopter may rely on across
releases of the four published crates (`a2a-protocol-types`,
`a2a-protocol-client`, `a2a-protocol-server`, `a2a-protocol-sdk`), how a
breaking change is introduced when one is unavoidable, and what still has to
happen before a `1.0`. [ROADMAP.md](ROADMAP.md) tracks the open items;
[RELEASING.md](RELEASING.md) is the release runbook.

## 1. Where the project is

The crates are at `0.11`. Between `0.7.0` (2026-07-24) and `0.11.0`
(2026-08-30) there were four minor releases in five weeks, and the changelog
for that span names nine breaking changes. That cadence was appropriate while
the A2A `v1.0` wire contract, the tenant model and the four bindings were
being brought to conformance; it is not appropriate for a dependency someone
has to keep compiling. The policy below is the commitment that replaces it.

## 2. Semantic versioning, and what "breaking" means here

All four crates follow [Semantic Versioning 2.0.0](https://semver.org/). In
the `0.x` series a **minor** release may contain breaking changes and a
**patch** release may not, exactly as SemVer defines for `0.y.z`. The four
crates are versioned in lockstep and released together.

A change is breaking when `cargo-semver-checks` says so, or when it changes
observable wire behaviour that a conforming peer could notice. Concretely,
each of the following is a minor bump, never a patch:

- Removing or renaming any public item, or changing a public signature.
- Adding a required method to a public trait (see §4 — this is avoided).
- Adding a variant to an enum, or a field to a struct, that is **not**
  `#[non_exhaustive]`.
- Raising the minimum supported Rust version (§5).
- Changing what a default feature enables, or removing a feature.
- Changing the JSON, gRPC or WebSocket wire shape produced for a given
  input, or which requests are rejected, other than to correct a
  demonstrated deviation from the A2A specification.

A specification correction is the one class of behaviour change that may
ship in a patch release: when the SDK is shown, by the official TCK or ITK or
by a reference SDK, to deviate from the published A2A specification, the fix
lands in the next release of any kind and the changelog says which
requirement it corrects.

## 3. How a breaking change is introduced

1. **Deprecate first.** The old item stays for at least one minor release
   with `#[deprecated(since = "X.Y.0", note = "...")]` pointing at the
   replacement, so a downstream build gets a warning with a fix in it before
   it gets an error. Removal happens no earlier than the following minor.
2. **Batch.** Breaking changes are collected and shipped together, at most
   one breaking minor per calendar month, rather than one per change. A fix
   that does not break anything ships whenever it is ready.
3. **Label.** Every breaking change is listed in `CHANGELOG.md` under a
   `### Breaking Changes` heading for that release, with the migration in the same
   entry. The GitHub release notes are extracted from that section.
4. **Prove it.** CI runs `cargo-semver-checks` against the previous release
   on every pull request. A finding on a change that is meant to be
   compatible is a bug in the change; a finding on a deliberate break is
   acknowledged in the PR and lands only in a minor release.

The exception to step 1 is a security fix that cannot be made without
removing an unsafe surface; those are documented as such.

## 4. What is designed to stay compatible

- **The eleven server extension traits** (`AgentExecutor`, `TaskStore`,
  `PushConfigStore`, `PushSender`, `ServerInterceptor`, `TenantResolver`,
  `Metrics`, `Dispatcher`, `AgentCardProducer` and the two event-queue
  traits) are unsealed and stay unsealed. New methods are added with default
  implementations so external implementations keep compiling; the rules for
  doing that are in
  [CONTRIBUTING.md](CONTRIBUTING.md#extending-a-public-trait).
- **Protocol enums and structs that can grow with the A2A specification** are
  `#[non_exhaustive]`, so a new variant or field from a specification
  revision is a patch-level addition. The two deliberate exceptions are
  closed sets fixed by their underlying standards: `ApiKeyLocation` and
  `JsonRpcResponse` stay exhaustive so consumers can match them completely.
- **`GrpcTransportConfig` and `ServeConfig`** are `#[non_exhaustive]` with
  builder-style `with_*` methods, so a new option there is additive. The
  other configuration structs (`GrpcConfig`, `DispatchConfig`,
  `RateLimitConfig` and their siblings) are still exhaustive as of `0.11`: adding a field to one
  is a breaking change under §2 and is batched accordingly. Converting them
  is on the roadmap for the next breaking minor, so that this exception
  disappears rather than being restated.
- **Feature flags** are additive: enabling a feature never removes or changes
  an API that is available without it.

## 5. Minimum supported Rust version

The MSRV is part of the public API. It is raised only in a minor release,
only for a language or standard-library feature that earns it, and the
release notes say what that feature is. It is never raised because a
transitive dependency moved: with the edition-2024 resolver, Cargo selects
dependency versions compatible with the declared `rust-version`, and CI
builds the workspace on that exact toolchain.

The current MSRV is in the workspace `Cargo.toml` (`rust-version`) and the
README badge, which CI keeps consistent.

## 6. What is explicitly not covered

- Anything under `examples/`, `benches/`, `tck/`, `itk/`, `book-tests/` or
  `bindings/`: those are not published crates.
- The `--no-default-features` build of the client without any TLS feature,
  which exists for tests and proxies and is documented as such.
- Types re-exported from dependencies (`tonic`, `hyper`, `rustls`,
  `opentelemetry`, `sqlx`): their stability is theirs. A major bump of such
  a dependency that leaks into a public signature is a breaking change of
  this crate and is treated as §3 describes.

## 7. The path to 1.0

`1.0` is cut when all of the following hold for two consecutive minor
releases:

- No breaking change shipped, and none is pending in `ROADMAP.md`.
- The official TCK reports no MUST failure outside the baselined,
  upstream-acknowledged rows, and the nightly ITK run against every peer
  SDK is green or its failures are attributed upstream.
- `cargo-semver-checks` runs on all published crates with all features.
- The MSRV has been stable for at least six months.

At `1.0` the rules above tighten in the SemVer-defined way: breaking changes
require a major release, and the deprecation window in §3 becomes two minor
releases.
