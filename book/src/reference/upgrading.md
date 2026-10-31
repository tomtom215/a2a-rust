<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Upgrading Between Minor Versions

This chapter is the migration guide the changelog is not. For each minor
boundary that broke something, or deprecated something you may use, it names
what breaks, shows the code before and after, and says when the fix is a
rename and when it is not. Every Rust "after" block in the chapter is compiled
against the current crates by `a2a-book-tests`; the "before" blocks are fenced
as `text` because they no longer compile, which is the point.

The newest minor boundary covered is 0.15 → 0.16, whose breaking section is
`## [Unreleased]` until 0.16.0 is released, in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).
A test, `book-tests/tests/upgrading_guide.rs`, fails when the changelog calls a
release from 0.7 on breaking and this page has no `## X.Y → X.Z` section for
its boundary (A2A v1.0.1 §1.4: migration guidance MUST be provided when
introducing major breaking changes).

## Which page do I need

Start at the boundary you are on and read every page up to the version you
are moving to. Each section below says in a few lines what breaks and links
to the page with the migrations.

| You are on | Read |
|---|---|
| 0.15.x | [0.15 → 0.16](#015--016) |
| 0.14.x | [0.14 → 0.15](#014--015), then the above |
| 0.13.x | [0.13 → 0.14](#013--014), then the above |
| 0.12.x | [0.12 → 0.13](#012--013), then the above |
| 0.11.x | [0.11 → 0.12](#011--012), then the above |
| 0.10.x | [0.10 → 0.11](#010--011), then the above |
| 0.9.x | [0.9 → 0.10](#09--010), then the above |
| 0.8.x | [0.8 → 0.9](#08--09), then the above |
| 0.7.x | [0.7 → 0.8](#07--08), then the above |
| 0.6.x | [0.6 → 0.7](#06--07), then the above |
| older | [Older boundaries](#older-boundaries) |

### Patch releases with something to do

A patch such as 0.14.1 breaks no API, so it has no section of its own. Two
patches still ask something of some deployments:

- **0.14.1** is a security fix with one migration: `PathSegmentTenantResolver`
  now reads only the request URL, so on gRPC and the Axum adapter, which carry
  no tenant in the URL, it resolves none, and the request is served from the
  default tenant or, under `require_resolved_tenant()`, refused. Use
  `BearerTokenTenantResolver`, or `HeaderTenantResolver` behind a proxy that
  sets the header and strips any inbound copy; the `[0.14.1]` entry in
  CHANGELOG.md has the advisory.
- **0.15.1** breaks no API but changes two behaviours of `Telemetry` that
  existing code can notice: `install_subscriber` now always installs a
  subscriber, so a `tracing_subscriber` `.init()` after it panics (install
  yours first, or add `Telemetry::layer()` to it); and `HTTP_PROXY`,
  `HTTPS_PROXY`, `ALL_PROXY` and `NO_PROXY` now apply to the export, so a pod
  that sets `HTTP_PROXY` for its other traffic and exports to an in-cluster
  collector must list the collector in `NO_PROXY`
  ([Observability](../deployment/observability/tls-proxies.md#no_proxy-and-an-in-cluster-collector)
  has the entries that match and the ones that do not).

## The policy

[STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
(adopted 2026-09-09) is the commitment. What it says, condensed:

- **SemVer 2.0.0, in the `0.y.z` sense.** A minor release may contain
  breaking changes; a patch release may not. The four crates
  (`a2a-protocol-types`, `-client`, `-server`, `-sdk`) are versioned in
  lockstep and released together, so you bump all four at once.
- **What counts as breaking:** whatever `cargo-semver-checks` says, plus any
  change to the JSON, gRPC or WebSocket bytes produced for a given input, or
  to which requests are rejected. Removing or renaming a public item, adding a
  required trait method, adding a variant or field to something that is not
  `#[non_exhaustive]`, raising the MSRV, and changing what a default feature
  enables are all minor bumps, never patches.
- **The one patch-level exception:** a correction to a demonstrated
  deviation from the A2A specification, shown by the official TCK, the ITK or
  a reference SDK. The changelog entry names the requirement it corrects.
- **Deprecate first.** The old item stays for at least one minor release with
  `#[deprecated(since = "X.Y.0", note = "...")]` pointing at the replacement.
  Removal is no earlier than the following minor. The exception is a security
  fix that cannot be made without removing an unsafe surface.
- **Batch.** At most one breaking minor per calendar month, unless the
  release's changelog section carries a `**Cadence exception:**` line giving a
  reason it cannot wait (0.14.0 does). Non-breaking fixes ship whenever they
  are ready.
- **Label.** Every breaking change is listed under a `### Breaking Changes`
  heading for that release, with the migration in the same entry. The GitHub
  release notes are extracted from that section.
- **Prove it.** CI runs `cargo-semver-checks` against the previous release on
  every pull request. A finding on a change meant to be compatible is a bug in
  the change.
- **The MSRV is part of the API.** Raised only in a minor, only for a language
  or standard-library feature that earns it or to adopt a new major version of
  a direct dependency (0.15.0: 1.88 to 1.94, for `sqlx` 0.9), and never
  because a transitive dependency moved.
- **1.0 is cut** when two consecutive minors ship with no breaking change and
  none pending in `ROADMAP.md`; the official TCK reports no MUST failure
  outside the baselined, upstream-acknowledged rows; `cargo-semver-checks`
  runs on all published crates with all features; and the MSRV has been
  stable for six months. At 1.0 the deprecation window becomes two minors.

### How to read a release's breaking section

Open the release's `## [x.y.z]` heading in `CHANGELOG.md` and look for the
`### Breaking Changes` heading first. It is the first subsection when it
exists, and each bullet carries its own migration. Three things to know
about the older entries, because the heading was not always spelled the same
way:

- **0.9.0** used `### Breaking` rather than `### Breaking Changes`.
- **0.11.0** has no breaking heading at all. Its three breaking changes are
  wire-format corrections under `### Changed`, each prefixed
  **BREAKING (wire format)**, because they change bytes on the wire without
  changing any Rust signature — which is exactly the class of change
  `cargo-semver-checks` cannot see.
- **0.8.0** put its breaking changes under `### Removed` (three scheduled
  deletions) and `### Changed` (two wire-format changes), with a `WARNING`
  admonition at the top for the one that can cause an outage.

From 0.12.0 onward, every breaking change is under `### Breaking Changes`,
including ones also listed elsewhere; the 0.12.0 and 0.13.0 sections do this.

## How to check your own code

**If you publish a library that re-exports or wraps these crates,** run
[`cargo-semver-checks`](https://github.com/obi1kenobi/cargo-semver-checks)
on *your* crate after bumping the dependency:

```bash
cargo install cargo-semver-checks --locked
cargo semver-checks
```

It reports which of your public items changed shape because a type you
re-export changed shape. It is the tool this project's CI uses on its own
crates.

**If you are building an application,** `cargo-semver-checks` has nothing to
say to you: it compares API surfaces, and an application has none. The honest
tool is the compiler, then your tests:

1. Bump all four crates to the same version in `Cargo.toml`.
2. `cargo build --all-targets`. A change to a signature, a field or a type
   that your code names is a compile error; fix those from the section for
   your boundary. A change of behaviour, value or wire format compiles
   silently (the `TaskState` discriminants, a status code, a stricter check),
   so read the whole section for your boundary, not only the errors.
3. `cargo build` again and read the **warnings**. Deprecations arrive as
   warnings one minor before they become errors, and each note names the
   replacement. `#![deny(deprecated)]` in your crate root turns them into
   errors if you would rather deal with them now.
4. `cargo test`. The wire-format changes (0.8's card shape, 0.11's status
   codes and push `Content-Type`) do not touch the compiler at all; only a
   test that asserts the old bytes, or a peer that does, will notice.
5. If a gateway, WAF or proxy sits in front of your agent, check its rules
   against the wire-format items for your boundary. They are the ones that
   fail outside your process.

### What makes future additions non-breaking

Two conventions in these crates decide whether a new field costs you a
compile error:

- A struct marked `#[non_exhaustive]` cannot be built with a struct literal
  from outside its crate — not even with `..Default::default()`. You build it
  from `Default` and set fields with `with_*` methods. When such a struct
  gains a field, your code does not change. `GrpcTransportConfig` becomes one
  in 0.12; `ServeConfig` already is.
- A struct that is *not* `#[non_exhaustive]` breaks any literal that names
  every field the moment a field is added. A literal that ends in
  `..Default::default()` keeps compiling, and so does the `with_*` form.
  Since 0.12 no configuration struct is in this group: STABILITY.md §4 lists
  the seventeen that carry the attribute (`RetentionPolicy` joined them in
  0.13). What remains here is most of the spec's wire types in
  `a2a-protocol-types`, which are deliberately built with literals.

So the habit that costs nothing today and saves a compile error later: build
configuration with `Default::default()` and `with_*` setters, and never with
a literal that names every field. Every setter on this page exists in the
current source.

Enums follow the same rule: `ClientError`, and the protocol enums that can
grow with the specification, are `#[non_exhaustive]`, so a `match` on one
already carries a wildcard arm and a new variant does not break it.

## 0.15 → 0.16

The WebSocket listener refuses browser origins it was not told to allow; a
`JwtValidator` checks `iss` and `aud` unless told not to; an internal error's
text is withheld from callers; configuration errors are `ServerError::Config`
and `ClientError::Config`; `ServeReport` is `#[must_use]`; `TlsConfig` is
`#[non_exhaustive]`. [Upgrading from 0.15 to 0.16](./upgrading/0.15-to-0.16.md)
has each, with the code to change.

## 0.14 → 0.15

0.15.0 makes the SDK follow the A2A 1.0 specification where it did not, and
moves to current releases of `sqlx`, OpenTelemetry and SLIM: 26 breaking items
and about a hundred behaviour changes. A toolchain older than Rust 1.94 no
longer builds it. [Upgrading from 0.14 to 0.15](./upgrading/0.14-to-0.15.md)
groups the migrations by area:

- [Toolchain and dependencies](./upgrading/0.14-to-0.15/toolchain.md): the MSRV goes to 1.94 for `sqlx` 0.9; OpenTelemetry moves to 0.33; SLIMRPC moves to `agntcy-slim-rpc` 3.4.
- [Telemetry, dashboards and alerts](./upgrading/0.14-to-0.15/telemetry.md): `a2a.server.latency` is removed; `Telemetry` needs the `otlp` feature; `init_otlp_pipeline` takes `Option<&str>`; error labels, histogram buckets, the executor span name and rate-limit keys change.
- [Agent cards and signing](./upgrading/0.14-to-0.15/agent-cards.md): `build()` requires a valid agent card and refuses one that carries a credential, has an OAuth URL that is not a URL, or is signed without the budget extension; `sign_agent_card` is deprecated, and a card signed by 0.14 may not verify.
- [Authentication and caller isolation](./upgrading/0.14-to-0.15/authentication.md): an authenticator needs a scheme the public card declares, and a card that demands credentials needs an interceptor that checks them; tasks are isolated per caller; caller ids name how the caller authenticated; a JWT with no `sub` and a short HS256 secret are refused.
- [Stores and tenants](./upgrading/0.14-to-0.15/stores-and-tenants.md): `save_appending_history` takes a `HistoryLimit`; `TenantLimits::max_stored_tasks` is removed; a request with a tenant needs stores that isolate tenants.
- [Client](./upgrading/0.14-to-0.15/client.md): the client refuses responses `a2a.proto` does not allow; `from_card` follows the card's interface order and checks capabilities and required extensions; `CachingCardResolver` honours `max-age`; the REST client sends `application/a2a+json`; `ClientRequest` gains `query_params`.
- [Wire format and bindings](./upgrading/0.14-to-0.15/wire-and-bindings.md): HTTP+JSON answers `application/a2a+json`; `TaskState` numbers follow the proto; the gRPC conversion refuses absent REQUIRED fields; only `A2A-Version: 1.0` is served; `pageSize` is 1 to 100; an error's `data` is a ProtoJSON `Any`; a JSON-RPC notification gets no reply.
- [Push notifications](./upgrading/0.14-to-0.15/push.md): a push config the server cannot honour is refused at creation and at delivery; push needs a push sender; webhook timeouts are 10 s per request and 30 s per delivery.
- [Streams and executors](./upgrading/0.14-to-0.15/streams-and-executors.md): what an executor writes is checked; a parked subscription stays open; a stream that falls behind catches up, and a long replay is read in pages; an appended chunk needs a part; a snapshot cannot move a task back to `Submitted`.
- [Errors and Rust API](./upgrading/0.14-to-0.15/errors-and-rust-api.md): request-field errors are `ServerError::InvalidField`; rate-limit, WebSocket and SLIMRPC error codes change; `ConvertError` and `SseParseError` are `#[non_exhaustive]`; `serve::Server` is not `UnwindSafe`; the conformance kit's cancel check is renamed.
- [Also in 0.15, not breaking](./upgrading/0.14-to-0.15/not-breaking.md):
  behaviour changes a gateway, a test or a client is most likely to meet.

## 0.13 → 0.14

0.14.0 makes a failure read the same on every binding, and a refused
credential answer the status a client acts on. None of it is a compile error:
two items change what a build contains, the rest what an error `match`, a
status check or a gateway rule sees.
[Upgrading from 0.13 to 0.14](./upgrading/0.13-to-0.14.md) has:

- [`tracing` is a default feature](./upgrading/0.13-to-0.14.md#tracing-is-a-default-feature)
- [`default-features = false` on the SDK removes rustls](./upgrading/0.13-to-0.14.md#default-features--false-on-the-sdk-removes-rustls)
- [A peer that goes away reads the same on every binding](./upgrading/0.13-to-0.14.md#a-peer-that-goes-away-reads-the-same-on-every-binding)
- [gRPC `UNAUTHENTICATED` and `PERMISSION_DENIED` are `401` and `403`](./upgrading/0.13-to-0.14.md#grpc-unauthenticated-and-permission_denied-are-401-and-403)
- [`ClientBuilder::from_card` refuses a card with no interface for protocol major 1](./upgrading/0.13-to-0.14.md#clientbuilderfrom_card-refuses-a-card-with-no-interface-for-protocol-major-1)
- [Graceful shutdown ends in-flight tasks before it drains connections](./upgrading/0.13-to-0.14.md#graceful-shutdown-ends-in-flight-tasks-before-it-drains-connections)
- [Shipped `TaskStore`s refuse to move a terminal task to another state](./upgrading/0.13-to-0.14.md#shipped-taskstores-refuse-to-move-a-terminal-task-to-another-state)
- [Behaviour changes](./upgrading/0.13-to-0.14.md#behaviour-changes)
- [SLIMRPC binding: a refused credential answers `UNAUTHENTICATED` / `PERMISSION_DENIED`](./upgrading/0.13-to-0.14.md#slimrpc-binding-a-refused-credential-answers-unauthenticated--permission_denied)

## 0.12 → 0.13

0.13.0 makes the event log the record and spends it on stream resumption.
Nine breaking items; `RequestContext` becoming `#[non_exhaustive]` is the one
most code meets. [Upgrading from 0.12 to 0.13](./upgrading/0.12-to-0.13.md)
has:

- [`RequestContext` is `#[non_exhaustive]`, and gains `call_context`](./upgrading/0.12-to-0.13.md#requestcontext-is-non_exhaustive-and-gains-call_context)
- [`EventQueueReader::read` yields a `StreamEvent`, not a `StreamResponse`](./upgrading/0.12-to-0.13.md#eventqueuereaderread-yields-a-streamevent-not-a-streamresponse)
- [`PurgeReport::journal_orphans_deleted` is now `orphan_rows_deleted`](./upgrading/0.12-to-0.13.md#purgereportjournal_orphans_deleted-is-now-orphan_rows_deleted)
- [`IdempotencyClaim` and `KeyError` are `#[non_exhaustive]`](./upgrading/0.12-to-0.13.md#idempotencyclaim-and-keyerror-are-non_exhaustive)
- [`FailureClass::ALL` is a slice](./upgrading/0.12-to-0.13.md#failureclassall-is-a-slice)
- [`build()` refuses a signed agent card it would have to edit](./upgrading/0.12-to-0.13.md#build-refuses-a-signed-agent-card-it-would-have-to-edit)
- [A keyed `message/send` is retried only against a peer that advertises it](./upgrading/0.12-to-0.13.md#a-keyed-messagesend-is-retried-only-against-a-peer-that-advertises-it)
- [`message.id` is validated at ingress](./upgrading/0.12-to-0.13.md#messageid-is-validated-at-ingress)
- [`RetentionPolicy` and `PurgeReport` are `#[non_exhaustive]`](./upgrading/0.12-to-0.13.md#retentionpolicy-and-purgereport-are-non_exhaustive)
- [Also in 0.13, not breaking](./upgrading/0.12-to-0.13.md#also-in-013-not-breaking)

## 0.11 → 0.12

0.12.0 is the batch of "this struct should have been `#[non_exhaustive]`"
fixes. Five items; fifteen configuration structs becoming `#[non_exhaustive]`
is the one most code meets. [Upgrading from 0.11 to 0.12](./upgrading/0.11-to-0.12.md)
has:

- [`HandlerLimits` gains two fields](./upgrading/0.11-to-0.12.md#handlerlimits-gains-two-fields)
- [`WebSocketTransportConfig` (client) gains `max_pending_requests`](./upgrading/0.11-to-0.12.md#websockettransportconfig-client-gains-max_pending_requests)
- [`ClientError` gains `TooManyPendingRequests { limit }`](./upgrading/0.11-to-0.12.md#clienterror-gains-toomanypendingrequests--limit-)
- [Fifteen configuration structs are `#[non_exhaustive]`](./upgrading/0.11-to-0.12.md#fifteen-configuration-structs-are-non_exhaustive)
- [`InMemoryQueueWriter` (server) is no longer `UnwindSafe` / `RefUnwindSafe`](./upgrading/0.11-to-0.12.md#inmemoryqueuewriter-server-is-no-longer-unwindsafe--refunwindsafe)
- [Also in 0.12, not breaking](./upgrading/0.11-to-0.12.md#also-in-012-not-breaking)

## 0.10 → 0.11

No Rust signature changed: three wire-format corrections and one addition
worth acting on, all of which can fail a test, a gateway rule or a peer.
[Upgrading from 0.10 to 0.11](./upgrading/0.10-to-0.11.md) has:

- [Six §5.4 error mappings corrected](./upgrading/0.10-to-0.11.md#six-54-error-mappings-corrected)
- [Push notifications are delivered as `application/a2a+json`](./upgrading/0.10-to-0.11.md#push-notifications-are-delivered-as-applicationa2ajson)
- [Advertise the WebSocket binding by URI (recommended, not required)](./upgrading/0.10-to-0.11.md#advertise-the-websocket-binding-by-uri-recommended-not-required)

## 0.9 → 0.10

No breaking change; one deprecation, which the compiler reports at every use.
[Upgrading from 0.9 to 0.10](./upgrading/0.9-to-0.10.md) has:

- [`TenantLimits::max_stored_tasks` is deprecated](./upgrading/0.9-to-0.10.md#tenantlimitsmax_stored_tasks-is-deprecated)

## 0.8 → 0.9

Two breaking items. [Upgrading from 0.8 to 0.9](./upgrading/0.8-to-0.9.md)
has:

- [`executor_timeout` defaults to one hour](./upgrading/0.8-to-0.9.md#executor_timeout-defaults-to-one-hour)
- [`shutdown()` and `shutdown_with_timeout()` return `ShutdownReport`](./upgrading/0.8-to-0.9.md#shutdown-and-shutdown_with_timeout-return-shutdownreport)

## 0.7 → 0.8

Three deprecations announced in 0.7 are removed and two agent-card fields
change shape on the wire. If you serve gRPC to 0.6 clients, the upgrade order
matters: get it wrong and it is an outage.
[Upgrading from 0.7 to 0.8](./upgrading/0.7-to-0.8.md) has:

- [The `grpc-legacy-json` feature and the JSON-tunnel gRPC service are removed](./upgrading/0.7-to-0.8.md#the-grpc-legacy-json-feature-and-the-json-tunnel-grpc-service-are-removed)
- [`with_event_queue_write_timeout` and `with_write_timeout` are removed](./upgrading/0.7-to-0.8.md#with_event_queue_write_timeout-and-with_write_timeout-are-removed)
- [The bare `a2a-notification-token` header leaves the default CORS allow-list](./upgrading/0.7-to-0.8.md#the-bare-a2a-notification-token-header-leaves-the-default-cors-allow-list)
- [`AgentCard.url` is no longer emitted](./upgrading/0.7-to-0.8.md#agentcardurl-is-no-longer-emitted)
- [`AgentCard.securitySchemes` is emitted in the v1.0 wire shape](./upgrading/0.7-to-0.8.md#agentcardsecurityschemes-is-emitted-in-the-v10-wire-shape)

## 0.6 → 0.7

The boundary that costs a consumer of `a2a-protocol-types` alone the most.
[Upgrading from 0.6 to 0.7](./upgrading/0.6-to-0.7.md) has:

- [`JsonRpcRequest.id` is the three-state `JsonRpcRequestId`](./upgrading/0.6-to-0.7.md#jsonrpcrequestid-is-the-three-state-jsonrpcrequestid)
- [`AuthenticationInfo.credentials` and `TaskPushNotificationConfig::task_id` are `Option<String>`](./upgrading/0.6-to-0.7.md#authenticationinfocredentials-and-taskpushnotificationconfigtask_id-are-optionstring)
- [Also in 0.7, changing what parses rather than what compiles](./upgrading/0.6-to-0.7.md#also-in-07-changing-what-parses-rather-than-what-compiles)
- [Also in 0.7, on the server and client](./upgrading/0.6-to-0.7.md#also-in-07-on-the-server-and-client)

## Older boundaries

Before 0.7 the changelog's own `Breaking Changes` sections are the migration
guidance. [Boundaries before 0.7](./upgrading/older.md) says what each of
0.3 → 0.4, 0.4 → 0.5 and 0.5 → 0.6 changes and where the changelog has it.
