<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Upgrading Between Minor Versions

The newest minor boundary this page covers is 0.14 → 0.15, whose breaking
section is `## [0.15.0]` in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).
(A patch such as 0.14.1 breaks no API, so it has no section of its own here.
Its security fix has one migration: `PathSegmentTenantResolver` now reads only
the request URL, so on gRPC and the Axum adapter, which carry no tenant in
the URL, it resolves none, and the request is served from the default tenant
or, under `require_resolved_tenant()`, refused. Use
`BearerTokenTenantResolver`, or `HeaderTenantResolver` behind a proxy that
sets the header and strips any inbound copy; the `[0.14.1]` entry in
CHANGELOG.md has the advisory.) A test,
`book-tests/tests/upgrading_guide.rs`, fails when the changelog calls a
release from 0.7 on breaking and this page has no `## X.Y → X.Z` section for
its boundary (A2A v1.0.1 §1.4: migration guidance MUST be provided when
introducing major breaking changes).

This page is the migration guide the changelog is not. One section per minor
boundary that broke something or deprecated something you may use, newest
first; each names what breaks, shows the code before and after, and says when
the fix is a rename and when it is not. Every Rust "after" block on this page
is compiled against the current crates by `a2a-book-tests`; the "before"
blocks are fenced as `text` because they no longer compile, which is the
point.

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

## 0.14 → 0.15

These come from making the SDK follow the A2A 1.0 specification where it did
not, and from moving to current releases of `sqlx`, OpenTelemetry and SLIM.
CHANGELOG.md lists 26 breaking items under `### Breaking Changes` and about a
hundred behaviour changes beside them. Each of the 26 has a subsection below;
a few need nothing from you but a newer toolchain or an `AssertUnwindSafe`.
The behaviour changes an existing deployment is most likely to trip over are
among the subsections too, and the last one points to the rest.

### The MSRV is 1.94, and the SQL stores use `sqlx` 0.9

The minimum supported Rust version goes from 1.88 to 1.94 in all four crates
and in the SLIMRPC binding, so a toolchain older than 1.94 no longer builds
them. STABILITY.md §5 lets a minor release raise it to adopt a new major
version of a direct dependency, and this is that case: `sqlx` 0.9 needs 1.94,
and its pool types are in the SQL stores' public constructors
(`SqliteTaskStore::from_pool` takes a `SqlitePool`,
`PostgresTaskStore::from_pool` a `PgPool`, and so do the other SQL stores). A
crate that builds one of those pools itself moves to `sqlx` 0.9 with this
release; with two `sqlx` majors in the build, its pool is a different type
from the one the store names, and the call stops compiling. A store that opens
its own pool from a URL needs only the toolchain.

```text
# 0.14
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
```

```toml
# 0.15
sqlx = { version = "0.9", features = ["runtime-tokio", "sqlite"] }
```

`sqlx` 0.9 also refuses a statement built at run time unless it is marked
`sqlx::AssertSqlSafe`; the library's own such statements are marked and built
from static text, so only a store of your own that formats SQL needs the same.
The two helpers the changelog lists as now taking `&'static str`
(`pg_schema::apply`, the SQLite `add_applied_seq`) are crate-internal, so
nothing you call changed.

### `save_appending_history` takes a `HistoryLimit`

A `TaskStore` that overrides `save_appending_history` takes
`limit: HistoryLimit` where it took `max_history: usize`, and trims with
`limit.trim(&mut history)`, which bounds bytes as well as messages (release
audit F2). A caller that passed a count passes
`HistoryLimit::new(count, usize::MAX)` for the old behaviour; the handler
passes `TASK_HISTORY_LIMIT`, 1,024 messages and 16 MiB.

### `a2a.server.latency` is gone; units are UCUM

