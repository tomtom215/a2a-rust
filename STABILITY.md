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

The crates are at `0.13` (prepared 2026-09-20). Between `0.7.0` (2026-07-24) and `0.11.0`
(2026-08-30) there were four minor releases in five weeks, and the changelog
for that span names nine breaking changes. That cadence was appropriate while
the A2A `v1.0` wire contract, the tenant model and the four bindings were
being brought to conformance; it is not appropriate for a dependency someone
has to keep compiling. The policy below is the commitment that replaces it.

`0.13.0` carries nine breaking changes, batched as §2 and §3 require —
though only eight were labelled when it shipped; see below. It is worth saying plainly what that costs: §7 cuts `1.0` after
**two consecutive** minor releases with no break, and `0.12.0` was the last
one to break, so that count restarts at zero here rather than reaching one.

They are: the `#[non_exhaustive]` marking of `RequestContext`; the event
position `EventQueueReader::read` now carries; one renamed `PurgeReport`
field; the `#[non_exhaustive]` marking of `IdempotencyClaim` and of
`KeyError`; `FailureClass::ALL` becoming a slice rather than a fixed-size
array; `RequestHandlerBuilder::build` refusing a signed agent card it would
otherwise have had to edit; a keyed `message/send` being retried only against
a peer known to honour the key; and `message.id` being validated at ingress.
Each has a migration in the changelog.

The last five were found by an audit after the first three were written down,
which is the honest account of why this section said "three" until 0.13.0
shipped. Nothing in `ROADMAP.md` is pending that requires another.

The ninth is the `#[non_exhaustive]` marking of `RetentionPolicy` and
`PurgeReport`. It reached 0.13.0 because the release was tagged on a later
merge than the preparation commit, and the changelog listed it — with
eighteen other entries that shipped — under `[Unreleased]` until 2026-09-23.
The published crates say which commit they were built from
(`.cargo_vcs_info.json`: `391f0df`), and that commit carries the marking.

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

- **The twelve server extension traits** (`AgentExecutor`, `TaskStore`,
  `PushConfigStore`, `PushSender`, `ServerInterceptor`, `TenantResolver`,
  `Metrics`, `Dispatcher`, `AgentCardProducer`, `RateLimitCounter` and the two
  event-queue traits) are unsealed and stay unsealed. New methods are added
  with default implementations so external implementations keep compiling; the
  rules for doing that are in
  [CONTRIBUTING.md](CONTRIBUTING.md#extending-a-public-trait).
- **Protocol enums and structs that can grow with the A2A specification** are
  `#[non_exhaustive]`, so a new variant or field from a specification
  revision is a patch-level addition. The three deliberate exceptions are
  closed sets fixed by their underlying standards — `ApiKeyLocation` (OpenAPI's
  header/query/cookie), `JsonRpcResponse` (JSON-RPC 2.0's result/error) and
  `JsonRpcRequestId` (JSON-RPC 2.0's absent/null/value id states) — and stay
  exhaustive so consumers can match them completely.
- **The configuration structs** (`HandlerLimits`, `DispatchConfig`,
  `CorsConfig`, `GrpcConfig`, `CacheConfig`, `PushRetryPolicy`,
  `RateLimitConfig`, `TaskStoreConfig`, `TenantStoreConfig`,
  `PerTenantConfig`, `TenantLimits`, `ServeConfig`, `RetentionPolicy`;
  `ClientConfig`, `RetryPolicy`, `WebSocketTransportConfig`,
  `GrpcTransportConfig`) are `#[non_exhaustive]` with `Default` (or a
  documented constructor) and a `with_*` setter per field, so a new option on
  any of them is additive. Until `0.12.0` most of them were exhaustive and
  adding a field was a breaking change under §2; that conversion was the bulk
  of the `0.12.0` breaking batch, and this exception no longer exists.

  `RetentionPolicy` was missed by that conversion and this list claimed
  otherwise until 2026-09-20 — the claim was found by needing to add a field
  to it, which is the only way an omission from a list of things that are
  *already done* ever gets found. `PurgeReport` is marked for the same reason:
  it is a report rather than a configuration struct, but a sweep that learns
  to count something new should not be a breaking change, and `0.13.0`'s own
  breaking list already carries one renamed field of it.
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

- Anything under `examples/`, `benches/`, `tck/`, `itk/` or `book-tests/`:
  those are not published crates.
- `bindings/a2a-protocol-slimrpc`. This bullet used to lump the binding in with
  the line above as "not a published crate", which is not why it is excluded:
  it *is* publishable, [`RELEASING.md`](RELEASING.md) documents how to publish
  it, and the book tells readers to depend on it. It is outside this guarantee
  because it is versioned independently and has not earned the guarantee — not
  because it cannot be published. (Separately, and as that document records
  with the evidence, it has in fact never been published to crates.io yet.)
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
