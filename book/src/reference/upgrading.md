<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Upgrading Between Minor Versions

As of 2026-09-25, with 0.14.0 in preparation. The newest minor boundary this
page covers is 0.13 → 0.14, whose breaking section is `## [Unreleased]` in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
until the release names it `## [0.14.0]`.
(0.12.1 is a patch and breaks nothing, so it has no section of its own here.)

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

## 0.13 → 0.14

0.14.0 makes a failure read the same on every binding, and a refused credential
answer the status a client acts on. CHANGELOG.md lists seven breaking items and
nine behaviour changes; one more change, in the SLIMRPC binding, is at the end.
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
a2a-protocol-sdk = { version = "0.14", default-features = false, features = ["tls-rustls"] }
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
The error now lists what the card offers. Versions are read leniently: empty,
`v1.0` and `1-preview` all count as major 1. If a peer's card is wrong but the
endpoint speaks 1.0, build from the URL with `ClientBuilder::new`.

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
  `application/a2a+json`.
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
the first three were written down, and four of them are attributes or types
that only bite a `match` or a literal. The ninth shipped in 0.13.0 but was
listed under `[Unreleased]` until 2026-09-23; its section is the last below.

### `RequestContext` is `#[non_exhaustive]`, and gains `call_context`

Building one with a struct literal from outside `a2a-protocol-server` stops
compiling. `cargo semver-checks check-release -p a2a-protocol-server
--baseline-version 0.12.1` runs 196 checks and reports exactly one failure,
`struct_marked_non_exhaustive` on this type. The added field is not a second
finding: once a struct is `#[non_exhaustive]` an added field is no longer
separately observable.

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

`with_stored_task` and `with_call_context` are the other two. *Reading* a
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
server.

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

It is checked with the same rule as `context_id` and `task_id` now — non-empty
after trimming, and within `HandlerLimits::max_id_length`. It was checked
nowhere, so an empty `messageId` was accepted, stored, and echoed in task
history, where every empty id collides with every other.

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

- **`TaskStore` gains seven methods, all defaulted.** Four for the event log
  (`supports_event_log`, `append_event`, `last_event_seq`, `read_events`) and
  three for idempotency (`supports_idempotency`, `claim_idempotency_key`,
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
the only two changes on this page that stop such code compiling are both
here. Neither appears under a `### Breaking Changes` heading — 0.7.0 has no
such heading — and both sit under `### Changed` in a release with sixteen
subsections, so they are not findable by chance.

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

Before 0.8 the changelog was the migration guide. It still is for these,
which break nothing at compile time — the 0.6 → 0.7 boundary, which does,
has a full section above:

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
