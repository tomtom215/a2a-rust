# Upgrading Between Minor Versions

As of 2026-09-10 (pre-0.12.0). The crates are at 0.11; the next release is
0.12.0, and its breaking section is already written under `## [Unreleased]`
in [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

This page is the migration guide the 191 KB changelog is not. One section per
minor boundary that broke something, newest first; each names what breaks,
shows the code before and after, and says when the fix is a rename and when
it is not. Every "after" block on this page is compiled against the current
crates by `a2a-book-tests`; the "before" blocks are fenced as `text` because
they no longer compile, which is the point.

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
- **Batch.** At most one breaking minor per calendar month. Non-breaking
  fixes ship whenever they are ready.
- **Label.** Every breaking change is listed under a `### Breaking Changes`
  heading for that release, with the migration in the same entry. The GitHub
  release notes are extracted from that section.
- **Prove it.** CI runs `cargo-semver-checks` against the previous release on
  every pull request. A finding on a change meant to be compatible is a bug in
  the change.
- **The MSRV is part of the API.** Raised only in a minor, only for a language
  or standard-library feature that earns it, never because a dependency moved.
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
including ones also listed elsewhere; the Unreleased section already does this.

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
2. `cargo build --all-targets`. Every API break on this page is a compile
   error; fix them from the section for your boundary.
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
  `HandlerLimits`, `WebSocketTransportConfig`, `TaskStoreConfig` and
  `CorsConfig` are in this group as of 0.12, as are `GrpcConfig`,
  `DispatchConfig` and `RateLimitConfig`; STABILITY.md §4 lists converting
  them as roadmap work for a future breaking minor.

So the habit that costs nothing today and saves a compile error later: build
configuration with `Default::default()` and `with_*` setters, and never with
a literal that names every field. Every setter on this page exists in the
current source.

Enums follow the same rule: `ClientError`, and the protocol enums that can
grow with the specification, are `#[non_exhaustive]`, so a `match` on one
already carries a wildcard arm and a new variant does not break it.

## 0.11 → 0.12

0.12.0 is the deliberate breaking minor before the two clean minors that
STABILITY.md §7 requires, so every "this struct should have been
`#[non_exhaustive]`" fix lands here rather than one per release. Five items.

### `HandlerLimits` gains two fields

`push_delivery_budget` (default 30 s) and `executor_drain_timeout` (default
5 s) are new. A literal naming every field stops compiling with
"missing fields":

```text
// 0.11 — a full struct literal
let limits = HandlerLimits {
    max_id_length: 2048,
    max_metadata_size: 1_048_576,
    max_cancellation_tokens: 10_000,
    // … every other field …
};
```

Use `Default` and the setters. All fourteen fields have one:

```rust
use std::time::Duration;
use a2a_protocol_server::HandlerLimits;

let limits = HandlerLimits::default()
    .with_max_id_length(2048)
    .with_push_delivery_budget(Duration::from_secs(30)) // new in 0.12
    .with_executor_drain_timeout(Duration::from_secs(5)); // new in 0.12
# let _ = limits;
```

A literal that ended in `..HandlerLimits::default()` was never broken by
this. Hand the result to `RequestHandlerBuilder::with_handler_limits` as
before.

### `WebSocketTransportConfig` (client) gains `max_pending_requests`

Default 64. The same rule: a full literal breaks, `Default` plus setters does
not, and every field has a setter.

```rust
use std::time::Duration;
use a2a_protocol_client::WebSocketTransportConfig;

let config = WebSocketTransportConfig::default()
    .with_connect_timeout(Duration::from_secs(5))
    .with_request_timeout(Duration::from_secs(30))
    .with_max_pending_requests(64); // new in 0.12
# let _ = config;
```

Hand it to `WebSocketTransport::connect_with_config(endpoint, config)` as
before. The new bound is why the next item exists.

### `ClientError` gains `TooManyPendingRequests { limit }`

Returned up front when a WebSocket connection already has `limit` requests
awaiting responses. The enum is `#[non_exhaustive]`, so an exhaustive `match`
already had a wildcard arm and nothing stops compiling. If you branch on
retryability, `ClientError::is_retryable()` returns `true` for it: room
appears as responses arrive.

```rust
use a2a_protocol_client::ClientError;

fn describe(err: &ClientError) -> String {
    match err {
        ClientError::TooManyPendingRequests { limit } => {
            format!("{limit} requests already in flight on this connection; retry")
        }
        other => other.to_string(),
    }
}
```

### `GrpcTransportConfig` (client) is `#[non_exhaustive]`

It gained `bare_address_scheme` and, under the `grpc-tls` feature,
`tls_config`, and it will not gain a field at your expense again. A struct
literal of any shape — including `..Default::default()` — is now an error
outside the crate:

```text
// 0.11
let config = GrpcTransportConfig {
    timeout: Duration::from_secs(30),
    ..Default::default()
};
```

Every field has a setter:

```rust
use std::time::Duration;
use a2a_protocol_client::GrpcBareAddressScheme;
use a2a_protocol_client::transport::grpc::GrpcTransportConfig;

let config = GrpcTransportConfig::default()
    .with_timeout(Duration::from_secs(30))
    .with_connect_timeout(Duration::from_secs(5))
    .with_max_message_size(4 * 1024 * 1024)
    .with_stream_channel_capacity(64)
    .with_bare_address_scheme(GrpcBareAddressScheme::HttpsExceptLoopback); // new in 0.12
# let _ = config;
```

`with_tls_config(tonic::transport::ClientTlsConfig)` is the sixth setter,
behind `grpc-tls`. No code in this repository, its examples or its bindings
built the struct literally, so the changelog expects this to bite rarely.

### `InMemoryQueueWriter` (server) is no longer `UnwindSafe` / `RefUnwindSafe`

It now holds the `Arc<dyn Metrics>` it reports dropped persistence events
through, and a trait object without those bounds removes the auto-impls.
Only code that wrapped a writer in `std::panic::catch_unwind` or
`AssertUnwindSafe` can notice. The handler that owns the writer never
required either bound. There is no migration beyond `AssertUnwindSafe` if you
are in that position.

### Also in 0.12, not breaking

- **MSRV lowered from 1.93 to 1.88; edition 2024.** Lowering is not a break.
- **Dependency majors:** opentelemetry / opentelemetry_sdk / opentelemetry-otlp
  0.32, tokio-tungstenite 0.30, rig-core 0.42 in the example. If your crate
  depends on one of these directly *and* holds a type the SDK hands back
  (`init_otlp_pipeline` returns `opentelemetry_sdk`'s `SdkMeterProvider`),
  align your version with the SDK's; STABILITY.md §6 says re-exported
  dependency types carry their own stability.

## 0.10 → 0.11

No Rust signature changed. Three wire-format corrections, each labelled
**BREAKING (wire format)** under `### Changed`, plus one addition worth
acting on. Nothing here is a compile error; all of it can fail a test, a
gateway rule or a peer.

### Six §5.4 error mappings corrected

`ErrorCode::http_status()` and `ErrorCode::grpc_status()` keep their
signatures and return different values for six codes. The SDK had implemented
a stale vendored copy of the specification's table; upstream had amended the
document in place under the same `1.0.0` version string.

| A2A error | HTTP was → is | gRPC was → is |
|---|---|---|
| `TaskNotCancelableError` | `409` → **`400`** | — |
| `ContentTypeNotSupportedError` | `415` → **`400`** | — |
| `InvalidAgentResponseError` | `502` → **`500`** | — |
| `PushNotificationNotSupportedError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |
| `UnsupportedOperationError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |
| `VersionNotSupportedError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |

The Axum adapter carried a second, hand-written copy of the table that had
drifted three more places (its `TaskNotCancelable` and
`InvalidStateTransition` answered `409`, its `PushNotSupported` answered
`501`; all are `400`). It now defers to `ErrorCode::http_status()`.
`PayloadTooLarge` (`413`) and `Overloaded` (`503`) remain adapter-specific
because A2A has no error code for either.

What to change: any client that matches on the old status, any test that
asserts it, any gateway rule keyed on it. The current values, as the crate
returns them:

```rust
use a2a_protocol_types::ErrorCode;

assert_eq!(ErrorCode::TaskNotCancelable.http_status(), 400); // was 409
assert_eq!(ErrorCode::ContentTypeNotSupported.http_status(), 400); // was 415
assert_eq!(ErrorCode::InvalidAgentResponse.http_status(), 500); // was 502
assert_eq!(ErrorCode::UnsupportedOperation.grpc_status(), "FAILED_PRECONDITION"); // was UNIMPLEMENTED
```

If you grade against the official `a2aproject/a2a-tck`, expect two MUST
failures — `HTTP_JSON-STATUS-001` and `GRPC-ERR-002`. That suite vendors the
same stale table; both rows are baselined with a reproduction in
`docs/official-tck-findings.md` §20.

### Push notifications are delivered as `application/a2a+json`

§4.3.3 specifies the A2A media type for the webhook `POST`; the sender used
`application/json`. A receiver that matches the header exactly — a framework
body parser keyed on `application/json`, a gateway rule, a WAF — stops
accepting deliveries until it also accepts `application/a2a+json`. The body
is unchanged, and any `+json` structured-suffix parser already handles it.
The value is exported:

```rust
use a2a_protocol_types::A2A_CONTENT_TYPE;

assert_eq!(A2A_CONTENT_TYPE, "application/a2a+json");
```

### Advertise the WebSocket binding by URI (recommended, not required)

`WEBSOCKET_BINDING_URI` (`https://a2a-rust.com/bindings/websocket/v1`) is new
in `a2a-protocol-types`. §5.8 says a custom binding **SHOULD** be identified
by a URI rather than a bare name. `AgentInterface::protocol_binding` is a
free-form string the crates never match on, so nothing breaks either way — but
a card advertising the bare `"WEBSOCKET"` should move, and a reader should
accept both while cards in the wild carry the old spelling.

```rust
use a2a_protocol_types::{A2A_VERSION, AgentInterface, WEBSOCKET_BINDING_URI};

let interface = AgentInterface {
    url: "wss://agent.example.com:3002".into(),
    protocol_binding: WEBSOCKET_BINDING_URI.into(), // was "WEBSOCKET"
    protocol_version: A2A_VERSION.into(),
    tenant: None,
};
# let _ = interface;
```

## 0.9 → 0.10

No breaking change. One deprecation, with a compiler warning at every use
site naming the replacement.

### `TenantLimits::max_stored_tasks` is deprecated

It named a cap on stored tasks and nothing ever read it: it sits on
`PerTenantConfig`, which the handler holds, and a store is constructed
independently and handed to the builder, so a store never saw it. The working
equivalent gives a named tenant its own `TaskStoreConfig`, and therefore its
own `max_capacity`:

```rust
use a2a_protocol_server::{TaskStoreConfig, TenantAwareInMemoryTaskStore};

let store = TenantAwareInMemoryTaskStore::new().with_tenant_override(
    "acme",
    TaskStoreConfig {
        max_capacity: Some(500),
        ..TaskStoreConfig::default()
    },
);
# let _ = store;
```

The override applies to a partition created after the store is built; a
tenant that already has one keeps it. The field is deprecated rather than
removed and is still present as of 0.12.

The same release corrected two pieces of documentation that read as if
something was enforced when it was not: `PerTenantConfig` and `TenantLimits`
now state that no code in the request path reads any of the five per-tenant
limits (data isolation runs through the tenant-aware stores' partitioning,
not through those limits), and `ClientConfig::max_response_size` now states
that it does not reach a transport supplied via `with_custom_transport` —
`WebSocketTransportConfig::max_message_size` is the setting that applies
there. Neither is a change in behaviour; both may be a change in what you
believed.

## 0.8 → 0.9

Two items under `### Breaking`.

### `executor_timeout` defaults to one hour

It was unbounded. An executor that never returned pinned its task, its event
queue and its cancellation token for the life of the process, and with
`max_cancellation_tokens` at 10,000, enough of them eventually stopped the
handler accepting work. A task that trips the ceiling now fails visibly, as a
`Failed` task with a timeout error.

If a task of yours legitimately exceeds an hour, either raise the ceiling or
restore unbounded execution explicitly with the new
`without_executor_timeout()`. The constant is
`a2a_protocol_server::builder::DEFAULT_EXECUTOR_TIMEOUT`.

```rust
use std::time::Duration;
use a2a_protocol_server::RequestHandlerBuilder;

fn six_hours(builder: RequestHandlerBuilder) -> RequestHandlerBuilder {
    builder.with_executor_timeout(Duration::from_secs(6 * 3600))
}

fn as_in_0_8(builder: RequestHandlerBuilder) -> RequestHandlerBuilder {
    builder.without_executor_timeout()
}
```

The changelog's own advice: a task genuinely running longer than an hour
should be using push notifications rather than holding an executor and a
stream open.

### `shutdown()` and `shutdown_with_timeout()` return `ShutdownReport`

Both returned `()` and discarded the executor-cleanup timeout with no log and
no return value, which made a hung cleanup indistinguishable from a clean
drain. The type is `#[must_use]`, so an existing `handler.shutdown().await;`
still compiles and warns rather than errors. Only a `let () = …` binding, or
a function whose return type was `()` and ended with the call, breaks.

```text
// 0.8
handler.shutdown_with_timeout(Duration::from_secs(10)).await;
```

```rust
use std::time::Duration;
use a2a_protocol_server::{RequestHandler, ShutdownReport};

async fn stop(handler: &RequestHandler) {
    let report: ShutdownReport = handler
        .shutdown_with_timeout(Duration::from_secs(10))
        .await;
    if !report.is_graceful() {
        eprintln!(
            "shutdown incomplete: {} queue(s) force-destroyed, executor cleanup completed: {}",
            report.queues_force_destroyed, report.executor_cleanup_completed
        );
    }
}
```

`queues_force_destroyed` is always `0` for `shutdown()`, which does not wait;
`executor_cleanup_completed == false` means the `on_shutdown` hook was
abandoned, not that it failed — it may still be running.

## 0.7 → 0.8

A deliberate breaking release: three deprecations announced in 0.7 come out,
on the schedule their notes named, and two card fields change shape on the
wire. Also the release that brought the four crates back into lockstep —
`a2a-protocol-server` alone had been at 0.8.0 while the others sat at 0.7.0.
If you pinned them separately, move all four to `0.8`.

> **If you serve gRPC to 0.6 clients, upgrade order matters — get it wrong
> and it is an outage, not a degradation.** Move gRPC clients onto the
> canonical `lf.a2a.v1.A2AService` first, then upgrade servers. Unaffected:
> JSON-RPC, REST and WebSocket deployments, and any gRPC deployment already on
> the canonical binding, which has been the default since 0.7 and is what the
> official Go, Python and Java SDKs speak.

### The `grpc-legacy-json` feature and the JSON-tunnel gRPC service are removed

Releases before 0.7 tunneled JSON inside a protobuf `bytes` envelope on a
non-standard service, `a2a.v1.A2aService`, served alongside the canonical
binding behind an off-by-default feature so that 0.6 clients survived a
rolling upgrade. The service is no longer registered on the listener, so a
0.6 client's calls fail at the gRPC layer and fall back to nothing.

Gone with it: the feature on `a2a-protocol-server` and `a2a-protocol-sdk`,
`dispatch/grpc/service.rs`, `GrpcDispatcher::into_legacy_service`, the
`LegacyA2aServiceServer` / `LegacyGrpcServiceImpl` re-exports, the JSON codec
helpers (`encode_json` / `decode_json` / `reader_to_grpc_stream`), the
`a2a.v1` proto and its build step.

```toml
# 0.7
a2a-protocol-server = { version = "0.7", features = ["grpc", "grpc-legacy-json"] }
```

```toml
# 0.8
a2a-protocol-server = { version = "0.8", features = ["grpc"] }
```

There is no in-process migration for a client on the old service: it moves
to `lf.a2a.v1.A2AService`. `GrpcDispatcher::serve` and `into_service` are
unchanged for everyone already on it.

### `with_event_queue_write_timeout` and `with_write_timeout` are removed

`RequestHandlerBuilder::with_event_queue_write_timeout` and
`EventQueueManager::with_write_timeout`, deprecated no-ops since 0.7.
Event-queue writes never block — the queue is a broadcast channel, and a slow
streaming consumer receives an explicit lag error on its reader rather than
exerting backpressure on the executor — so neither setter ever did anything.

Delete the call. To bound a slow consumer, size the queue and handle the lag
error on the reader:

```text
// 0.7
let handler = RequestHandlerBuilder::new(agent)
    .with_event_queue_write_timeout(Duration::from_secs(5))
    .build()?;
```

```rust
use a2a_protocol_server::RequestHandlerBuilder;

fn sized(builder: RequestHandlerBuilder) -> RequestHandlerBuilder {
    builder.with_event_queue_capacity(1024)
}
```

Said plainly, as the changelog says it: the two public setters are gone, but
`DEFAULT_WRITE_TIMEOUT` is still exported and the value is still threaded
into a dead field on `InMemoryQueueWriter`, because changing that
constructor's arity would have been an unadvertised break on top of the
advertised one.

### The bare `a2a-notification-token` header leaves the default CORS allow-list

`x-a2a-notification-token` is canonical and stays; the unprefixed spelling was
this SDK's own pre-0.7 name. A browser-hosted webhook receiver behind the same
CORS policy that still reads the bare header can restore it —
`CorsConfig::allow_headers` is a plain `String` and always was, so this
changes a default, not a capability:

```rust
use a2a_protocol_server::CorsConfig;

let mut cors = CorsConfig::new("https://console.example.com");
cors.allow_headers.push_str(", a2a-notification-token");
# let _ = cors;
```

Hand it to `JsonRpcDispatcher::with_cors` or `RestDispatcher::with_cors` as
before.

### `AgentCard.url` is no longer emitted

It is the v0.3 top-level URL. The v1.0 `AgentCard` has no `url`;
`supportedInterfaces` replaced it, and emitting it made the card fail the
specification's own JSON schema. The field is still **parsed**, so a card
from a v0.3 peer still loads. To publish an agent's address, populate
`supported_interfaces` (a `Vec<AgentInterface>`; the 0.11 section above shows
one).

```json
{ "name": "…", "url": "https://agent.example.com/rpc", "supportedInterfaces": [ … ] }
```

becomes

```json
{ "name": "…", "supportedInterfaces": [ { "url": "https://agent.example.com/rpc", "protocolBinding": "JSONRPC", "protocolVersion": "1.0" } ] }
```

A consumer that read `url` from the raw JSON needs the interface list
instead.

### `AgentCard.securitySchemes` is emitted in the v1.0 wire shape

`SecurityScheme` is a protobuf `oneof`, so its ProtoJSON encoding is a
single-key object naming the arm. The SDK emitted the v0.3 OpenAPI-style form.
The bytes on `/.well-known/agent-card.json` change:

```json
{ "type": "apiKey", "in": "header", "name": "X-API-Key" }
```

becomes

```json
{ "apiKeySecurityScheme": { "location": "header", "name": "X-API-Key" } }
```

**Both encodings are accepted** on input (the v1.0 form under either the
`json_name` or the proto field-name spelling of the arm, and the v0.3 form),
and a v0.3 scheme normalises to the v1.0 form on re-emission.
`ApiKeySecurityScheme` emits `location`, with `in` kept as an alias.
Deserialization is strictly more permissive than before; only a consumer
reading the card's raw JSON keys rather than parsing it with an A2A
implementation notices.

## Older boundaries

Before 0.8 the changelog was the migration guide, and it still is for these:

- **0.6 → 0.7** (`## [0.7.0] - 2026-07-24`): no `Breaking` heading. It is
  where the JSON tunnel went behind `grpc-legacy-json` and
  `with_event_queue_write_timeout` / `with_write_timeout` became deprecated
  no-ops, both slated for the 0.8 removals above; and where a consumer that
  falls behind the broadcast ring started receiving a marked `streamLagged`
  error and a closed stream instead of a silently skipped gap.
- **0.4 → 0.5** (`## [0.5.0] - 2026-04-02`, `### Breaking Changes`):
  `TaskStore::save()` and `TaskStore::insert_if_absent()` take `&Task`
  instead of an owned `Task`.
- **0.3 → 0.4** (`## [0.4.0] - 2026-03-31`, `### Breaking Changes`): the
  move to the A2A v1.0.0 wire format. Clients and servers on the old wire
  format need that section in full.