Dashboards and alerts on `a2a.server.latency` (Prometheus
`a2a_server_latency_seconds`) read `a2a.server.request.duration`
(`a2a_server_request_duration_seconds`) instead: the same measurement under
the conventions' name, with the conventions' current buckets, 0.1 ms to 100 s
(`a2a.server.latency` used 5 ms to 10 s; the other two call-duration
histograms moved too, see
[Dashboards, alerts and rate-limit tables](#dashboards-alerts-and-rate-limit-tables)).
A stored series' `le` labels and quantiles mix both sets until the old series
ages out. It was deprecated in 0.14.0. Units of counts are brace-annotated
(`{request}`); the other Prometheus names are unchanged.

### A signed agent card declares the budget extension

The builder adds `https://a2a-rust.com/extensions/budget/v1` to every card,
as it adds the failure taxonomy, and a signed card cannot be edited without
breaking its signature, so `build()` refuses a signed card that does not
declare it. Add it to `capabilities.extensions` (`required: false`) before
signing. An unsigned card needs nothing.

### `TenantLimits::max_stored_tasks` is gone

The field, `TenantLimits::with_max_stored_tasks` and
`TenantLimitsBuilder::max_stored_tasks` are removed. Nothing ever read them
(see [0.9 → 0.10](#tenantlimitsmax_stored_tasks-is-deprecated)); a build that
used them stops compiling where it used to warn. Give the tenant its own
`TaskStoreConfig` with `TenantAwareInMemoryTaskStore::with_tenant_override`
instead.

### `Telemetry` is the `otlp` feature; `otel` is on by default

On `a2a-protocol-server` directly, `Telemetry`, the `Langfuse` preset and
`init_otlp_pipeline` need `features = ["otlp"]`; `otel` now means the
instrumentation alone (`OtelMetrics`, the span bridge) and is a default
feature of the server and the client. On `a2a-protocol-sdk` both are default
and nothing changes. A build that must not link the OpenTelemetry API sets
`default-features = false`.

### `init_otlp_pipeline` and `init_otlp_pipeline_with_endpoint` take `Option<&str>`

Wrap the service name in `Some`. It now names the service only when
`OTEL_SERVICE_NAME` (or `service.name` in `OTEL_RESOURCE_ATTRIBUTES`) does
not; a deployment that set both used to get the argument and now gets the
environment's value. `None` leaves the name to the environment and the SDK's
`unknown_service` default.

### Cards and push configs that would leak or drop a credential are refused

`build()` and every card handler refuse a card with a credential in one of its
URLs (user information, or a query parameter named like a secret such as
`token` or `api_key`); move it out of the card. Creating a push config whose
`authentication` names a scheme but carries no credentials is refused with
`InvalidParams` on `authentication.credentials`; leave `authentication` out
for an unauthenticated webhook.

### A card whose OAuth or OpenID Connect URLs are not URLs is refused

a2a.proto says each OAuth flow URL "MUST be in the form of a URL", and that
`oauth2_metadata_url` requires TLS. `AgentCard::validate_security_schemes`,
which `build()` and every card handler run (the static, dynamic and hot-reload
handlers, and per-caller extended cards), now refuses a flow URL, refresh URL
or `openIdConnectUrl` that is not `scheme://authority...`, and an
`oauth2MetadataUrl` that is not `https`. The deprecated implicit and password
flows' URLs are checked only when given, and must be `https` (the proto: "The
OAuth2 standard requires the use of TLS"). A card that carried a relative URL
built before and does not now; write the absolute one. A client runs the same
check on a card it receives (see
[The client refuses what `a2a.proto` does not allow](#the-client-refuses-what-a2aproto-does-not-allow)).

```text
// 0.14 built; 0.15 refuses
{"oauth2SecurityScheme": {"flows": {"clientCredentials": {"tokenUrl": "/oauth/token", "scopes": {}}}}}
```

```json
{"oauth2SecurityScheme": {"flows": {"clientCredentials": {"tokenUrl": "https://idp.example/oauth/token", "scopes": {}}}}}
```

### An authenticator needs a scheme declared in the public card

Spec §13.3: agents "MUST authenticate requests using one of the schemes
declared in the public `AgentCard.securitySchemes`". `build()` now refuses an
authenticating interceptor (bundled or custom) behind a public card that
declares no `securitySchemes`, and a scheme declared on the extended card but
not, the same, on the public one. Declare the scheme your interceptor checks
in the public card: an `httpAuthSecurityScheme` with scheme `Bearer` for
`BearerTokenAuthInterceptor` or `JwtAuthInterceptor`, an
`apiKeySecurityScheme` naming the header for `ApiKeyAuthInterceptor`, an
`mtlsSecurityScheme` for `MutualTlsAuthInterceptor`
([Authentication](../building-agents/authentication.md#api-key--static-bearer-token)
shows one), and repeat any extended-card scheme in the public card.

### A card that demands authentication needs an interceptor that enforces it

Spec §3.3.2: "Servers MUST reject requests with invalid or missing
authentication credentials"; §7.4 and §13.3 add that a server authenticates
every request by its declared requirements, with one of the schemes the public
card declares. `build()` now refuses two configurations that built before,
with `InvalidParams` naming the fix:

- A card, or a skill, whose `securityRequirements` ask for a credential when no
  registered interceptor authenticates. Such a handler served every request
  without the credential its card demanded. Register an authenticating
  interceptor (`BearerTokenAuthInterceptor`, `ApiKeyAuthInterceptor`,
  `JwtAuthInterceptor`, or your own that returns `true` from `authenticates()`),
  or, when a gateway, mesh or sidecar in front of the process checks the
  credential, say so with `RequestHandlerBuilder::authenticated_upstream()`.
  A requirement list that contains an empty requirement (`{}`, optional
  authentication) asks for nothing, and needs neither.
- An authenticating interceptor that checks none of the schemes the public card
  declares, so a client following the card could never pass it. The new
  provided method `ServerInterceptor::checks_scheme` takes a `&SecurityScheme`
  and answers `Option<bool>`: it is how an interceptor says which it checks.
  The bundled authenticators answer it, and a custom one that keeps the
  default `None` is trusted to check one of them.

[The card and the interceptors must agree](../building-agents/authentication.md#the-card-and-the-interceptors-must-agree)
has the rules in full. Before, then after, for a card that asks for a bearer
token that a gateway checks:

```text
// 0.14: built, and served every request without checking the token
RequestHandlerBuilder::new(agent).with_agent_card(card_with_bearer_requirements).build()?;
```

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let card: AgentCard = serde_json::from_value(serde_json::json!({
    "name": "my-agent", "description": "An example agent", "version": "1.0.0",
    "supportedInterfaces": [{
        "url": "http://localhost:3000",
        "protocolBinding": "JSONRPC",
        "protocolVersion": "1.0"
    }],
    "capabilities": {},
    "defaultInputModes": ["text/plain"],
    "defaultOutputModes": ["text/plain"],
    "skills": [{ "id": "echo", "name": "Echo", "description": "Echoes text", "tags": ["echo"] }],
    "securitySchemes": { "bearer": { "httpAuthSecurityScheme": { "scheme": "Bearer" } } },
    "securityRequirements": [{ "schemes": { "bearer": {} } }]
}))?;
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .authenticated_upstream() // the gateway in front checks the token
    .build()?;
# drop(handler);
# Ok(())
# }
```

### A push config the sender cannot authenticate is not delivered

Spec §13.2: agents MUST send the credentials `authentication` specifies.
`HttpPushSender::send` used to deliver with no `Authorization` header when a
stored config named a scheme other than `Bearer` or `Basic`, or no
credentials; it now refuses the delivery with `InvalidParams`. Creation
already refused such configs, so only one stored another way is affected.

### `sign_agent_card` is deprecated, and refuses `None`

It takes `key_id: Option<&str>`, and in 0.14 `None` wrote a protected
header without `kid`, which spec §8.4.2 requires; it now returns
`InvalidParams` for `None`. It is removed in 0.16. Call
`sign_card_es256(card, key, key_id)`, whose `key_id` is a `&str`; with a key
id it signs what `sign_agent_card` did, apart from the two changes in the
next subsection. For Ed25519, `sign_card_ed25519`.

### A card signed by 0.14 may no longer verify

Spec §8.4.1 rule 1: the signed bytes are the card in protobuf field presence,
which leaves out a field holding its default value unless the field is
`REQUIRED` or declared `optional`. Canonicalization now does that. It drops an
empty `capabilities.extensions`, an extension's `required: false` or empty
`description`, an interface's empty `tenant`, and empty
`securityRequirements`; before, they were signed, so a card carrying one
canonicalized differently from the specification's own worked example. A
signature made by 0.14 over such a card therefore does not verify here, and
one made here does not verify under 0.14. A card with none of these fields is
unaffected. The protected header also carries `typ: "JOSE"` now (§8.4.2). The
server never signs the card it serves, so sign it again where you do, with
`sign_card_es256` or `sign_card_ed25519`, and serve the new signature:

```text
// 0.14: the signature covers the bytes 0.14 canonicalized
let signature = sign_agent_card(&card, pkcs8, Some("my-key"))?;
card.signatures = Some(vec![signature]);
```

```rust
use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::signing::sign_card_es256;

fn resign(card: &mut AgentCard, pkcs8: &[u8]) -> A2aResult<()> {
    let signature = sign_card_es256(card, pkcs8, "my-key")?;
    card.signatures = Some(vec![signature]);
    Ok(())
}
```

### `ConvertError` and `SseParseError` are `#[non_exhaustive]`

Build a `ConvertError` with `ConvertError::new(field, reason)`, and give a
`match` on `SseParseError` a wildcard arm.

### `serve::Server` is no longer `UnwindSafe`

With `tls-rustls` (a default feature of the SDK, not of `a2a-protocol-server`
alone), `Server` holds the TLS acceptor that `Server::with_tls` sets, built on
rustls' server configuration, which implements neither `UnwindSafe` nor
`RefUnwindSafe`; neither does `Server` now. `cargo-semver-checks` reports it
as `auto_trait_impl_removed`. Only code that moved a `Server` across
`std::panic::catch_unwind` notices, and the migration is the one the
`InMemoryQueueWriter` item in
[0.11 → 0.12](#inmemoryqueuewriter-server-is-no-longer-unwindsafe--refunwindsafe)
gives:

```text
// 0.14
let outcome = std::panic::catch_unwind(move || drop(server));
```

```rust
use std::panic::{AssertUnwindSafe, catch_unwind};
use a2a_protocol_server::serve::Server;

fn drop_guarded(server: Server) -> bool {
    catch_unwind(AssertUnwindSafe(move || drop(server))).is_ok()
}
```

### HTTP+JSON answers `application/a2a+json` unless asked otherwise

A response to a request that names no JSON type in `Accept` and sends no
`application/a2a+json` body is labelled `application/a2a+json` (§11.1's
SHOULD), on both `RestDispatcher` and the axum router; it was
`application/json`. A client that reads only `application/json` keeps getting
it by sending `Accept: application/json`. A request that asks only for
`text/event-stream` still gets `application/json` errors, so that released
a2a-go can read them. A JSON request body no longer decides the response
type on its own: an `application/json` body with no `Accept` is answered as
`application/a2a+json`.

### A parked subscription stays open until its task finishes

`HandlerLimits::subscribe_max_idle` is an `Option<Duration>` and `None` by
default: a `SubscribeToTask` stream to a task parked in `input-required` or
`auth-required` runs until the task reaches a terminal state or the client
goes, as §3.1.6 requires. It used to end after five minutes with no terminal
state, and the client resubscribed. `with_subscribe_max_idle(d)` still sets a
bound and takes a `Duration` as before; code that read the field compares
against `Some(d)` now. A bound trades conformance for reclaiming subscriptions
whose clients are connected but no longer reading; a client that has gone is
noticed without one (the field's rustdoc says how, per binding).

### `TaskState` numbers its variants as the proto does

`TaskState` is now generated from `a2a.proto` (spec §1.4, ADR 0016), and each
variant's discriminant is its proto number. `TaskState::Completed as i32` was
5 (its place in the old declaration) and is now 3, as `TASK_STATE_COMPLETED`
is in the proto and in the prost enum. Only code that casts a `TaskState` to
an integer sees a difference; `TaskState::ALL` lists the variants as before.
The wire form, `"TASK_STATE_COMPLETED"`, is unchanged.

### `build()` refuses an agent card the spec does not allow

Spec §5.7: a required array MUST contain at least one element.
`RequestHandlerBuilder::build` now runs `AgentCard::validate`, which checks
`defaultInputModes`, `defaultOutputModes`, `skills`, and every skill's `tags`,
and the card's `description` and `version`, which `a2a.proto` marks REQUIRED,
as well as the name and interfaces it checked before. It also refuses an empty
REQUIRED string of an interface (`url`, `protocolBinding`, `protocolVersion`),
of the provider (`organization`, `url`), of a skill (`id`, `name`,
`description`) and of a signature (`protected`, `signature`). A card missing
any of them used to be served as it was. Add a description and a skill:

```rust
# use a2a_protocol_sdk::prelude::*;
# let url = "http://localhost:3000";
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc(url))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
# assert!(card.validate().is_ok());
```

`AgentCard::new` now starts both mode lists at `["text/plain"]`, so a card
built with it only needs the description and the skill. `build()` also runs
`AgentCard::validate_security_schemes`, which refuses a security scheme whose
REQUIRED string is empty, such as an API key with no `name` (and an OAuth or
OpenID Connect URL that is not a URL; see
[that subsection](#a-card-whose-oauth-or-openid-connect-urls-are-not-urls-is-refused)).
A `DynamicAgentCardHandler` whose producer
returns an invalid card answers `500`, `HotReloadAgentCardHandler` refuses
to reload one and keeps serving the previous card, and
`StaticAgentCardHandler::new` returns `InvalidParams` for one.

### The client refuses what `a2a.proto` does not allow

Spec §5.7: implementations SHOULD reject a message missing a field the proto
marks REQUIRED. The client now holds what it receives to the rules the server
holds its own executors to, on every binding. A task with an empty `id`, a
status in `TASK_STATE_UNSPECIFIED`, a message without a `messageId`, `role` or
parts, an artifact without an `artifactId` or parts, a status or artifact
update without its `taskId` or `contextId`, and a push config without a `url`
or with an `authentication` that has no `scheme`, fail the call with
`ClientError::InvalidAgentResponse`, which names the call and the field; on a
stream, the event ends the stream with that error. They used to be returned as
if valid. Over gRPC such a field absent on the wire (proto3's `""` or `0`), or
a card's absent `capabilities`, is refused by the conversion where it used to
read as its zero value ([below](#the-grpc-conversion-refuses-an-absent-required-field)),
and any answer the conversion refuses is now `InvalidAgentResponse` rather
than `Transport`. Match `InvalidAgentResponse` to tell a non-conformant agent
from a transport fault; `ClientError` is `#[non_exhaustive]`, so the new
variant breaks no `match`.

An agent card fetched by `resolve_agent_card`, `fetch_card_from_url` or
`CachingCardResolver`, or returned by `get_extended_agent_card`, must pass
`AgentCard::validate` and `AgentCard::validate_security_schemes`, the checks
`build()` runs above; the deprecated implicit and password OAuth flows'
URLs must also be `https` (the proto: "The OAuth2 standard requires the use of
TLS"). A refused extended card is not adopted. `ClientBuilder::from_card` does
not validate the card it is given, but no longer selects an interface whose
`protocolVersion` is empty.

Two requests change on the wire: `list_tasks` sends `status_timestamp_after`
in UTC with a `Z` (a value with an offset is rewritten; one that is not ISO
8601 is refused, unsent, with `InvalidParams`), and an inline push config's
`task_id` is no longer sent with `SendMessage`, as the proto asks.

### The gRPC conversion refuses an absent REQUIRED field

proto3 reads an absent string as `""`, an absent enum as `0` and an absent
message as `None`. The `TryFrom<pb::…>` conversions of `a2a-protocol-types`
(the `proto` feature, which the gRPC binding uses) used to turn each into its
zero value, so a field the same JSON could not omit converted without it. They
now return a `ConvertError` whose `reason` is `missing required field` for
those fields: a task's `id` and its status's `state`, a message's `message_id`
and `role`, an artifact's `artifact_id`, a status or artifact update's
`task_id` and `context_id`, a card's `name`, `description`, `version` and
`capabilities` (no longer defaulted to the empty set), every interface's `url`,
`protocol_binding` and `protocol_version`, the provider's `organization` and
`url`, a skill's `id`, `name` and `description`, a signature's `protected` and
`signature`, an API key's `name`, an HTTP scheme's `scheme`, an
`openIdConnectUrl`, and the URLs of the authorization-code, client-credentials
and device-code flows.

A gRPC server answers a request carrying one `INVALID_ARGUMENT` with a
`google.rpc.BadRequest` violation naming the field; it used to reach the
handler, which refused most of them with the same code. A gRPC client reports
a response the conversion refuses as `InvalidAgentResponse` (above). Code that
calls the conversions itself gets the `ConvertError` where it got a value:

```rust
use a2a_protocol_types::proto as pb;
use a2a_protocol_types::task::Task;

// A peer that never set the task's `id` sends proto3's `""`.
let err = Task::try_from(pb::Task::default()).unwrap_err();
assert_eq!(err.field, "task.id");
assert_eq!(err.reason, "missing required field");
```

### Push configs the server cannot honour are refused at creation

A push config with an empty or scheme-less `url`, or with an authentication
scheme other than Bearer or Basic, answers `InvalidParams`. The second used to
be stored and then delivered with no credentials at all.

### Push needs a push sender

Spec §3.3.4: when push is not supported, every push-config operation MUST
answer `PushNotificationNotSupported`. A handler with no push sender now
answers that to Get, List and Delete as well as Create; they used to run
against the store. And `build()` refuses an agent card that declares
`capabilities.pushNotifications` with no sender, since every push-config
operation would refuse what the card advertises. Wire one with
`.with_push_sender(HttpPushSender::new())`, or declare `pushNotifications`
false.

### `ClientBuilder::from_card` follows the card's interface order

Spec §8.3.2: a client MUST select the first interface it supports, preferring
earlier entries in the card's list. `from_card` used to prefer JSON-RPC
wherever the card listed it; it now takes the card's first interface for
protocol 1.x and falls back down the list past one it cannot build. A card
listing `HTTP+JSON` before `JSONRPC` now gets HTTP+JSON. To keep the old
choice, rank the bindings yourself:

```rust
# use a2a_protocol_sdk::prelude::*;
# fn main() -> Result<(), ClientError> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::rest("http://localhost:3001"))
#     .with_description("An example agent")
#     .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let client = ClientBuilder::from_card_preferring(&card, &["JSONRPC".into()])?.build()?;
# drop(client);
# Ok(())
# }
```

`ClientConfig::preferred_bindings` and its setter are deprecated: nothing
reads them. Use `from_card_preferring` to rank, and
`ClientBuilder::chosen_interface` to see what was chosen.

### A client built from a card checks the card's capabilities

Spec §3.3.4: a client SHOULD check the agent card before an operation that
needs an optional capability. A client from `ClientBuilder::from_card` (or
`A2aClient::from_card`) now does: streaming and resubscription need
`capabilities.streaming`, the push-config methods and a send carrying an
inline push config need `pushNotifications`, and `get_extended_agent_card`
needs `extendedAgentCard`, each set to `true`. Otherwise the call returns
`ClientError::CapabilityNotDeclared` and nothing is sent.

A conformant agent refuses the same calls, so code that already handled
`UnsupportedOperationError` or `PushNotificationNotSupportedError` from such
an agent should handle this error the same way. For an agent that streams or
takes push configs without declaring it, opt out:

```rust
# use a2a_protocol_sdk::prelude::*;
# fn main() -> Result<(), ClientError> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3001"))
#     .with_description("An example agent")
#     .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let client = ClientBuilder::from_card(&card)?
    .without_capability_checks()
    .build()?;
# drop(client);
# Ok(())
# }
```

The CLI's equivalent is `--no-capability-check`. A client built for a bare
endpoint with `ClientBuilder::new` has no card and checks nothing.

### A client built from a card refuses a required extension it did not ask for

a2a.proto, `AgentExtension.required`: "If true, the client must understand and
comply with the extension's requirements". When the card marks an extension
`required: true` and `with_extensions` does not ask for its URI,
`ClientBuilder::build` now fails with `ClientError::Protocol` carrying code
`ErrorCode::ExtensionSupportRequired`, whose message names the extension; it
used to build, and the agent refused every call. Ask for the extension, or
call `skip_required_extension_check()` when an interceptor of yours sends the
`A2A-Extensions` header instead. A client built with `ClientBuilder::new` has
no card and is unaffected.

```rust
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_types::extensions::AgentExtension;
# fn main() -> Result<(), ClientError> {
const CONSENT: &str = "https://ext.example/consent/v1";
let mut consent = AgentExtension::new(CONSENT);
consent.required = Some(true);
let mut card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3001"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
card.capabilities.extensions = Some(vec![consent]);

let client = ClientBuilder::from_card(&card)?
    .with_extensions([CONSENT])
    .build()?;
# drop(client);
# Ok(())
# }
```

### `build()` requires an agent card

Spec §8.1: an A2A server MUST make an agent card available.
`RequestHandlerBuilder::build` now refuses a handler without one. Before, it
built, answered `/.well-known/agent-card.json` with 404, and skipped every
check a card would drive: streaming and push-notification capabilities, input
modes, required extensions. Give it the card your agent publishes:

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_streaming(true);
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .build()?;
# drop(handler);
# Ok(())
# }
```

Declare what the agent does: with a card, a streaming call to an agent that
does not declare `streaming` is refused, as the spec requires. A handler whose
card is published by something else, or one that is not a public A2A server
(a test), keeps the old behaviour with `.without_agent_card()`.

### An authenticating interceptor isolates tasks by caller

Spec §13.1: results are scoped to the caller's authorization boundaries. A
handler with an authenticating interceptor now gives each caller it
identifies (a labelled token or key, a JWT's `sub`, or a custom interceptor's
`set_caller_identity`) a partition of its own, and another caller's task is
`TaskNotFound`. Callers it does not name keep sharing the tenant's records, so
a deployment on unlabelled tokens or keys sees no change in what each caller
can reach; its default stores do become the tenant-aware in-memory ones.
Their capacity (`with_task_store_config`, 10,000 tasks by default) now applies
to each caller's partition, up to 1,000 partitions; supply a
`TenantAwareInMemoryTaskStore` with your own `TenantStoreConfig` to bound the
total differently.

Two configurations that built before now fail `build()`, each naming its
remedy: an authenticating interceptor with a task or push-config store you
supplied that does not partition by tenant (use the `TenantAware*` stores),
and an approval gate that wants an approver other than the requester,
including the one `Profile::Auditable` installs (or let requesters approve
with `ApprovalGate::allowing_self_approval()`). An explicit
`isolate_tasks_by_caller()` without an authenticating interceptor still fails,
as before. Where callers are meant to see each other's tasks, say so:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
#         .with_security_schemes(std::collections::HashMap::from([(
#             "bearer".to_owned(),
#             a2a_protocol_types::security::SecurityScheme::Http(
#                 a2a_protocol_types::security::HttpAuthSecurityScheme {
#                     scheme: "Bearer".to_owned(),
#                     description: None,
#                     bearer_format: None,
#                 },
#             ),
#         )]))
# }
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::InMemoryTaskStore;
# struct MyAgent;
# a2a_protocol_sdk::server::agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_task_store(InMemoryTaskStore::new())
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("token-a", "alice"),
        ("token-b", "bob"),
    ]))
    .share_tasks_across_callers()
    .build()?;
# drop(handler);
# Ok(())
# }
```

### A tenant needs stores that isolate tenants

A request that resolves to a non-empty tenant, named by the client or derived
by a `TenantResolver`, is now refused with `UnsupportedOperation` (`-32004` on
JSON-RPC, `400` on HTTP+JSON) unless both the task store and the push-config
store answer `true` from the new `isolates_tenants()`. The default stores
ignore the tenant, so such a request used to be served from records every
tenant shared: on 0.14.1 tenant B could get, list, subscribe to and cancel
tenant A's task. The six `TenantAware*` stores answer `true`, the default
stores `false`, and the built-in wrappers what the store they wrap answers.
Requests without a tenant are unaffected. Configure a `TenantAware*` task
store; an unset push-config store then defaults to
`TenantAwareInMemoryPushConfigStore`. Where tenants only key per-tenant limits
over records you mean every tenant to share, say so with
`RequestHandlerBuilder::accept_unisolated_tenants()`. A custom `TaskStore` or
`PushConfigStore` that partitions by tenant must override `isolates_tenants()`
to return `true`, and a wrapper store must forward it.

```text
// 0.14: a request naming tenant "acme" was served from the shared records
RequestHandlerBuilder::new(agent).with_agent_card(card).build()?;
```

```rust
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::TenantAwareInMemoryTaskStore;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#     .with_description("An example agent")
#     .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
// Each tenant gets records of its own.
let isolated = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card.clone())
    .with_task_store(TenantAwareInMemoryTaskStore::new())
    .build()?;

// Or: tenants key limits over records every tenant shares.
let shared = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card)
    .accept_unisolated_tenants()
    .build()?;
# drop((isolated, shared));
# Ok(())
# }
```

### `CachingCardResolver` trusts the card's `max-age`

Spec §8.6.2: clients SHOULD honor RFC 9111 when fetching agent cards.
`CachingCardResolver::resolve` used to send a request on every call; it now
returns the stored card without one while the card is fresh, for its
`Cache-Control: max-age` (or `Expires`). A card the agent changes is seen
once the stored one goes stale, which against this SDK's servers is up to
`agent_card_max_age` (default 3600 s) later. To see a change at once, as
before, build the resolver with `.with_always_revalidate(true)`, or lower
the server's `max-age`. `with_options` now also starts the resolver with an
empty cache, so a clone given other options no longer shares its original's
stored or extended card.

### Push webhook timeouts: 10 s per request, 30 s per delivery

`HttpPushSender`'s request timeout goes from 30 s to 10 s and
`HandlerLimits::push_delivery_timeout` from 5 s to 30 s, both inside the
10-30 s that spec §4.3.3 recommends. The old pair let one attempt of the
sender's three run; now all three start and two always finish. A webhook
that needs more than 10 s per request needs `HttpPushSender::with_timeout`.
Because the delivery bound now equals `push_delivery_budget`, an event's
deliveries run all at once instead of 17 at a time; to bound how many are in
flight, raise `push_delivery_budget` or lower the delivery bound.

### Only `A2A-Version: 1.0` is served

Spec §3.6.2: a request is processed with the semantics of the version it
asks for, and a server that does not support that version MUST answer
`VersionNotSupportedError`. This server implements 1.0 only, so a request
naming `1.1`, or a bare `1`, is now refused with an error that says it
supports `1.0`; a patch number is still ignored (`1.0.1` is 1.0). It used to
accept any `1.x` and serve it as 1.0, which is also what the official Python
SDK does. Every client in this SDK, and every peer the conformance suites
run against, sends `1.0`.

The Axum adapter (`A2aRouter`) now enforces the version, and the request media
type, as `RestDispatcher` does. A request that names no version, in the
`A2A-Version` header or the `A2A-Version` query parameter, is
`VersionNotSupportedError` (unless the `DispatchConfig` given to
`A2aRouter::with_config` has `with_require_version_header(false)`), and a
request body that is neither `application/json` nor `application/a2a+json` is
`ContentTypeNotSupportedError`. The agent card, `/health` and `/ready` stay
versionless. A client that called the router without the header must now send
it.

### OpenTelemetry 0.33 for the `otel` and `otlp` features

`opentelemetry`, `opentelemetry_sdk`, `opentelemetry-otlp` and the rest of the
family move from 0.32 to 0.33.1, and `tracing-opentelemetry` from 0.33 to
0.34.0. Types from them appear in `Telemetry`'s API, so move your own
dependencies on these crates to the same versions. Export now retries a
failed batch (four attempts by default), which is `opentelemetry-otlp`'s new
default, and `shutdown` reports only exports that were lost after retrying,
on gRPC as well as OTLP/HTTP.

### SLIMRPC is built on `agntcy-slim-rpc` 3.4

`a2a-protocol-slimrpc` goes from 0.6.0 to 0.7.0. It requires `0.15` of the
server, client and types crates, and moves its SLIM requirements to the line
`agntcy-slim-rpc` 3.4.1 needs: `agntcy-slim-rpc` 2.3 → 3.4, `agntcy-slim-auth`
0.15 → 0.17, `agntcy-slim-datapath` 0.18 → 0.19, `agntcy-slim-service`
0.12 → 0.13 and `agntcy-slim-config` 0.16 → 0.17; `agntcy-slim-session` stays
at 0.7. They are public dependencies: `SlimRpcServer::from_app`,
`SlimRpcTransport::from_app`, `SlimRpcMulticast::from_app` and the builders'
`with_identity` take SLIM types, so pass types from the same SLIM versions.
The binding's own code needed no change. Over the wire, SLIM 3.x sessions are
MLS and verify the peer's identity token on the request that sets one up; a
peer on the 2.x line was not tried against this binding. The binding is
outside the root workspace and versioned on its own, as in
[0.12 → 0.13](#also-in-013-not-breaking).

```text
# 0.14, bindings/a2a-protocol-slimrpc 0.6.0
a2a-protocol-slimrpc = "0.6"
agntcy-slim-rpc = "2.3"
```

```toml
# 0.15, bindings/a2a-protocol-slimrpc 0.7.0
a2a-protocol-slimrpc = "0.7"
agntcy-slim-rpc = "3.4"
```

### A stream that falls behind catches up instead of ending

A reader slower than the agent used to end with `streamLagged` as soon as the
broadcast queue overwrote an event it had not read. It now reads what it
missed from the task's event log and goes on (spec §3.5.2), on every store that
keeps a log, which is every built-in one. A test that provokes `streamLagged`
with a small `with_event_queue_capacity` no longer sees it there; also bound
the log below what the stream misses
(`TaskStoreConfig::with_max_events_per_task`) and, since an attached stream
now holds what it owes past that bound, leave it no room to
(`with_max_held_events_factor(Some(1))`). Lowering
`HandlerLimits::subscribe_replay_limit` no longer provokes it either: since
0.15.0 the limit is the size of each read of the log, and a longer replay is
read in pages (next subsection).

### A replay longer than `subscribe_replay_limit` is read in pages

Spec §3.5.2: each stream MUST receive the same events in the same order.
`HandlerLimits::subscribe_replay_limit` (1,000 by default) bounds each read of
the task's event log now, not the replay. A stream that lagged by more than the
limit used to end with `streamLagged`, and a `Last-Event-ID` resume that
needed more than the limit stopped at it and went on live, past the positions
between the replay's end and where its receiver attached, with no error. Both
now read the log a page at a time, from where each page ended, delivering each
page before the next is read, and go to the live channel only after the last
page. The same holds for a turn a `SubscribeToTask` stream missed between
polls. A page the log cannot supply is still reported as `streamLagged`, once,
after what it did supply.

Memory stays bounded by the limit, but a client that resumes with
`Last-Event-ID: 0` now receives the task's whole retained history on one
connection, where it received at most one page. A proxy, a client buffer or a
test that assumed the old cap sees more frames; the limit still sets the size
of each read, and `with_subscribe_replay_limit` sets it:

```rust
use a2a_protocol_server::HandlerLimits;

let limits = HandlerLimits::default().with_subscribe_replay_limit(500);
# let _ = limits;
```

### ListTasks takes a `pageSize` from 1 to 100

A2A's proto bounds `page_size` to 1 through 100, and the server now refuses
anything else with `InvalidParams` (400 on HTTP+JSON). It used to clamp a
larger size to 1000 and read 0 as the default of 50. A client paging through
every task should ask for 100 at most; leaving `pageSize` out still lists 50.

### The conformance kit's cancel check is `cancel_succeeds`

The `conformance` feature's `cancel_emits_terminal` check is renamed
`cancel_succeeds`; a CI step that greps for the old name finds nothing. It
failed a `cancel` hook that wrote no terminal state, because such a hook left
the task's streams without one. The handler now writes `Canceled` after such a
hook (spec §3.1.2), so the check passes it, and fails a hook that errors,
panics or overruns.

### `GetExtendedAgentCard` with no extended card configured

A card that declares `capabilities.extendedAgentCard` and a handler built
without `with_extended_agent_card` now answers `GetExtendedAgentCard` with
`ExtendedAgentCardNotConfiguredError` (`-32007`), as §3.1.11 and §13.3
require; it used to serve the public card in the extended card's place. To
serve the same content as before, pass the public card to
`with_extended_agent_card` too:

```rust
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::RequestHandlerBuilder;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
    .with_capabilities(AgentCapabilities::none().with_extended_agent_card(true));
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card.clone())
    .with_extended_agent_card(card)
    .allow_unauthenticated_extended_card()
    .build()
    .unwrap();
# let _ = handler;
# }
```

`allow_unauthenticated_extended_card()` is there because the extended card
here is the public card, so nothing secret is served. Spec §13.3 requires the
extended card endpoint to be authenticated, and by default a handler whose
card declares `extendedAgentCard` refuses to serve it unless an interceptor
authenticates; an extended card with more content than the public one needs
an authenticating interceptor instead.

A handler built with `without_agent_card()` and no extended card answers
`UnsupportedOperationError` (`-32004`) where it answered `-32007`: nothing on
it declares the capability. Both are HTTP `400`.

### An appended artifact chunk needs a part

An executor's `TaskArtifactUpdateEvent` with `append: true` and no parts is
refused by the event queue (`InvalidParams`), as one without `append` already
was: a2a.proto's `Artifact.parts` "Must contain at least one part" and makes
no exception for a chunk. An executor that ended a streamed artifact with an
empty `lastChunk: true` chunk sets `lastChunk: true` on the chunk carrying the
last part instead.

### Request-field errors are `ServerError::InvalidField`

The handler's own checks of a request field (ids, a message's role, parts and
metadata, a push config's fields, `ListTasks` filters, REST query
parameters) return `ServerError::InvalidField { field, message }`, which
answers as `InvalidParams` does plus a `google.rpc.BadRequest` detail naming
the field. They returned `ServerError::InvalidParams(message)`. The code, the
HTTP status and the `invalid_params` metric label are unchanged, and the
response gains the `BadRequest` detail on JSON-RPC, HTTP+JSON, WebSocket and
gRPC. The `SendMessage` id checks now name the field by its wire path, in the
violation's `field` and in the message: `message.messageId`,
`message.contextId` and `message.taskId` (they named `message.id`,
`context_id` and `task_id`), and `message.referenceTaskIds`, which was not
checked before. A client or test that matched the old strings matches the new
ones. A `match` or `matches!` on the variant is affected, and still compiles.
Match either variant, or the code:

```rust
use a2a_protocol_server::ServerError;
use a2a_protocol_types::error::ErrorCode;

fn is_invalid_params(err: &ServerError) -> bool {
    matches!(err, ServerError::InvalidParams(_) | ServerError::InvalidField { .. })
        || err.to_a2a_error().code == ErrorCode::InvalidParams
}

let err = ServerError::InvalidField {
    field: "message.parts".into(),
    message: "message must contain at least one part".into(),
};
assert!(is_invalid_params(&err));
```

### An error's `data` detail is a ProtoJSON `Any`

The JSON-RPC, HTTP+JSON and WebSocket bindings carry an error's `data` as a
`google.protobuf.Struct` detail in the error's details array. Its fields
were written beside `@type`; they are now under `value`, as ProtoJSON writes
an `Any` holding a `Struct`:

```json
{"@type": "type.googleapis.com/google.protobuf.Struct", "value": {"streamLagged": 6}}
```

`A2aError::is_stream_lagged` and `dropped_event_count` read both shapes, as
does the REST client's stream decoder. Code that read a detail's fields
directly, or a client in another language that reads them beside `@type`,
reads `value` instead.

### The REST client sends `application/a2a+json`

The HTTP+JSON client labels a request body `application/a2a+json`, as §11.1
asks, and a non-streaming call asks for
`application/a2a+json, application/json` (a streaming call still asks for
`text/event-stream`). A GET or DELETE has no body and now no `Content-Type`.
a2a-java 1.3.0.Final and older refuse a request labelled anything but
`application/json` (`ContentTypeNotSupportedError`); 1.4.0.Final accepts it. A
client that must reach an older a2a-java server uses its JSON-RPC interface.

### A SLIMRPC multicast lag is `is_stream_lagged`

A multicast member that fell behind ends with the typed `streamLagged` error,
as a lagging stream does on the JSON-RPC, HTTP+JSON, WebSocket and gRPC
bindings, so `ClientError::is_stream_lagged()` and `dropped_event_count()`
read it. Code that matched the old message text (`"event(s) dropped"`)
matches `is_stream_lagged()` instead. A unicast SLIMRPC stream's lag is
still the server's error text, without the marker.

### A JSON-RPC notification gets no reply

A JSON-RPC request with no `id` member is a notification, and "the Server
MUST NOT reply to a Notification, including those that are within a batch
request" (JSON-RPC 2.0 §4.1). The server ran one and answered it with
`"id": null`; it now runs it and answers nothing: `204 No Content` over HTTP
for a notification alone, a batch's array without the notifications' answers
(`204` when every member is one), and no frame over WebSocket. A streaming
method sent as a notification is not run, as in a batch: a stream is its
only answer. A client that sent requests without an `id` and read the reply
sends an `id`. `"id": null` is a call, and is still answered.

### A task snapshot cannot move a task back to `Submitted`

An executor's `Task` snapshot that changes the task's state is held to the
transitions a status update is. For one moving a task back to `Submitted`,
the background processor marks the task failed, and a blocking send answers
`InvalidStateTransition` (an `InvalidParams` error), as each does for such a
status update. One leaving a terminal state is refused. A snapshot in the
task's current state is stored as before.

### `ClientRequest` gains `query_params`

An API key that the agent's card places in the query string is sent there
now, so a request reaches the transports with query parameters as well as
headers. `ClientRequest` has a new public field, `query_params`, and a struct
literal that names every field no longer compiles. Build the request with
`ClientRequest::new`, which leaves it empty:

```rust
use a2a_protocol_client::ClientRequest;

let mut req = ClientRequest::new("GetTask", serde_json::json!({ "id": "t-1" }));
assert!(req.query_params.is_empty());
req.query_params.push(("api_key".into(), "k".into()));
```

A custom `Transport` still compiles: the new `send_request_with_query` and
`send_streaming_request_with_query` methods have defaults, which send a call
with no query parameters as before and refuse one with them
(`ClientError::InvalidAuthScheme`). Override both if your transport has a URL
to put them in. gRPC, WebSocket and the SLIMRPC binding refuse such a call.

### A caller is named by how it authenticated

The bundled interceptors record `bearer:<label>`, `api-key:<label>` and
`jwt:<issuer>:<sub>` where they recorded the bare label or `sub`, so one `sub`
from two issuers is two callers. Two things follow here, and a third is the
next subsection.

An approver allowlist, or anything else that compares against an identity,
names the new form. `auth::caller_id` builds it:

```rust
use a2a_protocol_server::approval::ApprovalGate;
use a2a_protocol_server::auth::caller_id;

let gate = ApprovalGate::new().with_approvers([
    caller_id::bearer("alice"),
    caller_id::jwt(Some("https://idp.example"), "bob"),
]);
# let _ = gate;
```

Records a SQLite or PostgreSQL tenant-aware store keeps under caller
isolation are keyed by the old name. Move them once, before serving with
0.15, or each caller's tasks and webhooks are not found. The old key does not
say how the caller authenticated, so you give the map; the helpers cover a
deployment that used one authenticator. `migrate_caller_keys` exists on the
SQLite and PostgreSQL stores (the `sqlite` or `postgres` feature); the
in-memory stores keep nothing across a restart and need no migration:

```rust,no_run
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::caller_keys;
use a2a_protocol_server::store::tenant_sqlite_store::TenantAwareSqliteTaskStore;

let tasks = TenantAwareSqliteTaskStore::new("sqlite:tasks.db").await?;
let moved = tasks.migrate_caller_keys(caller_keys::bearer_labels()).await?;
println!("{} partitions, {} rows", moved.partitions, moved.rows);
# Ok(())
# }
```

Run it on the push-config store too, with its cipher set: a sealed config's
context names its partition, and the migration seals it again under the new
one. For several authenticators, pass your own `Fn(&str) -> Option<String>`
returning the new identity of each old name, or `None` to leave it.

### A JWT with no `sub` is refused under caller isolation

A JWT with no `sub` authenticates and names nobody. Under the default caller
isolation it used to be served the tenant's shared records, which handed an
anonymous token every unattributed task. It is now refused with
`PermissionDenied`, unless callers share tasks
(`RequestHandlerBuilder::share_tasks_across_callers()`), which keeps the shared
records for it. A JWT issuer that omits `sub` needs one added, or the handler
needs `share_tasks_across_callers()` as in
[An authenticating interceptor isolates tasks by caller](#an-authenticating-interceptor-isolates-tasks-by-caller).

### `JwtValidator` refuses an HS256 secret shorter than 32 bytes

RFC 7518 §3.2 requires an HS256 key at least as long as the hash output, so
`with_hs256_secret` (the `auth-jwt` feature) now takes a secret of at least
`MIN_HS256_SECRET_LEN` (32) bytes. It took any length, down to one byte, and a
guessable secret let anyone who guessed it mint tokens the server accepted. A
shorter one **fails closed**: the call logs an error and stores nothing, so
every HS256 token is rejected. A deployment configured with a short secret
therefore stops accepting HS256 tokens on upgrade, with a log line to say why.
Move to a secret of at least 32 bytes, and use `try_with_hs256_secret`, which
returns the error so the process can refuse to start. RS256 and ES256 keys are
not affected.

```text
// 0.14: any length was stored
let validator = JwtValidator::new().with_hs256_secret(b"hunter2".to_vec());
```

```rust
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::auth::jwt::JwtValidator;

assert!(JwtValidator::new().try_with_hs256_secret(b"hunter2".to_vec()).is_err());

// Load yours from a secret store; it must be at least 32 bytes.
let secret = b"0123456789abcdef0123456789abcdef".to_vec();
let validator = JwtValidator::new().try_with_hs256_secret(secret)?;
# let _ = validator;
# Ok(())
# }
```

### What an executor may write is checked

The event queue every executor writes through now holds what it writes to
rules the server already held a client to. A write that breaks one returns
`InvalidParams` naming the field, before the event is stored or sent:

- A message with no `messageId`, role or parts, or in `ROLE_UNSPECIFIED`; an
  artifact with no `artifactId` or parts; a status in
  `TASK_STATE_UNSPECIFIED`. These are refused wherever they sit: in the event
  itself, or inside a status update or a task snapshot.
- A status, artifact or task event that names another task or context than the
  running one. An empty task or context id gets the running task's.
- What the gRPC binding cannot carry: a `raw` part that is not base64 (one in
  URL-safe or unpadded base64 is kept in standard padded form), an integer
  beyond ±2^53 in metadata or a `data` part, and `metadata` that is not a JSON
  object. Over JSON they were stored and served, and the task then answered
  `INTERNAL` over gRPC.

Five more things change for an executor and the streams it feeds:

- A `Message` written after the task stream began is streamed as a
  `TaskStatusUpdateEvent` that keeps the state the turn last wrote and carries
  the message in `status.message`. A follow-up (a message naming the task's
  `taskId`) answered with a `Message` gets a task stream, a `Task` and then
  that update, and a blocking follow-up gets the `Task` with the message as
  its status message. A new task's first-event `Message` is still a direct
  reply.
- A streaming send whose first event is a `Message` is the message-only stream
  §3.1.2 describes: that one `Message`, then the stream closes. It used to
  lead with a `Task` snapshot, so the stream was `[Task, Message]`. An
  executor that wants a task stream and narrates first writes a status update
  before its first `Message`.
- Events written after the terminal status are not streamed, and a stream ends
  once the terminal event is delivered.
- A non-append artifact update for an id the task already holds replaces that
  artifact, in the blocking and background paths alike; it stored a second one.
- `RequestContext::activated_extensions()` lists only the extensions the card
  declares, in request order. An executor that read an undeclared one no
  longer finds it: declare it on the card. Without a card the list passes
  through.

A client that read a mid-task `Message` event reads `status.message` on the
status update instead:

```rust
use a2a_protocol_types::{Message, StreamResponse};

fn narration(event: &StreamResponse) -> Option<&Message> {
    match event {
        // 0.14 also delivered a mid-task `StreamResponse::Message`.
        StreamResponse::StatusUpdate(update) => update.status.message.as_ref(),
        _ => None,
    }
}
```

### Dashboards, alerts and rate-limit tables

- **Error labels.** `a2a.server.errors`' `error` label and the audit trail's
  `call` records said `protocol` for every A2A error a binding sent as it was.
  They now say `unauthenticated` or `permission_denied` for a refused
  credential, and the error code's label (`version_not_supported`,
  `content_type_not_supported`, ...) for any other.
- **Histogram buckets.** `rpc.server.call.duration`, `rpc.client.call.duration`
  and `a2a.server.request.duration` use the semantic conventions' current
  buckets, 0.1 ms to 100 s, where they used 5 ms to 10 s. The boundaries are
  part of a series' identity, so a stored histogram's quantiles across the
  upgrade mix the two sets until the old series ages out.
- **The executor's span** is `invoke_agent {card name}` where it was
  `a2a.execute`. An alert or query on the old name matches the new one, or the
  handler keeps the old span:

  ```rust
  use a2a_protocol_server::RequestHandlerBuilder;

  fn old_span_name(builder: RequestHandlerBuilder) -> RequestHandlerBuilder {
      builder.with_agent_span_conventions(false)
  }
  ```

- **`error.type`** on `rpc.server.call.duration` and the server span marks a
  server failure only, by each binding's conventions: every JSON-RPC error
  code; a 5xx but not a 4xx on HTTP+JSON; `UNKNOWN`, `DEADLINE_EXCEEDED`,
  `UNIMPLEMENTED`, `INTERNAL`, `UNAVAILABLE` and `DATA_LOSS` on gRPC. It was
  set for every status but success. `rpc.status_code` still carries every
  status, so a dashboard counting client errors reads that instead.
- **Rate-limit keys.** `RateLimitInterceptor` keys are `id:<identity>`,
  `ip:<address>`, `tenant:<id>` and `anon`; an identity was the bare key. A
  caller no interceptor identified shared one `"anonymous"` bucket, and with no
  trusted proxy hops it is now keyed by its peer address (an IPv6 address by
  its /64). A `RateLimitCounter` table sees the new spellings, and rows keyed
  the old way age out with their window.
- **Trace propagation.** With `otel` and a recording `tracing-opentelemetry`
  layer, the client sends a `traceparent` on every call, naming its own span,
  so the server span a call causes is the child of the client's. Opt out with
  `ClientBuilder::with_trace_propagation(false)`.

### Error codes some clients may match on

- A rate-limit refusal from `RateLimitInterceptor` is HTTP `429` with
  `Retry-After` and gRPC `RESOURCE_EXHAUSTED`, not `500` / `INTERNAL`. The
  JSON-RPC code is still `-32603`.
- On WebSocket, errors carry their A2A code and typed `data`: a lagged
  stream is `-32603` with the `streamLagged` marker, and an over-capacity
  request `-32603`, where both were `-32000`.
- SLIMRPC status codes follow the SLIMRPC specification's table:
  `JSONParseError` is `INTERNAL`, `ContentTypeNotSupportedError`
  `UNIMPLEMENTED`, `VersionNotSupportedError` `FAILED_PRECONDITION`.
- A credential refused over WebSocket is still `-32600`, now with an
  `ErrorInfo` from `a2a-rust.com` (`UNAUTHENTICATED` with the challenge as
  `www-authenticate` metadata, or `PERMISSION_DENIED`; the
  [binding specification](../bindings/websocket.md#6-authentication-126)
  §6). The SDK's client reports it as `ClientError::UnexpectedStatus` with
  status `401` or `403`, as over HTTP and gRPC; code that matched
  `ClientError::Protocol` with `ErrorCode::InvalidRequest` for it matches
  `UnexpectedStatus` instead.

### Also in 0.15, not breaking

- **Deprecated.** Besides `sign_agent_card` and
  `ClientConfig::preferred_bindings` (above):
  `HotReloadAgentCardHandler::update` is deprecated for `try_update`, which
  checks the card as `reload_from_json` does and refuses an invalid one with
  `InvalidParams`, leaving the current card in place. `OAuthFlows::Implicit`
  and `OAuthFlows::Password` are deprecated because the A2A 1.0 proto marks
  both; they stay, parse and serialize as before (spec §1.4 keeps deprecated
  definitions until the next major version), and code that names them gets a
  warning.
- **Several replicas sharing a store need session affinity.** Without every
  turn of a task routed to one replica, two replicas can run the same task at
  once, and a continuation saved on one can overwrite what the other's turn
  wrote meanwhile. One replica, or several with affinity, is the supported
  topology for 0.15; see
  [Running More Than One Replica](../deployment/horizontal-scaling.md).
- **`serve` and `serve_with_addr` cut off a slow peer.** They run on `Server`
  under `ServeConfig::default()`, with a 30-second header-read timeout and a
  75-second idle timeout (traffic in either direction counts, so an SSE stream
  with its 30-second keep-alive stays open). Their signatures are unchanged;
  for a ceiling, TLS or a graceful drain, use `Server` directly.
- **A header sent as several lines is one field.** Every binding joins the
  lines in order with `", "` (`"; "` for `cookie`), where each kept the last.
  `A2A-Extensions` on two lines activates both; a field that takes one value,
  such as `Authorization`, reaches the interceptors as both values joined, so a
  bearer token sent on two lines is refused.
- **A `Content-Type` is read as a media type**: the type and subtype before any
  `;` parameter, compared without regard to case. `Application/JSON` is
  accepted; `application/json-rpc` and `application/json-patch+json`, which a
  prefix match served, are refused with `ContentTypeNotSupportedError`.
- **On HTTP+JSON the path's id is the request's.** A body's `id` or `taskId` no
  longer replaces the path's, an empty path segment reaches its handler as an
  empty id and is `InvalidParams`, and the Axum router reads the body of
  `:cancel` and `:subscribe` (a malformed one is `400`).
- **JSON-RPC follows three more rules of JSON-RPC 2.0.** An `id` that is not a
  string, a number or `null` is `-32600`; omitted `params` read as `{}`; and
  `SendStreamingMessage` or `SubscribeToTask` inside a batch is
  `UnsupportedOperationError` (`-32004`), not `-32602`.
- **The WebSocket binding no longer routes `message/stream`**, the one v0.3
  method name it still served; it is `-32601`, as on every other binding.
- **SLIMRPC refuses a call that declares no `A2A-Version`** with
  `VersionNotSupportedError`, as the HTTP and gRPC bindings do by default.
  `SlimRpcServer::with_require_version(false)` restores the old behaviour.
- **ListTasks follows the proto.** `statusTimestampAfter` includes its bound
  ("greater than or equal to"), where every store excluded it, and `totalSize`
  is the number of tasks the filters match before pagination.
- **Listing or deleting the push configs of an unknown task is
  `TaskNotFound`** (`404` on HTTP+JSON, `-32001` on JSON-RPC, `NOT_FOUND` on
  gRPC); it answered an empty list and success. Deleting a config that is
  already gone from a task that exists still succeeds.

These are the ones a gateway, a test or a client is most likely to meet. The
rest are under `### Behaviour Changes` in the `## [0.15.0]` section of
CHANGELOG.md.

## 0.13 → 0.14

0.14.0 makes a failure read the same on every binding, and a refused credential
answer the status a client acts on. CHANGELOG.md lists seven breaking items and
ten behaviour changes (two more are listed under Fixed); the SLIMRPC binding's
has its own section at the end.
None of them is a compile error: the first two change what a build contains,
and the rest change what a `match` on an error, a status check or a gateway
rule sees, which only a test, a peer or a run notices.

### `tracing` is a default feature

Of `a2a-protocol-client`, `a2a-protocol-server` and `a2a-protocol-sdk` (ADR
0013). A default build used to compile every log call to nothing, so an agent
logged nothing whatever subscriber it installed. With no subscriber the cost is
a level check per call site. Nothing to do for most builds. To keep the old,
silent build, depend with `default-features = false`, and on the client or SDK
add back `features = ["tls-rustls"]` if you dial `https://`.

### `default-features = false` on the SDK removes rustls

The SDK used to take the client with the client's own defaults, so rustls stayed
in an SDK built without default features. A build that did that and still
reached `https://` agents was relying on the defect:

```toml
a2a-protocol-sdk = { version = "0.15", default-features = false, features = ["tls-rustls"] }
```

### A peer that goes away reads the same on every binding

The same event — a connection or stream ending before its final event — used to
be a retryable `Http` error on JSON-RPC and HTTP+JSON, a non-retryable
`Transport("WebSocket connection closed")` on WebSocket, and a non-retryable
`Protocol(InternalError)` on gRPC. Now, on WebSocket and gRPC as on the HTTP
bindings, a stream cut off ends with `ClientError::IncompleteStream`, a unary
call in flight fails with `HttpClient`, and a WebSocket handshake refused with an
HTTP status is `UnexpectedStatus`. A frame over the size cap, or one that breaks
the protocol, stays a non-retryable `Transport` error.

```text
// 0.13
Err(ClientError::Transport(_)) => reconnect(),               // WebSocket drop
Err(ClientError::Protocol(e)) if e.code == ErrorCode::InternalError => resume(), // gRPC cut
```

Match the new variants, or ask `is_retryable()`, which is true for both:

```rust
use a2a_protocol_client::ClientError;

fn peer_went_away(err: &ClientError) -> bool {
    matches!(err, ClientError::IncompleteStream { .. } | ClientError::HttpClient(_))
}
```

### gRPC `UNAUTHENTICATED` and `PERMISSION_DENIED` are `401` and `403`

Over the gRPC transport and the SLIMRPC binding's client they are now
`ClientError::UnexpectedStatus { status: 401, .. }` and `{ status: 403, .. }`,
the shapes JSON-RPC and REST already reported; they were
`Protocol(InvalidParams)` on gRPC and `Protocol(InternalError)` on SLIMRPC.
`BearerAuthInterceptor` now drops a token a gRPC agent refused. A peer's
`CANCELLED` is a non-retryable `Protocol` error rather than a retryable
`Timeout`, so a retry policy stops re-sending a call the peer abandoned.

```rust
use a2a_protocol_client::ClientError;

fn credential_refused(err: &ClientError) -> bool {
    // was: ClientError::Protocol(e) if e.code == ErrorCode::InvalidParams
    matches!(err, ClientError::UnexpectedStatus { status: 401 | 403, .. })
}
```

### `ClientBuilder::from_card` refuses a card with no interface for protocol major 1

It used to accept such a card with a warning and fail at the first call with
whatever the wire produced — a v0.3 endpoint answers `-32601 method not found`.
The error now lists what the card offers. Versions are read leniently: `v1.0`
and `1-preview` both count as major 1. (An empty one counted too until 0.15; it
no longer does.) If a peer's card is wrong but the endpoint speaks 1.0, build
from the URL with `ClientBuilder::new`.

### Graceful shutdown ends in-flight tasks before it drains connections

`Server::serve_with_shutdown` used to drain first, and an open SSE stream does
not close until its task ends, so the drain waited out its 15 s and delegated
tasks outlived the process. The order is now: stop accepting; let tasks finish
for up to `ServeConfig::completion_grace` (5 s); cancel the rest and give their
executors `task_grace` (10 s) to cancel what they delegated and write a terminal
event; then drain (`drain_timeout`, 15 s). The trade: a task that outlives the
completion window is now cancelled where it used to get the whole drain window.
If your tasks need longer:

```rust
use std::time::Duration;
use a2a_protocol_server::ServeConfig;

let config = ServeConfig::default().with_completion_grace(Duration::from_secs(30));
# let _ = config;
```

The gRPC and WebSocket dispatchers gain their own `serve_with_shutdown` with the
same three setters.

### Shipped `TaskStore`s refuse to move a terminal task to another state

Atomically with the write, answering `UnsupportedOperation` with a
`TerminalStateConflict` in its data
(`a2a_protocol_server::store::TerminalStateConflict::from_error` recognises it).
Code that reopened a finished task through a shipped store gets that error; send
the follow-up as a new task instead. A custom store is not covered unless it
applies `store::refuses_write` inside its writes.

### Behaviour changes

No type or signature changed for these; each changes what a caller observes.

- **A refused credential answers `401`/`403`, not `400`** (ADR 0014). The auth
  interceptors used to answer every refusal as `InvalidRequest`: `400` on both
  HTTP bindings, `INVALID_ARGUMENT` on gRPC. A missing or wrong credential now
  answers `401` with a `WWW-Authenticate` challenge (`Bearer realm="a2a"`, or
  `ApiKey header="x-api-key"`, naming the configured header), and gRPC
  `UNAUTHENTICATED`; the AIP-193 body's `status` says `UNAUTHENTICATED`. JSON-RPC
  answers the same `401` with its body unchanged (`-32600`); a batch stays `200`.
  WebSocket, which has no status per call, sends the body alone. Update any
  client, test or alert that matched `400` for an auth failure. A custom
  authenticating interceptor gets the same statuses by returning:

  ```rust
  use a2a_protocol_types::AuthRejectionKind;
  use a2a_protocol_types::error::A2aError;

  let err = A2aError::unauthenticated("authentication required", "Bearer realm=\"a2a\"");
  assert_eq!(
      err.auth_rejection().map(|r| r.kind()),
      Some(AuthRejectionKind::Unauthenticated)
  );
  let _forbidden = A2aError::permission_denied("not permitted");
  ```

- **A message part whose `mediaType` the card does not declare is refused**
  with `ContentTypeNotSupportedError` (-32005 over JSON-RPC, HTTP `400` over
  HTTP+JSON), on `SendMessage` and `SendStreamingMessage`, when the card
  declares input modes — `defaultInputModes` or any skill's `inputModes`. Parts
  without a `mediaType` are not checked. Declare every type the agent accepts,
  or keep the old behaviour while the card is corrected:

  ```rust
  use a2a_protocol_server::RequestHandlerBuilder;

  fn lenient(builder: RequestHandlerBuilder) -> RequestHandlerBuilder {
      builder.allow_undeclared_input_modes()
  }
  ```

- **The axum adapter's error bodies are AIP-193**, as `RestDispatcher`'s are:
  `{"error": {"code", "status", "message", "details"}}` instead of
  `{"error": "<text>"}`. A client that read `error` as a string reads
  `error.message`.
- **The axum adapter's successful responses carry `A2A-Version`**, as its
  errors already did. HTTP+JSON responses stay `application/json` on purpose:
  the official Go SDK's client cannot read an error labelled
  `application/a2a+json`. (0.15 changed this; see
  [HTTP+JSON answers `application/a2a+json` unless asked otherwise](#httpjson-answers-applicationa2ajson-unless-asked-otherwise).)
- **JSON that is not a JSON-RPC Request object is answered `-32600` (Invalid
  Request), not `-32700` (Parse error)**, over JSON-RPC and WebSocket: a body
  with no `method`, a bare value such as `1`, an empty batch `[]`, a batch item
  that is not a request. A client that matched `-32700` for these sees `-32600`.
  (Listed under **Fixed** in the changelog.)
- **Every task status carries a timestamp.** The event queue stamps any status
  update or task snapshot the executor left without one; a timestamp the
  executor set is kept. The stamp counts toward `max_event_size`, so an event
  within a few dozen bytes of the limit may now be refused. (Listed under
  **Fixed**.)
- **A continuation sent the moment a task reaches `input-required` is admitted**
  instead of refused as "already being processed" while the executor that
  parked it is still returning; admission waits up to
  `HandlerLimits::executor_drain_timeout` (5 s) for it.
- **An idle `SubscribeToTask` stream over SSE ends at
  `HandlerLimits::subscribe_max_idle`** (5 min). It used to stay open as long as
  its client did. Resubscribe, or raise the bound with `with_subscribe_max_idle`.
  (0.15 removed the default bound; see
  [A parked subscription stays open until its task finishes](#a-parked-subscription-stays-open-until-its-task-finishes).)
- **A WebSocket request whose connection ends is cancelled** on the server,
  as a request on the HTTP bindings already was when its client disconnected.
- **WebSocket client: a stream the caller reads more slowly than the agent
  writes ends with a `stream_lagged` error** once 64 frames are waiting
  (`ClientError::is_stream_lagged`); resubscribe to continue. It used to be
  held back, which stalled every other call on the socket.
- **WebSocket client: a call on a `WebSocketTransport` whose socket dropped
  reconnects** to the same endpoint with the same configuration, instead of
  failing with a non-retryable `Transport` error. Streams already open keep the
  connection they were opened on.

Two changes to interceptors are additive but worth reading:
`ServerInterceptor::after` now runs once the response exists, so a failing one
no longer orphans a send's task (listed under **Fixed**), and the new, defaulted `on_complete(ctx, outcome)` is
called on every call whose `before` ran — with `CallOutcome::Succeeded`,
`Failed(&ServerError)` or `Cancelled` — which is where to release something
`before` acquired, since `after` runs only on success.

### SLIMRPC binding: a refused credential answers `UNAUTHENTICATED` / `PERMISSION_DENIED`

`bindings/a2a-protocol-slimrpc`'s server sent `INVALID_ARGUMENT` for a credential
an interceptor refused, as the gRPC dispatcher did before ADR 0014, so a
client's `BearerAuthInterceptor` never dropped a revoked token there. It now
answers `UNAUTHENTICATED` for `A2aError::unauthenticated` and `PERMISSION_DENIED`
for `A2aError::permission_denied`, which this SDK's client reads as `401` and
`403`. An ordinary `InvalidRequest` keeps `INVALID_ARGUMENT`.

## 0.12 → 0.13

0.13.0 makes the event log the record and spends it on stream resumption.
Nine breaking items. The first is the one most code will meet; the third is
a rename you fix at the read site; the last five were found by an audit after
the first three were written down, and two of them are attributes or types
that only bite a `match` or a binding; the other three change behaviour. The
ninth shipped in 0.13.0 but was listed under `[Unreleased]` until 2026-09-23;
its section is the last below.

### `RequestContext` is `#[non_exhaustive]`, and gains `call_context`

Building one with a struct literal from outside `a2a-protocol-server` stops
compiling. `cargo semver-checks check-release -p a2a-protocol-server
--baseline-version 0.12.1` ran 196 checks in the run recorded when this was
written and failed one, `struct_marked_non_exhaustive` on this type. That run
was not exhaustive: the `PurgeReport` rename below makes
`struct_pub_field_missing` fire too. The added field is not a second finding:
once a struct is `#[non_exhaustive]` an added field is no longer separately
observable.

```text
// 0.12 — a struct literal, from outside the crate
let ctx = RequestContext {
    message,
    task_id,
    context_id,
    stored_task: None,
    metadata: None,
    cancellation_token: CancellationToken::new(),
};
```

`new` takes the three fields with no sensible default; everything else has a
`with_*`:

```rust
use a2a_protocol_server::RequestContext;
use a2a_protocol_types::{Message, TaskId};

let ctx = RequestContext::new(
    Message::user_text("msg-1", "3 + 5"),
    TaskId::new("task-1"),
    "ctx-1".to_owned(),
)
.with_metadata(serde_json::json!({ "tier": "gold" }));
# let _ = ctx;
```

`with_stored_task` and `with_call_context` are the other two in 0.13; later
releases add more `with_*` setters. *Reading* a
`RequestContext` is unaffected — the fields are still public, so an executor
that only reads `ctx.message` needs no change. The attribute is deliberate
rather than incidental: this type grows, and every previous growth would have
broken a literal nobody writes.

### `EventQueueReader::read` yields a `StreamEvent`, not a `StreamResponse`

The event now travels the queue with the position it holds in the task's log:
`StreamEvent { seq: Option<u64>, event: StreamResponse }`. `Reattached::Channel`
and the persistence channel carry the same type. Only code that matched the
result of `read()` directly is affected.

```text
// 0.12
while let Some(Ok(StreamResponse::StatusUpdate(update))) = reader.read().await {
    handle(update);
}
```

Reach through `.event` for a value, or `.map(|e| e.event)` for the `Result`:

```rust
use a2a_protocol_server::{EventQueueReader, InMemoryQueueReader};
use a2a_protocol_types::StreamResponse;

async fn drain<R: EventQueueReader>(reader: &mut R) {
    while let Some(Ok(ev)) = reader.read().await {
        if let StreamResponse::StatusUpdate(update) = ev.event {
            // `ev.seq` is the number the store wrote and the SSE `id:` carries.
            println!("{:?} {:?}", ev.seq, update.status.state);
        }
    }
}
# let _ = drain::<InMemoryQueueReader>;
```

`StreamEvent` is itself `#[non_exhaustive]`, so destructure it with `..` or —
as above — reach through `.seq` and `.event`.

`seq` is `None` for frames the server synthesized rather than the agent
emitting — the `SubscribeToTask` snapshot, and the terminal frame the reattach
hook builds from stored state. Those are not in the log, so they carry no `id:`
and a resuming client's offset is unaffected by having seen them. It is a type
change rather than an accessor because the position has to be the *same* number
the store wrote, and the only way to guarantee that is to assign it once and
carry it.

### `PurgeReport::journal_orphans_deleted` is now `orphan_rows_deleted`

The retention sweep reclaims two side tables now, not one — the artifact
journal and the event log — so the old name described half of what the number
counts. Renamed rather than kept and widened: a field whose name names one of
its two sources is read as the count for that source.

```text
// 0.12
tracing::info!(orphans = report.journal_orphans_deleted, "retention sweep");
```

Rename it at the read site. The meaning is unchanged for anyone who had only
the journal:

```rust
use a2a_protocol_server::store::PurgeReport;

fn summarize(report: &PurgeReport) -> String {
    format!(
        "{} tasks, {} orphan side-table rows, complete={}",
        report.tasks_deleted, report.orphan_rows_deleted, report.complete,
    )
}
# let _ = summarize(&PurgeReport::default());
```

It is still normally zero: both side tables carry an `ON DELETE CASCADE`, so a
non-zero count means rows outlived their task — which happens on a `SQLite`
pool handed to `from_pool` without `foreign_keys=ON`.

### `IdempotencyClaim` and `KeyError` are `#[non_exhaustive]`

Both are enums a caller matches on. A `match` from outside the defining crate
now needs a wildcard arm — and should name it, because a future variant
reaching a silent catch-all is how a new outcome gets folded into the wrong
one.

`IdempotencyClaim` is also re-exported from `a2a_protocol_server::store` now,
beside `RecordedEvent`. It is the return type of a `TaskStore` method, so an
out-of-tree store has to name it, and it was reachable only at
`store::task_store::IdempotencyClaim`.

```rust
use a2a_protocol_server::store::IdempotencyClaim;
use a2a_protocol_types::task::TaskId;

fn describe(claim: IdempotencyClaim) -> String {
    match claim {
        IdempotencyClaim::Claimed => "create the task".to_owned(),
        IdempotencyClaim::Replay(id) => format!("return {id}"),
        IdempotencyClaim::Conflict { held_by } => format!("refuse; held by {held_by}"),
        // Required from 0.13. Naming it beats a silent fallthrough.
        other => format!("unhandled claim outcome: {other:?}"),
    }
}
# fn main() { let _ = describe(IdempotencyClaim::Replay(TaskId::new("t"))); }
```

### `FailureClass::ALL` is a slice

It was `[FailureClass; 5]`, so the length was in the type and the sixth
variant would have broken every caller that bound it — the break
`#[non_exhaustive]` on the enum exists to prevent.

```rust
use a2a_protocol_types::failure::FailureClass;

fn tokens() -> Vec<&'static str> {
    // was: for c in FailureClass::ALL
    FailureClass::ALL.iter().map(|c| c.as_str()).collect()
}
# fn main() { assert!(!tokens().is_empty()); }
```

### `build()` refuses a signed agent card it would have to edit

The builder advertises the extensions this server can honour by appending to
`capabilities.extensions`, and a card signature covers everything except
`signatures` — so appending to a card that is already signed leaves the served
card canonicalizing to bytes nobody signed, and every client that verifies it
fails while this side reports nothing.

Declare the extensions before signing. A card that already declares them
builds and is served byte-identical to what was signed.

```rust
use a2a_protocol_types::extensions::AgentExtension;
use a2a_protocol_types::failure::FAILURE_EXTENSION_URI;
use a2a_protocol_types::idempotency::IDEMPOTENCY_EXTENSION_URI;

# fn example(card: &mut a2a_protocol_types::agent_card::AgentCard) {
// Before signing, not after:
card.capabilities.extensions = Some(vec![
    AgentExtension::new(IDEMPOTENCY_EXTENSION_URI),
    AgentExtension::new(FAILURE_EXTENSION_URI),
]);
# }
# fn main() {}
```

Declare `IDEMPOTENCY_EXTENSION_URI` only when the store you configure reports
`supports_idempotency()`; `FAILURE_EXTENSION_URI` is advertised on every
server. From 0.15 the builder also advertises `BUDGET_EXTENSION_URI` on every
card (see
[A signed agent card declares the budget extension](#a-signed-agent-card-declares-the-budget-extension)),
`LEASE_EXTENSION_URI` when a lease store is configured and
`APPROVAL_EXTENSION_URI` when an approval gate is; a signed card must declare
each of them.

### A keyed `message/send` is retried only against a peer that advertises it

`RetryTransport` treated any valid key in `Message.metadata` as making the
send retryable. The key rides in a field A2A defines as free-form, and the
extension is not part of A2A v1.0, so a conformant peer from another SDK
ignores it and runs the send — and the retry starts a second task.

`ClientBuilder::from_card` reads the advertisement and enables it for you.
For a client built on a bare endpoint, assert it only for a peer you know
implements the extension:

```rust
use a2a_protocol_client::ClientBuilder;

# fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
let client = ClientBuilder::new("http://localhost:8080")
    .with_peer_honouring_idempotency(true)
    .build()?;
# let _ = client;
# Ok(())
# }
# fn main() {}
```

### `message.id` is validated at ingress

It is checked like `context_id` and `task_id` now — non-empty after trimming,
and within `HandlerLimits::effective_max_message_id_length()`, which is
`max_id_length` but never below 36, the length of a UUID, so tightening
`max_id_length` does not refuse conformant clients. It was checked nowhere, so
an empty `messageId` was accepted, stored, and echoed in task history, where
every empty id collides with every other.

No code change is needed unless you were sending empty or very long message
ids. If you generate them, `uuid::Uuid::new_v4().to_string()` is what the
examples use.

### `RetentionPolicy` and `PurgeReport` are `#[non_exhaustive]`

The ninth item. `RetentionPolicy` was missed by 0.12's conversion of the
configuration structs, and `PurgeReport` is marked for the same reason: both
gain fields (`idempotency_key_max_age`, `idempotency_keys_deleted`) that would
otherwise have broken a literal. It shipped in 0.13.0, but the changelog listed
it under `[Unreleased]` until 2026-09-23, so the 0.13.0 release notes do not
mention it.

Build a policy with `new` and the setters, and read a report's fields rather
than destructuring it without `..`:

```rust
use std::time::Duration;
use a2a_protocol_server::store::RetentionPolicy;

let policy = RetentionPolicy::new(Duration::from_secs(7 * 24 * 3600))
    .with_batch_size(500);
# let _ = policy;
```

### Also in 0.13, not breaking

- **`TaskStore` gains nine methods, all defaulted.** Six for the event log
  (`supports_event_log`, `append_event`, `last_event_seq`, `read_events`,
  `earliest_event_seq`, `event_log_covers`) and three for idempotency
  (`supports_idempotency`, `claim_idempotency_key`,
  `release_idempotency_key`). A custom store keeps compiling, and the defaults
  report *no* support rather than a successful claim or an empty log — so
  forgetting to implement them cannot quietly produce a send that runs twice or
  a resumption that silently skips events.
- **`bindings/a2a-protocol-slimrpc` moves to `0.5.0`**, pinned to `0.13`. It is
  outside the root workspace and versioned independently, so this is not a
  break of the four published crates; it bumps in this release rather than
  after it because the binding depends on the workspace by `path` as well as by
  version, and `^0.12` stops resolving the moment the crates read 0.13.0. See
  [its chapter](../bindings/slimrpc.md).

## 0.11 → 0.12

0.12.0 is the deliberate breaking minor before the two clean minors that
STABILITY.md §7 requires, so every "this struct should have been
`#[non_exhaustive]`" fix lands here rather than one per release. Five items;
the fourth is the one most code will meet.

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

Use `Default` and the setters. Every field has one (fourteen in 0.12):

```rust
use std::time::Duration;
use a2a_protocol_server::HandlerLimits;

let limits = HandlerLimits::default()
    .with_max_id_length(2048)
    .with_push_delivery_budget(Duration::from_secs(30)) // new in 0.12
    .with_executor_drain_timeout(Duration::from_secs(5)); // new in 0.12
# let _ = limits;
```

A literal that ended in `..HandlerLimits::default()` was not broken by the
new fields — but it is by the fourth item below, which makes the struct
`#[non_exhaustive]`; the setter form above is the one that survives both.
Hand the result to `RequestHandlerBuilder::with_handler_limits` as before.

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

### Fifteen configuration structs are `#[non_exhaustive]`

Server: `HandlerLimits`, `DispatchConfig`, `CorsConfig`, `GrpcConfig`,
`CacheConfig`, `PushRetryPolicy`, `RateLimitConfig`, `TaskStoreConfig`,
`TenantStoreConfig`, `PerTenantConfig`, `TenantLimits`. Client:
`ClientConfig`, `RetryPolicy`, `WebSocketTransportConfig`,
`GrpcTransportConfig`. (`ServeConfig` already was; the wire types in
`a2a-protocol-types` are deliberately not on the list — a literal is how the
spec's data types are meant to be built.) None of them will gain a field at
your expense again.

The rule is the same for all fifteen: a struct literal of any shape —
`..Default::default()` included — is an error outside the crate, and every
public field has a `with_<field>` setter. Before:

```text
// 0.11
let limiter = RateLimitInterceptor::new(RateLimitConfig {
    requests_per_window: 100,
    window_secs: 60,
    ..RateLimitConfig::default()
})?;
```

After — start from `Default` (or the documented constructor:
`CorsConfig::new`/`permissive`, `CacheConfig::with_max_age`) and chain the
setters for the fields you set:

```rust
use a2a_protocol_server::{RateLimitConfig, RateLimitInterceptor};
# fn example() -> Result<(), a2a_protocol_server::ServerError> {
let limiter = RateLimitInterceptor::new(
    RateLimitConfig::default()
        .with_requests_per_window(100)
        .with_window_secs(60),
)?;
# let _ = limiter;
# Ok(())
# }
```

Some setters did not exist before this release and were added for it, so a
missing one is a 0.11 build, not a missing field: `RateLimitConfig`'s four,
`TaskStoreConfig`'s four (`with_max_capacity`, `with_task_ttl`,
`with_eviction_interval`, `with_max_page_size`), `CorsConfig`'s four,
`TenantStoreConfig::with_per_tenant`/`with_max_tenants`,
`PerTenantConfig::with_default`/`with_overrides`/`with_override`,
`TenantLimits::with_*` for each of its fields (its `builder()` still works),
`DispatchConfig::with_require_version_header`, and ten on `ClientConfig`.
The setters for `Option` fields take the `Option`, so `None` is spelled
where the literal spelled it:

```rust
use a2a_protocol_server::TaskStoreConfig;

let no_ttl = TaskStoreConfig::default()
    .with_max_capacity(Some(50_000))
    .with_task_ttl(None);
# let _ = no_ttl;
```

`GrpcTransportConfig` also gained `bare_address_scheme` and, under the
`grpc-tls` feature, `tls_config`; every field has a setter there too:

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

`with_tls_config(tonic::transport::ClientTlsConfig)` is its sixth setter,
behind `grpc-tls`.

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
a stale vendored copy of the specification's table. Upstream did not amend the
document in place, as this entry previously said: the corrections shipped as
the tagged patch release **v1.0.1** on 2026-05-28, and A2A's `v1.0.0` tag
still carries the superseded table.

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
    TaskStoreConfig::default().with_max_capacity(Some(500)),
);
# let _ = store;
```

The override applies to a partition created after the store is built; a
tenant that already has one keeps it. The field was deprecated rather than
removed until 0.15.0, which removes it.

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

Until 0.14, `queues_force_destroyed` was always `0` for `shutdown()`, whatever
it cut off; it now counts the live queues it destroyed.
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
Event-queue writes to the broadcast side never block — a slow
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

The two public setters are gone. `DEFAULT_WRITE_TIMEOUT` (5 s) is still
exported — 0.8 kept it, and a then-dead field on `InMemoryQueueWriter`, rather
than change that constructor's arity — and since 0.10 it is enforced: a write
that cannot reach the persistence channel within it fails with an error naming
the stalled processor.

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

## 0.6 → 0.7

The boundary that costs a consumer of `a2a-protocol-types` alone the most:
the only two changes between 0.7 and 0.12 that stop such code compiling are
both here. (0.13 and 0.15 add more for that crate, such as `FailureClass::ALL`
and, with the `proto` feature, a `ConvertError` literal.) Neither appears under
a `### Breaking Changes` heading — 0.7.0 has no such heading — and both sit
under `### Changed` in a release with sixteen subsections, so they are not
findable by chance.

### `JsonRpcRequest.id` is the three-state `JsonRpcRequestId`

`JsonRpcRequest.id` was `Option<serde_json::Value>`, which cannot tell an
absent `id` member from an explicit `"id": null`. JSON-RPC 2.0 does: the
first is a notification, the second is a call. It is now an enum with all
three states, so `"id": null` stops collapsing into a notification on
round-trip.

Responses did **not** change. `JsonRpcId` is still
`Option<serde_json::Value>`, and it is still what `JsonRpcSuccessResponse.id`
and `JsonRpcErrorResponse::new` take. Code that threaded a request id
straight into a response stops compiling:

```text
// 0.6 — the request id went directly into the response
fn error_response(id: Option<serde_json::Value>, error: JsonRpcError) -> JsonRpcErrorResponse {
    JsonRpcErrorResponse::new(id, error)
}
```

`to_response_id()` is the bridge. It maps `Absent` and `Null` alike to
`None`, mirroring the specification's rule for a request whose id could not
be determined, so the bytes on the response are what they were before:

```rust
use a2a_protocol_types::jsonrpc::{JsonRpcError, JsonRpcErrorResponse, JsonRpcRequestId};

fn error_response(id: &JsonRpcRequestId, error: JsonRpcError) -> JsonRpcErrorResponse {
    JsonRpcErrorResponse::new(id.to_response_id(), error)
}

let replied = error_response(
    &JsonRpcRequestId::Value(serde_json::json!(1)),
    JsonRpcError::new(-32600, "Invalid Request"),
);
assert_eq!(replied.id, Some(serde_json::json!(1)));

// Both non-value states answer with a null id, as they did before.
let notification = error_response(
    &JsonRpcRequestId::Absent,
    JsonRpcError::new(-32600, "Invalid Request"),
);
assert_eq!(notification.id, None);
```

In practice a server threads the request id through one or two local helpers
like the one above, so changing those signatures is usually the entire
migration; every call site that passes an id into them keeps compiling
untouched.

The new capability is worth a second look while you are there. A server can
now see `JsonRpcRequestId::Absent` and decline to answer at all, which is
what JSON-RPC 2.0 requires for a notification — `is_absent()` is provided for
exactly that. Migrating with `to_response_id()` alone preserves the old
behaviour of replying to everything; it does not adopt the new one.

### `AuthenticationInfo.credentials` and `TaskPushNotificationConfig::task_id` are `Option<String>`

Both were required, which rejected valid cross-SDK payloads at parse time — a
push configuration nested in `SendMessageConfiguration`, before the task it
will belong to exists, legitimately carries no task id. Both are now
`Option<String>`, matching the canonical schema.

The struct literal is the obvious break:

```text
// 0.6
let auth = AuthenticationInfo {
    scheme: "bearer".to_string(),
    credentials: "my-token".to_string(),
};
```

```rust
use a2a_protocol_types::push::AuthenticationInfo;

let auth = AuthenticationInfo {
    scheme: "bearer".to_string(),
    credentials: Some("my-token".to_string()),
};
# let _ = auth;
```

The non-obvious break is the one to search for. Any `format!` that
interpolated `credentials` stops compiling for want of `Display`, and rustc
suggests `{:?}` in its note — which builds an `Authorization` header reading
`Bearer Some("my-token")`. `unwrap_or_default()` sends an empty bearer token
instead. Neither is right: omit the header when there is nothing to send.

```text
// 0.6 — and note that neither `{:?}` nor `unwrap_or_default()` is the fix
request.header("Authorization", format!("Bearer {}", auth.credentials))
```

```rust
use a2a_protocol_types::push::AuthenticationInfo;

let auth = AuthenticationInfo { scheme: "bearer".to_string(), credentials: None };

let header = auth
    .credentials
    .as_deref()
    .map(|credentials| format!("Bearer {credentials}"));

// No credentials configured, so no header at all — not an empty one.
assert!(header.is_none());
```

On the server side, a standalone `CreateTaskPushNotificationConfig` carrying
no task id is now refused with a structured invalid-params error rather than
a parse error, and every push-config store guards the missing routing key
explicitly. A handler of your own that forwards `task_id` onward has the same
decision to make: an absent id is a malformed request, not a default.

### Also in 0.7, changing what parses rather than what compiles

Three tightenings reject input that previously got through. A `Part` carrying
more than one of `text` / `raw` / `url` / `data` now fails deserialization
instead of silently taking the first match. A `JsonRpcResponse` carrying both
`result` and `error` is rejected per JSON-RPC 2.0 §5 instead of being read as
a success with the error discarded, and a mistyped `result` now surfaces the
real type error rather than an opaque "no variant matched". And RFC 8785
canonicalization for signing now sorts object keys by UTF-16 code units
(§3.2.3) and formats doubles per ECMAScript `Number::toString` (§3.2.2),
which changes the signature computed over any card containing
supplementary-plane keys — a card signed under 0.6 and verified under 0.7 can
disagree.

### Also in 0.7, on the server and client

This is where the JSON tunnel went behind `grpc-legacy-json` and
`with_event_queue_write_timeout` / `with_write_timeout` became deprecated
no-ops, all three slated for the 0.8 removals above. It is also where a
consumer that falls behind the broadcast ring started receiving a marked
`streamLagged` error and a closed stream instead of a silently skipped gap,
where `max_concurrent_streams` gained a default cap of 1024 (previously
unlimited, an unauthenticated denial-of-service vector), and where
`RateLimitInterceptor::new` became fallible and stopped trusting a
client-supplied `X-Forwarded-For` unless `RateLimitConfig::trusted_proxy_hops`
is set.

## Older boundaries

Before 0.8 the changelog was the migration guide. It still is for these; only
0.5 → 0.6 breaks nothing at compile time (0.4 → 0.5 changes the `TaskStore`
signatures, and 0.3 → 0.4 the wire format, `PartContent` and `OAuthFlows`),
and the 0.6 → 0.7 boundary has a full section above:

- **0.5 → 0.6** (`## [0.6.0] - 2026-06-10`): no `Breaking` heading, and
  nothing that fails to compile — no public API signature changed, which is
  what makes this the easiest boundary to cross without noticing. A
  types-only consumer gets a clean build and a different service. What moved
  is the bytes on the wire and which requests are accepted, both breaking
  under the policy above, so read `### Changed` and `### Fixed` for this
  release rather than looking for a section that is not there.
  `Task.history` is populated for the first time — nothing had ever appended
  to it — so `tasks/get` now returns real history subject to `historyLength`,
  capped at 1,024 messages with the oldest dropped first, while `message/send`
  responses and streaming snapshots omit it unless
  `SendMessageConfiguration.historyLength` asks; response sizes move in both
  directions. A dropped `message/stream` connection no longer cancels the
  task: work runs to completion and clients reattach with `SubscribeToTask`,
  so anything that relied on disconnect-kills-task must now call `CancelTask`
  explicitly. `Working → Working` became a valid transition, so an executor
  emitting more than one progress note is no longer marked
  `TASK_STATE_FAILED` by the background processor behind a stream that
  already said `Completed` — if you worked around that split-brain, the
  workaround can go. Continuations carry artifacts, metadata and history
  forward instead of overwriting them, with only the status returning to
  `Submitted`. `tasks/resubscribe` joins `SubscribeToTask` and
  `tasks/subscribe` as a routed alias. And on the client, a JSON-RPC error
  answering `message/stream` surfaces as `ClientError::Protocol` carrying the
  original code, instead of ending the stream with zero events and no error,
  so code that read an empty stream as "nothing happened" now sees the
  failure it was missing.
- **0.4 → 0.5** (`## [0.5.0] - 2026-04-02`, `### Breaking Changes`):
  `TaskStore::save()` and `TaskStore::insert_if_absent()` take `&Task`
  instead of an owned `Task`.
- **0.3 → 0.4** (`## [0.4.0] - 2026-03-31`, `### Breaking Changes`): the
  move to the A2A v1.0.0 wire format. Clients and servers on the old wire
  format need that section in full.
