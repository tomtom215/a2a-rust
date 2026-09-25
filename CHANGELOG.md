<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- **`tracing` is a default feature of `a2a-protocol-client`,
  `a2a-protocol-server` and `a2a-protocol-sdk`.** A default build used to
  compile every log call and failure report in the crates to nothing, so an
  agent logged nothing however its subscriber was set up, and paths that
  report only through `tracing` — a trace header the WebSocket binding
  cannot carry, a skipped webhook — were silent (audit O13). The maintainer
  chose default-on (ADR 0013). `tracing` was already in every default build's
  dependency graph through the HTTP stack; the feature adds
  `tracing-attributes` and `syn`. With no subscriber installed the cost is a
  level check per call site. **Migration:** none for most builds. To keep the old behaviour,
  depend with `default-features = false` (and re-add `tls-rustls` on the SDK
  if you want HTTPS).

- **`default-features = false` on `a2a-protocol-sdk` now removes what the
  SDK's defaults enable.** The SDK took the client with the client's own
  defaults, so rustls stayed in an SDK built without default features,
  contrary to its manifest (audit K1). The SDK now takes the client and
  server without their defaults and forwards its own: a default build
  enables exactly what it did (plus `tracing`, above), and
  `cargo tree -p a2a-protocol-sdk --no-default-features` has no rustls,
  hyper-rustls or webpki-roots. **Migration:** a build that used
  `default-features = false` and still reached `https://` agents was relying
  on the defect; add `features = ["tls-rustls"]`.

- **A peer that goes away mid-call reads the same on every binding.** Found
  by driving each binding against a scripted peer that cuts streams off
  (audit N13): the same event — the connection or stream ending before a
  final event — was a retryable `Http` error on JSON-RPC and HTTP+JSON, a
  non-retryable `Transport("WebSocket connection closed")` on WebSocket, and
  a non-retryable `Protocol(InternalError)` on gRPC, so retry and resume
  logic, and the failure class an executor reports, depended on the binding.
  Now, over WebSocket, a stream in flight when the socket drops ends as
  `IncompleteStream` (retryable, as `EventStream` rules on every other
  binding), a unary call in flight fails with `HttpClient` (retryable), a
  connection that cannot be made is `HttpClient`, and a handshake the peer
  answers with an HTTP status is `UnexpectedStatus { status, .. }` — so a
  401 reaches `BearerAuthInterceptor`, which drops the refused token, as it
  already did over HTTP and gRPC. A frame over the size cap, or one that
  breaks the protocol, stays a non-retryable `Transport` error. Over gRPC, a
  response that ends with no `grpc-status` trailer — what tonic reports for a
  truncated stream — ends a stream as `IncompleteStream` and fails a unary
  call with `HttpClient`, both retryable. **Migration:** code matching
  `Transport(_)` to detect a dropped WebSocket, or
  `Protocol(e)` with `e.code == InternalError` to detect a truncated gRPC
  stream, should match `IncompleteStream { .. }` / `HttpClient(_)`, or ask
  `is_retryable()`. Not a wire change. The retry a retryable drop invites
  reconnects on the same `WebSocketTransport` (audit N18; see **Fixed**).

- **gRPC status codes map to what the caller has to do about them.** Over the
  gRPC transport and the slimrpc binding, `UNAUTHENTICATED` is now
  `ClientError::UnexpectedStatus { status: 401, .. }` and `PERMISSION_DENIED`
  is `UnexpectedStatus { status: 403, .. }`, the shapes the JSON-RPC and REST
  bindings already report. They were `Protocol(InvalidParams)` on gRPC and
  `Protocol(InternalError)` on slimrpc, which told a caller with a revoked
  token to fix its request, and meant `BearerAuthInterceptor` never dropped a
  token a gRPC agent had refused: the next call sent it again. A peer's
  `CANCELLED` is now a non-retryable `Protocol` error (`InternalError`, or
  the A2A code its `ErrorInfo` names) instead of a retryable `Timeout`, so a
  call the peer abandoned is no longer re-sent. **Migration:** code matching
  `Protocol(e)` with `e.code == InvalidParams` to detect an authentication
  failure over gRPC should match `UnexpectedStatus { status: 401 | 403, .. }`.
  A peer's cancellation now reports `is_retryable() == false`, so a retry
  policy stops re-sending it with no change to the caller's code. Neither is
  a wire change; both change the client's reading of one.

- **`ClientBuilder::from_card` refuses a card with no interface for A2A
  protocol major 1.** Such a card used to be accepted with a warning and then
  fail at the first call with whatever the wire produced — a v0.3 endpoint
  answers `-32601 method not found`. The error now lists what the card offers.
  Versions are read leniently: empty, `v1.0` and `1-preview` all count as
  major 1.

- **Graceful shutdown ends in-flight tasks before it drains connections.**
  `Server::serve_with_shutdown` used to drain first, and an open SSE stream is
  a connection that does not close until its task ends: the drain waited out
  its 15 s, `RequestHandler::shutdown` then cancelled tokens without waiting
  for any executor, and a delegating executor's downstream tasks outlived the
  process. Reproduced with a coordinator delegating to two a2a-go workers:
  exit after 16 s, `abandoned: 1`, no terminal event upstream, no `CancelTask`
  downstream. The order is now: stop accepting; let tasks finish on their own
  for up to `ServeConfig::completion_grace` (5 s); cancel the rest and give
  their executors `task_grace` (10 s) to cancel what they delegated and write a
  terminal event; then drain. With the completion window at zero, the same run
  exits in 0.02 s with both Go workers cancelled and the upstream stream
  ending `TASK_STATE_CANCELED`; with the default window it first waits up to
  5 s for the delegation to finish on its own (not re-measured). An
  executor that returns without a terminal state after shutdown has its
  `cancel` hook run, as `CancelTask` would. **The trade:** a task that outlives
  the completion window is now cancelled where it used to get the whole drain
  window; the three phases are bounded by 30 s together, a Kubernetes
  `terminationGracePeriodSeconds` default.

- **Every shipped `TaskStore` refuses to move a terminal task to a different
  state**, atomically with the write, answering `UnsupportedOperation` with a
  `TerminalStateConflict` in its data. Code that "reopens" a finished task
  through a shipped store now gets that error. See **Fixed** for why.

### Behaviour Changes

Changes in what a caller observes for the same input, with no change to any
type or signature — the entries a careful adopter has to read at every bump,
and which `cargo semver-checks` cannot see. Listed here as well as under
their own heading. An adopter asked for this section after 0.12.1's change to
what a new task on an existing context inherits (#130) reached them filed
under **Fixed**; that entry did say it changed the wire shape, but not where
someone scanning for such changes would look.

- **HTTP+JSON operation responses are `Content-Type: application/a2a+json`**
  (server; audit N38). Spec §11.1 says the A2A media type SHOULD be used
  for requests and responses; `RestDispatcher` and the axum adapter sent
  `application/json`. Successes and errors both change; the adapter's
  successes also gain the `A2A-Version` header its errors and
  `RestDispatcher` already sent. The agent card's well-known URL,
  `/health` and `/ready` stay `application/json`, and JSON-RPC stays
  `application/json` (§9). Both media types are still accepted on
  requests. **Migration:** a client that required `application/json` on
  an HTTP+JSON response accepts `application/a2a+json`; this SDK's client
  does not check it.
- **The axum adapter's error bodies are AIP-193, as `RestDispatcher`'s are**
  (server, `axum` feature; audit N37). It answered `{"error": "<text>"}`; it
  now answers `{"error": {"code", "status", "message", "details"}}` through
  the REST dispatcher's own builders. A client that read `error` as a string
  from the adapter reads `error.message`. REST's own route and body errors
  gain the `status` name they lacked.
- **A refused credential answers `401`/`403`, not `400`** (server; audit N36,
  ADR 0014). The auth interceptors answered every refusal as
  `InvalidRequest`: HTTP `400` on both HTTP bindings, gRPC
  `INVALID_ARGUMENT`. Now a missing or wrong credential answers `401` with a
  `WWW-Authenticate` challenge (`Bearer realm="a2a"`, or
  `ApiKey header="x-api-key"`) and gRPC `UNAUTHENTICATED`; the AIP-193 body's
  `status` says `UNAUTHENTICATED`. JSON-RPC answers the same `401` with the
  body unchanged (`-32600`); batches stay `200`. WebSocket is unchanged (the
  body alone). **Migration:** a client or alert that matched `400` for an
  auth failure matches `401`/`403`.
- **`SendMessage` refuses a part whose `mediaType` the agent card does not
  declare, with `ContentTypeNotSupportedError`** (server; audit N34). When
  the card declares input modes — `defaultInputModes` or any skill's
  `inputModes` — a part carrying an explicit `mediaType` outside them is
  refused (-32005 over JSON-RPC, HTTP 400 over HTTP+JSON), as spec §3.1.1
  requires; it used to reach the executor. Parts without a `mediaType` are
  not checked, and nothing is enforced without a card or when the card
  declares no modes. **Migration:** declare every media type the agent
  accepts on its card, or call
  `RequestHandlerBuilder::allow_undeclared_input_modes()` to keep the old
  behaviour while the card is corrected.
- **A continuation sent the moment a task reaches `input-required` is
  admitted** (server; audit N21). It used to be refused as "already being
  processed" whenever the executor that parked the task had not yet returned
  — see **Fixed**.
- **A WebSocket stream the caller reads more slowly than the agent writes
  ends with a `stream_lagged` error once 64 frames are waiting** (client;
  audit N29). It used to be held back without loss, which stalled every
  other call on the socket — see **Fixed**. `ClientError::is_stream_lagged`
  identifies it; resubscribe to continue.
- **A call on a `WebSocketTransport` whose socket has dropped reconnects**
  (client; audit N18) instead of failing with a non-retryable `Transport`
  error. Code that matched that error to detect a dead transport now sees
  the call succeed, or fail retryably if the reconnect does.
- **A WebSocket request whose connection ends is cancelled** (server;
  audit N30). It used to run on after its peer had gone, which is how a
  peer that stopped reading held its connection open; a request on the HTTP
  bindings is dropped the same way when its client disconnects.
- **An idle `SubscribeToTask` stream over SSE now ends at
  `subscribe_max_idle`** (server; audit N25). It used to stay open for as
  long as its client did — see **Fixed**. A client that relied on the
  stream staying open resubscribes, which §3.5.2 supports.

### Security

- **The published manifests no longer admit dependency versions with RustSec
  advisories** (audit N24). A consumer's cargo keeps whatever version its own
  lockfile holds as long as our requirement admits it, so a fix that only
  moved this repository's lockfile never reached them — an adopter on 0.12.1
  found `cargo update -p a2a-protocol-client` leaving rustls 0.23.43 in place,
  inside RUSTSEC-2026-0285. Measured against the RustSec database on
  2026-09-24, the requirements admitted affected releases under seven
  advisories: `rustls` (RUSTSEC-2024-0336, RUSTSEC-2024-0399,
  RUSTSEC-2026-0285), `ring` (RUSTSEC-2025-0009), `bytes`
  (RUSTSEC-2026-0007), `sqlx` (RUSTSEC-2024-0363), `time`
  (RUSTSEC-2026-0009) and `tokio` (RUSTSEC-2025-0023, unsound). The floors
  are now `rustls >=0.23.45`, `ring 0.17.12`, `bytes 1.11.1`, `sqlx 0.8.1`,
  `time 0.3.47` and `tokio >=1.44.2`; each declares a `rust-version` at or
  below 1.88, and both lockfiles already held newer versions, so neither
  changed. **Effect on consumers:** upgrading to this release moves any of
  those dependencies still below its new floor. A new CI gate,
  `scripts/check_advisory_floors.py`, fails when a normal or build
  dependency of a published crate admits an affected, published version;
  `cargo deny`, which reads our lockfile, could not see this and was green
  throughout.

### Added

- **`ServerInterceptor::on_complete` and `CallOutcome`.** `after` runs only
  on success and its error replaces the response, so no hook saw a failed
  or abandoned call: an interceptor that acquired something in `before` —
  an in-flight gauge, a concurrency slot, an open audit record — had no
  place to release it. `on_complete` is called once per call, in reverse
  order, on every interceptor whose `before` ran (including the one that
  refused the call), with `Succeeded`, `Failed(&ServerError)` — the error
  the caller is sent — or `Cancelled`, when the call's future is dropped
  unanswered. It returns `()`, so it cannot change the response. The
  default does nothing, so existing interceptors compile and behave as
  before. Every `RequestHandler` method, on every binding, runs through it;
  calls made with `ServerInterceptorChain::run_before` and `run_after`
  directly do not. A new test drops a real JSON-RPC and HTTP+JSON
  connection mid-call and sees `Cancelled` in `on_complete` and `cancelled`
  in the `rpc.server.call` record — the first test of the premise that
  N26, N27 and N28 rest on: hyper drops a request's future when its client
  disconnects.

- **`Task::new`, `TaskQueryParams::new` (with `with_history_length` and
  `with_tenant`), and `a2a_protocol_server::CancellationToken`** (audit
  N16). A custom store or test built a `Task`, and every `GetTask` caller a
  `TaskQueryParams`, with a full struct literal that breaks when a field is
  added; and an executor that stored its `RequestContext::cancellation_token`
  needed its own `tokio-util` dependency at a matching version to name the
  type. The re-export reaches the SDK as `a2a_protocol_sdk::server::CancellationToken`.

- **`testing::ScriptedPeer`** (client feature `testing`): a loopback A2A
  peer that stalls, cuts off, mis-frames or refuses every call on purpose,
  over JSON-RPC, HTTP+JSON, WebSocket and gRPC, for testing code that calls
  agents — a coordinator has to survive the worker it delegates to, and a
  well-behaved test server never produces these failures. This crate's own
  tests drive every binding against every script with it
  (`tests/scripted_peer_tests.rs`), which is how N13 above was found. Adds no
  dependency; not for production builds.

- **`GrpcDispatcher::serve_with_shutdown` and
  `WebSocketDispatcher::serve_with_shutdown`**, with `with_completion_grace`,
  `with_task_grace` and `with_drain_timeout` on both dispatchers (defaults
  `DEFAULT_COMPLETION_GRACE`, `DEFAULT_TASK_GRACE`, `DEFAULT_DRAIN_TIMEOUT`).
  Both take a bound listener and a signal and return the same `ServeReport` as
  `Server::serve_with_shutdown`, in the same order: stop accepting, end the
  tasks in flight, then drain. Until now neither dispatcher took a signal, so
  a gRPC or WebSocket deployment had to assemble the sequence by hand or
  orphan its delegations on shutdown (audit S8). Two differences follow from
  the transports: gRPC connections are sent `GOAWAY` as soon as accepting
  stops, so they take no new calls during the completion window; and each
  WebSocket connection, once the tasks have ended, finishes the requests it
  has read and is closed with a Close frame. The gRPC server's own error is
  returned rather than discarded.

- **Release gates for what a tag publishes** (`scripts/check_release_tree.py`,
  four steps in `release.yml`). A release now fails if the tagged tree has
  entries under `[Unreleased]`, if any packaged file changed after the
  release's own notes were last edited, if a patch release or a second
  breaking minor in one calendar month carries `### Breaking Changes`, or if a
  packaged `.crate` was built from another commit or a dirty tree. `v0.13.0`
  would have failed three of them (audit N7, N10). The cost is ordering:
  release notes must be the last edit before the tag, and the next breaking
  release cannot be tagged before October 2026. Not a change to any crate.

- **The mutation gate grades function bodies it never could.**
  `scripts/install_cargo_mutants.sh` installs a pinned cargo-mutants 27.1.0,
  patched so that a function returning a `Result` alias (`A2aResult<T>`,
  `ClientResult<T>`) or a boxed future (`Pin<Box<dyn Future<Output = T>>>`,
  every object-safe async trait method here) gets a "replace the body" mutant
  that compiles. Stock 27.1.0 generated none that did for 346 functions, so
  whether their effects were tested at all had never been graded (audit N4,
  N9, N11). The survivors the patched build found are killed by tests on
  this branch. The install verifies the crate's checksum, applies the patch
  exactly, and refuses a binary without it.

- **A Go SDK interop gate in CI** (`go-sdk-interop`, `scripts/go_sdk_interop.sh`).
  The official Go SDK's client against this server, and this client against an
  a2a-go server, over JSON-RPC, HTTP+JSON and gRPC. Before it, a2a-go ran in CI
  only as a server driven by the in-repo TCK, which does not use
  `a2a-protocol-client`. Against the unfixed tree it was red for five of the
  defects fixed below (S2, S7, T1, C8 and C9 in the 2026-09-22 audit's
  numbering); its C10 check could not run there, because the T1 failure
  stopped card resolution first. `scripts/prove_gates_fail.sh` proves it fails when
  the push token stops reaching a Go webhook.

- **`RequestHandler::finish_in_flight` and `RequestHandler::cancel_in_flight`**,
  returning `InFlightReport`; `ServeConfig::completion_grace` and `task_grace`
  (`DEFAULT_COMPLETION_GRACE`, `DEFAULT_TASK_GRACE`); `ServeReport::tasks`; and
  `Dispatcher::request_handler`, defaulted to `None` and overridden by the
  JSON-RPC and REST dispatchers. Call `finish_in_flight` yourself when
  something else owns the sockets — `examples/deploy-agent` does, behind Axum.

- **`impl From<ClientError> for A2aError`**, so an executor that delegates can
  use `?`. A downstream `Protocol` error passes through unchanged; exactly the
  errors `is_retryable()` accepts fail the task as `Transient`, everything
  else as `Internal`, with the client's text and source chain kept. The server
  now honours a class recorded with the new **`failure::set_error_class`** /
  **`failure::error_class`**, because no error code can express `Transient`.

- **Client stream liveness bounds:** `ClientConfig::stream_idle_timeout` and
  `stream_first_event_timeout` (both 5 minutes by default, with builder
  setters), applied by `A2aClient` to every stream on every binding, and TCP
  keepalive (60 s idle, 15 s interval, 4 probes) on every client connection. A
  stream whose server sent one event and then went silent while holding the
  connection open used to hang `next()` forever. Any bytes reset the idle
  bound, SSE keep-alive comments included; on gRPC and WebSocket, which carry
  no heartbeat the client can see, it bounds the gap between events.

- **Resuming a broken stream:** `ClientError::IncompleteStream { last_event_id,
  detail }`, `EventStream::last_event_id()` and
  `A2aClient::subscribe_to_task_from(id, last_event_id)`, which sends
  `Last-Event-ID`. Verified against this repository's JSON-RPC and REST
  dispatchers. gRPC and WebSocket streams carry no ids.

- **`ClientConfig::max_event_size`** (16 MiB) and `EventStream::with_max_event_size`.

- **`CallInterceptor::on_error`** and **`TokenProvider::invalidate`**, both
  no-op by default. `BearerAuthInterceptor` uses them so an OAuth2 token the
  agent answered with `401` is not sent again; the refused call still fails.
  On every binding: gRPC's `UNAUTHENTICATED` and a WebSocket handshake refused
  with 401 reach it as `UnexpectedStatus { status: 401, .. }` since the two
  entries under **Breaking Changes**.

- **`OAuth2ClientCredentials::with_failure_backoff`** (1 s), **`Method::returns_empty`**,
  and **`push::webhook::notification_token`**, which reads the push token under
  either header name.

- **`TaskStore::save_appending_history`**, which persists a task whose history
  grew by the messages a turn added, without the caller shipping the history
  it has already stored. Additive: the default reads the record, appends,
  trims and writes it back through `save`, which is exactly what the send path
  did before, so a store that does not override it behaves as it always has.
  `InMemoryTaskStore` overrides it to extend a `Vec` in place.

  `task.history` is **ignored** by this method — the appended messages come
  from their own parameter. That is deliberate: reusing the field for the tail
  would put the caller back in possession of the whole conversation, which is
  the cost being removed. `max_history` is a parameter rather than a constant
  in the store, because retention is the handler's policy and a store
  inventing its own cap would silently disagree with the rest of the server.

  Measured on one box, 1,400 sequential posts to one channel: 3,962µs →
  2,748µs per send at the 1,024-message history cap, and growth across the run
  3.2x → 2.1x. It does **not** make a send O(1);
  `docs/swarm-scale-findings.md` has the per-stage attribution showing what
  still dominates.

- **Spans for every inbound call, on every binding** (ADR 0013; audit O1, O2,
  O4). With the `tracing` feature — on by default — each call runs in one
  `SERVER` span named `lf.a2a.v1.A2AService/{Method}`, with the semantic
  conventions' `rpc.system.name`, `rpc.method`, and on failure
  `rpc.status_code` and `error.type`; HTTP+JSON spans add `http.request.method`
  and `http.route`. The executor, event processor, push delivery and SSE
  writer run in child spans, the executor's carrying `a2a.task.id` and
  `a2a.context.id`. With `otel` and a `tracing-opentelemetry` layer, an
  inbound `traceparent` is the span's remote parent and the `traceparent` sent
  downstream names that recorded span instead of an id nothing records.
  `InboundTracePolicy::Drop` records no span for the call or anything it
  spawns. Gate: `crates/a2a-protocol-sdk/tests/observability_e2e/`, which
  failed on `main`.
- **`rpc.server.call.duration`** in `OtelMetrics`, and **`Metrics::on_rpc_call`**
  with **`RpcCall`** for any other exporter: every call's duration with the
  conventions' buckets (`otel::RPC_DURATION_BUCKETS`), method, and the status
  its binding answered with — including calls the handler never sees (a body
  that is not JSON-RPC, an unknown method, an HTTP+JSON request refused on its
  parameters) and calls whose peer went away (`error.type` `cancelled`)
  (audit O5, O11). Additive: the callback defaults to a no-op.
- **Connection statistics are reported.** `serve`, `serve_with_addr` and
  `Server::serve_with_shutdown` now call `Metrics::on_connection_pool_stats`,
  which nothing called, so the four `a2a.server.pool.*` instruments the book
  catalogues were never emitted (audit O10). The gRPC and WebSocket
  dispatchers' own listeners do not report them yet.

### Changed

- **A client stream that ends before its final event yields
  `ClientError::IncompleteStream` before `None`.** A final event is a
  `Message`, or a task in a terminal or interrupted state (§3.1.2, §11.7).
  Before, a stream cut off mid-task was indistinguishable from one that
  finished, and a half-written last frame was dropped silently.

- **`stream_connect_timeout` bounds only establishing a stream**; the first
  event has its own 5-minute bound. The 30 s connect timeout used to bound the
  first event too, which cut off agents — a2a-go among them — that send headers
  and then think. If you shortened the connect timeout to fail fast, also set
  `with_stream_first_event_timeout`.

- **The JSON-RPC client reads a streaming call's pre-stream error in either
  shape**: the plain JSON-RPC error body this server sends, or one SSE frame
  carrying it, which is what a2a-go's server sends. Both become the same
  `ClientError::Protocol`. The server's own shape is unchanged: a plain JSON
  200, which the official a2a-tck requires (STREAM-SUB-003/004). a2a-go
  v2.5.0's client reads streaming answers only as SSE, so it still loses
  these errors. That is a2a-go's to fix, and `go-sdk-interop` pins it.

- **Wire: an HTTP+JSON stream's error frame is a `google.rpc.Status`**
  (`{"error":{code,status,message,details}}`), the error's `data` a flat
  `google.protobuf.Struct` detail. a2a-go's REST client could not decode the
  old bare `{code,message}` frame. This client decodes all three shapes.

- **Push notifications carry the token under both `X-A2A-Notification-Token`
  (Python SDK) and `A2A-Notification-Token` (a2a-go)**, the same value in each.
  A Go webhook read no token before. This reverses 0.8's removal of the
  unprefixed name, which is a2a-go's rather than this SDK's invention. Push
  logs now show only `scheme://host[:port]`: webhook URLs often embed secrets.

- **`StringList` reads a2a-go's bare-array scopes and `null`** as well as the
  spec's `{"list":[...]}`, which is still the only form written.

- **A streaming client's terminal frame waits for the store's ruling** (at most
  the queue's write timeout, 5 s) and carries the stored state. See **Fixed**.

- **`save_status_delta` is now overridden by `SqliteTaskStore` and
  `PostgresTaskStore`**, not only by `InMemoryTaskStore`. Until now a SQL
  deployment got nothing from it — no regression, since the trait's default
  delegates to `save`, but no win either, and neither the trait docs nor the
  findings report said so. Measured on one box, 100 status events on a channel
  holding 200 messages, medians of five runs:

  | store | `save` | `save_status_delta` |
  |---|---|---|
  | `InMemoryTaskStore` | 8,499µs | 639µs |
  | `SqliteTaskStore` | 122,071µs | 37,216µs |
  | `PostgresTaskStore` | 287,287µs | 120,104µs |

  Both overrides update the `state` column that `list` filters on and the
  `updated_at` that §3.1.4 orders by, either of which is wrong in a way no
  round-trip through `get` would reveal. The SQLite override deliberately does
  *not* delete the artifact journal that its `save` deletes: `save` has just
  rewritten the document with every part in it, a status delta has not, so
  those rows are still the only record of appended parts.

- **`a2a.server.latency` uses the semantic conventions' buckets** (5 ms to
  10 s) and is deprecated in favour of `rpc.server.call.duration`, to be
  removed no earlier than the next minor release. It kept the SDK's default
  boundaries, sized for milliseconds, so every call under 5 s landed in one
  bucket (audit O5). Dashboards that read its `le` labels see new ones.

### Fixed

- **A continuation answering `input-required` at once is admitted** (server;
  audit N21). A blocking send returns when the task reaches an interrupted
  state, and a stream delivers that state, before the executor that wrote
  it has returned; admission refused any send while that executor's token
  was live, with an error telling the caller to wait for `input-required` —
  the state it had just been sent. The executor's writer now marks the turn
  *parked* when the latest state it writes is `input-required` or
  `auth-required`, and admission waits up to
  `HandlerLimits::executor_drain_timeout` (default 5 s) for a parked turn's
  executor to finish. A send into a task still working is refused at once,
  as before, and one still beside a live executor past the bound is refused
  exactly as before, so two executors never run for one task.
- **An idle `SubscribeToTask` stream over SSE ends at `subscribe_max_idle`**
  (server; audit N25). The bound was measured inside a future that each SSE
  keep-alive discarded, so with the defaults — keep-alive 30 s, bound 300 s
  — a subscription to a task parked at `input-required` never ended and
  polled the store for as long as its client stayed. `InMemoryQueueReader::read`
  is now cancel-safe across the reattach wait.
- **A send whose client goes away mid-commit no longer wedges its task**
  (server; audit N26). A request dropped between claiming its idempotency
  key and spawning its executor — a client timing out during a slow store
  write — kept the task's queue lease and cancellation token for the life
  of the process, so every later continuation was refused as "already being
  processed", and a retry carrying the same idempotency key waited on a
  task that did not exist. What the send took is now released when its
  future is dropped, and only what it took: a continuation dropped while it
  waits for the previous turn leaves that turn's executor cancelable.
- **A blocking send whose client goes away still records the task's
  outcome** (server; audit N27). The collector that persists a blocking
  send's events ran in the request's future, so when a client timed out on a
  slow agent the executor finished into nothing: the task stayed `working`
  in the store, and no later push notification was sent. The collection now
  runs on its own task, which shutdown waits for, and the request awaits it.
- **A failing interceptor `after` hook no longer orphans a send's task**
  (server; audit N28). `after` ran between spawning the executor and
  attaching what persists its events, so its error left a task the agent
  went on to complete at `submitted` in the store. It now runs once the
  response exists. `ServerInterceptor::after`'s documentation said it runs
  even when the handler fails; no method ever did that, and it now says so.
- **`WebSocketTransport` reconnects after its connection drops** (client;
  audit N18). Once its socket dropped, every later call failed at once with
  a non-retryable `Transport("WebSocket connection closed")`, so a
  long-lived client had to be rebuilt by hand after any server restart. The
  next call now reconnects to the same endpoint with the same configuration,
  bounded by `connect_timeout`, one reconnect for concurrent callers; a call
  that races the old connection's close fails as retryable. Streams already
  open keep the connection they were opened on.
- **An unread WebSocket stream no longer stalls other calls on the same
  socket** (client; audit N29). The transport's single reader waited for
  room in a stream's 64-frame buffer before reading anything else, so a
  `GetTask` made before reading an open stream timed out once the agent had
  sent more than 64 events. The reader no longer waits: a stream whose
  buffer overflows ends with a `stream_lagged` error, as a lagging reader
  does on the server.
- **A WebSocket peer that stops reading is closed by the idle timeout**
  (server; audit N30). With `with_idle_timeout` set, a peer that stopped
  reading while being streamed to was never closed: the stream's send held
  the sink's lock, the keepalive waited for that lock, and so did the
  closing handshake, so the connection and its `max_connections` slot were
  held for as long as the peer liked. The keepalive no longer waits, the
  connection's requests are cancelled when the peer is gone, and the close
  handshake is bounded at 1 s.
- **A gRPC stream the client cancels releases its subscription at once**
  (server; audit N31), rather than at the task's next event.
- **The agent card's poll watcher retries a card that failed to parse**
  (server; audit N32). It recorded the file's mtime even when the reload
  failed, so on a filesystem with coarse timestamps a fix written in the
  same second as a half-written file was never loaded.
- **Every task status carries a timestamp** (server; audit N35).
  `EventEmitter::status` — the helper executors are pointed to — built its
  status with `TaskStatus::new`, which leaves `timestamp` empty, so agents
  built on it served statuses with no time: spec §5.6.1 wants ISO 8601 UTC
  timestamps, `ListTasks` is ordered by them (§3.1.4), and ACTS
  DM-SERIAL-001 (a MUST) failed. The event queue now stamps any status
  update or task snapshot the executor left without one, before it is
  persisted or streamed; a timestamp the executor set is kept. The stamp
  counts toward `max_event_size`, so a status event within a few dozen
  bytes of that limit may now be refused.
- **The two HTTP+JSON dispatchers answer the same failure identically**
  (server; audit N37) — see **Behaviour Changes**. Found while fixing N36.
- **HTTP+JSON responses carry the A2A media type** (server; audit N38) —
  see **Behaviour Changes**. The response header cited a §11.1 from the
  2026-03-31 specification snapshot, which said `application/json`;
  upstream changed that line, and the 2026-08-30 refresh did not reach the
  code. Found by the ACTS conformance suite (REST-CT-001, a SHOULD).
- **This SDK's client recovers from a revoked or rotated token against this
  SDK's server** (server; audit N36). The client drops a cached token on
  `401`; the server never sent one, so the refused token was re-sent on every
  call until the provider's cache expired, on every binding. See **Behaviour
  Changes**. New in `a2a-protocol-types`: `A2aError::unauthenticated`,
  `A2aError::permission_denied`, `A2aError::auth_rejection` and
  `AuthRejection`/`AuthRejectionKind`, for custom interceptors to get the
  same statuses.
- **`SendMessage` checks message parts' media types against the agent
  card** (server; audit N34) — see **Behaviour Changes** for what is
  refused and how to opt out. Found by the ACTS conformance suite
  (CORE-SEND-004, a MUST); the official Rust SDK has the same gap open as
  a2aproject/a2a-rs#270.
- **JSON that is not a JSON-RPC Request object is answered Invalid Request
  (-32600), not Parse error (-32700)** (server, JSON-RPC and WebSocket; audit
  N33). A body with no `method`, a bare value such as `1`, an empty batch
  `[]` and a batch item that is not a request were all answered -32700,
  which JSON-RPC 2.0 reserves for a body that is not JSON; its own examples
  answer these -32600. Found by the ACTS conformance suite (CORE-ERR-006, a
  MUST). A client that matched on -32700 for these cases now sees -32600.

- **The agent card's poll watcher sees a change made just after it starts**
  (audit N23). It read the file's baseline mtime inside its own task, on
  the blocking pool, so a card rewritten before that read landed became the
  baseline and was never loaded. The baseline is now read when
  `spawn_poll_watcher` is called.
- **A WebSocket stream outlives the client that opened it**, as streams on
  the HTTP and gRPC bindings do (audit N22). Dropping the client, or the
  `WebSocketTransport`, aborted the connection's reader while the stream
  still held its own sender, so the stream went silent: no later event, and
  no end until the idle bound (five minutes by default) reported a
  `Timeout`. The stream now holds the connection open until it is dropped.
- **Wire: `RestDispatcher` answers an overload with `503` and an oversized
  body with `413`**, as the axum adapter always has, instead of `500` and
  `400`. The two HTTP+JSON dispatchers had separate mappings, so the same
  overload told a client of one to retry and a client of the other not to
  (audit N20). One mapping now serves both.
- **The pool counters count each connection once.** `OtelMetrics` added the
  cumulative totals in each report to its counters, so every report
  re-counted every earlier connection (audit O10).
- **The WebSocket dispatcher's documentation** said it routes the v0.3
  `method/verb` aliases; it routes only `message/stream` of them and refuses
  the rest with `MethodNotFound`, as its own test asserts (audit N19).

- **The book's Rust examples compile.** 127 of its 130 `ignore`d blocks now
  compile (the other 3 are `slimrpc`, outside the workspace), and 79 of those
  also run; the other 48 would call a network or a database. Most needed only
  hidden `# ` lines supplying what the page established in prose. Five could not have compiled as shown — each confirmed by
  compiling the original with only its missing context added: a client
  moved into two calls (E0382), a `match` on the `#[non_exhaustive]`
  `SendMessageResponse` with no wildcard arm (E0004), an
  `Arc<RateLimitInterceptor>` passed where a `ServerInterceptor` is required
  (E0277), `&*writer` on a queue writer with no `Deref` (E0614), and
  `ClientBuilder::new("…".into())` (E0283). Others named what does not exist
  (`send_streaming_message()`, `AuthInterceptor::new`, `RetryInterceptor`)
  or needed a feature the page did not mention (`hyper-util`'s
  `server-auto`, `tracing-subscriber`'s `json`). And compiling them contradicted the prose around them:
  - `AgentExecutor::cancel`'s default was documented as refusing with
    `TaskNotCancelable`; it has cancelled since 0.7.
  - The builder was said to choose the transport from the URL; without
    `with_protocol_binding` it is always JSON-RPC.
  - CORS read as on by default; it is off until `with_cors`.
  - "`sqlx::PgConnection` is not `Send + Sync`" — it is. A single connection
    is still the wrong choice, because every call then waits on its lock.
  - The body-size pitfall's fix checked only `size_hint()`, which a chunked
    body leaves unbounded; the page now adds `http_body_util::Limited`, as
    the REST dispatcher does.
  - The custom authentication interceptor compared tokens with a `HashSet`
    lookup and did not mark itself `authenticates()`; the page now points
    fixed token lists at the constant-time built-ins, and its example
    records the caller and sets the marker.
  - The production page's health-check example could not type-check; the
    REST dispatcher and `A2aRouter` already answer `/health` and `/ready`,
    and the JSON-RPC dispatcher does not, which the page now says.

  `.book-ignore-baseline` now holds only the three `slimrpc` blocks.
  Documentation only; no crate changed.

- **The configuration reference's defaults are the defaults.**
  `book/src/reference/configuration.md` gave `with_executor_timeout` a
  default of None; the builder sets one hour, so a task can be failed by a
  bound the page said was not there (audit S12). It also had no row for
  `TaskStoreConfig::max_events_per_task` (512) or `idempotency_key_ttl` (24
  hours), or for `require_version_header` (on) in `DispatchConfig` and
  `GrpcConfig`. `crates/a2a-protocol-sdk/tests/book_defaults.rs` now reads
  each defaults table on the page and compares it with the struct's real
  `Default`, in both directions: a documented value that differs, a row that
  names no field, and a field with no row all fail. Against the page as it
  was, it reports exactly those five. Documentation only.

- **The four crate READMEs — each crate's crates.io page — compile.** Seven
  of their eight Rust blocks did not: the client's documented
  `resubscribe()`, `get_authenticated_extended_card()` and
  `ClientBuilder::with_transport()`, none of which exist; a `Message` literal
  and `match`es that the types' `#[non_exhaustive]` rejects; a server prelude
  that is the SDK's. Each README is now a doctest of its crate
  (`#[cfg(doctest)] #[doc = include_str!("../README.md")]`), and its feature
  table — like the two in the book and the one in `crates/README.md` — is
  checked against the manifest by `scripts/check_feature_tables.py`, which
  found 17 missing or wrong rows. The book and root README no longer say
  `default-features = false` removes rustls from the SDK; it does not yet
  (audit K1).

- **Agent-card signing canonicalizes numbers the way RFC 8785 says.** Three
  deviations, each found by the RFC's own vectors, now in
  `crates/a2a-protocol-types/tests/rfc8785_vectors.rs`: a double exactly
  halfway between two shortest renderings took the upper one, not the even
  one (`1424953923781206.2`, Appendix B note 4); a card parsed from text
  could read a float one ULP off, because serde_json's default parser is not
  correctly rounded (5 of Appendix B's 24 values); and an integer beyond
  2^53 was written with all its digits rather than as the double JCS
  defines every number to be (`9007199254740993` is `9007199254740992`).
  Against V8's `JSON.stringify` over the same 1,000,077 random doubles, the
  canonicalizer disagreed 1,474 times from the bits and 237,518 times from
  the text; it now disagrees 0 times on either, and 0 times over 8,552,446
  structured ones (every `m × 2^e` with odd `m` below 4096), which is where
  the fix's own first version was caught wrong at 2^-24. **What it trades:** a
  signature made by 0.13.0 or earlier over a card holding an affected number
  does not verify here, and the reverse — though neither verified against
  any other conforming implementation, which is the correction (a
  specification fix, `STABILITY.md` §2). The `signing` feature now enables
  serde_json's `float_roundtrip`, which makes float parsing correctly
  rounded, and slower, in every crate of a build that enables `signing`.

- **`Server::serve_with_shutdown` sees its signal at the connection
  ceiling.** It waited for a connection permit before it looked at the
  signal, so with `ServeConfig::max_connections` set and every slot held by a
  stream that only shutdown could end, shutdown never began: the test that
  pins it timed out after 20 s with the only slot held. The signal is now
  watched while waiting for the permit as well as for the peer. The new gRPC
  and WebSocket shutdown paths are built the same way and tested for it.

- **An unreachable OAuth2 token endpoint is a transient failure.** A refused
  or dropped connection to the token endpoint, or to the OIDC discovery
  document, surfaced as `ClientError::Transport`, which is not retryable, so
  through `From<ClientError> for A2aError` a task whose token fetch met a
  restarting identity provider failed as `Internal` rather than `Transient`.
  It is now `ClientError::HttpClient`, as the transports report the same
  error. A token endpoint that answers 429 or 5xx is still `Transport`; that
  is recorded as open (audit N2).

- **Replicas starting together against an empty PostgreSQL database all come
  up.** `CREATE TABLE IF NOT EXISTS` races in PostgreSQL: two sessions that
  both find a table absent both create it, and the second fails with a
  duplicate key in `pg_type` or `pg_class`. Every schema-creating constructor
  — `PostgresTaskStore::{new, from_pool, with_migrations}`, the tenant-aware
  task store, both push-config stores and `PostgresRateLimitCounter` — ran its
  DDL unlocked (`with_migrations` included, because it creates
  `schema_versions` before it can lock it), so a multi-replica deployment's
  first start could crash a replica. Measured on 16.13 with 48 constructors
  against a fresh database: 2 failed on the first attempt; 0 of 48 in each of
  20 consecutive runs after. Each constructor's DDL now runs in one transaction
  holding a transaction-scoped advisory lock, the same key for every store, so
  they serialize against each other and against the migration runner. **The
  trade:** schema creation is serialized across the whole database — a
  cold start of N replicas does its DDL one replica at a time — and the key,
  the bytes of `"a2a_schm"`, is one an application's own advisory locks on
  the same database should avoid.

- **A task another replica canceled no longer ends `Completed`.**
  `CancelTask` on replica B wrote and answered `Canceled`, and replica A's
  executor, which never heard of it, then wrote `Completed` over it: every
  store applied status writes unconditionally. `tests/cross_replica_cancel/`
  reproduced it on the in-memory, SQLite and PostgreSQL stores. The refused
  write is now how A learns: it cancels its executor's token, stops persisting
  and pushing its events, answers a blocking caller with the stored task, and
  pushes the stored terminal status once. **The trade:** A learns at its
  executor's next write, not immediately, and non-terminal frames emitted in
  between still reach a client streaming from A. Custom `TaskStore`s are not
  covered unless they apply `store::refuses_write` inside their writes.

- **Agent cards from a2a-go with `securityRequirements` parse.** The reverse
  direction is a2a-go's to fix: v2.5.0 rejects the spec's shape. The interop
  gate pins that rejection and fails when it stops holding.

- **Deleting a push config on an a2a-go agent reports success.** a2a-go answers
  JSON-RPC with no `result` and HTTP+JSON with an empty 200; both are now
  accepted, for Empty-result methods only.

- **Interface selection skips other protocol majors**, so an a2a-go agent's
  v0.3 endpoint listed first is no longer chosen; binding names match
  case-insensitively in `build()` as in selection; and `build()` falls back to
  the card's next interface when the chosen one cannot be built.

- **HTTP+JSON streaming errors decode to the A2A error they carry**, from a
  non-2xx answer and from inside a 200 stream, where both used to surface as
  `UnexpectedStatus` or `Serialization("unknown variant …")`. The stream-lag
  signal survives the new server frame: `A2aError::is_stream_lagged` is true
  across a REST stream from this server.

- **OAuth2 refresh failures are shared, not serialised.** Every caller waiting
  on a refresh gets its outcome, failure included, from one request. Five
  callers with 1 s timeouts used to fail at 1, 2, 3, 4 and 5 s. A caller
  cancelled mid-refresh hands the attempt to the next waiter.

- **Short-lived OAuth2 tokens are cached for at least half their lifetime.** A
  token living 30 s or less was refetched on every call.

- **The SSE parser refuses a line that outgrows `max_event_size`** with
  `EventTooLarge` instead of silently truncating it past twice the limit.

- **`RequestHandler::shutdown` counts the live queues it destroys.**
  `ShutdownReport::queues_force_destroyed` was a hard-coded 0, so a shutdown
  that cut live streams reported itself graceful.

- **A message naming a live task in its own context is no longer refused.**
  `resolve_task_id` accepted only the single task `find_task_by_context`
  returns — the most recently updated non-terminal one — and rejected every
  other, including tasks that were live and in the same context. Since a
  message carrying a `contextId` and no `taskId` starts a *new* task
  (§3.4.3), one participant posting without a `taskId` forked the channel,
  the fork became the context's canonical task, and every other participant
  was locked out of the original permanently with
  `InvalidParams: message task_id does not match task found for context`.

  §3.4.3 permits continuing "a specific task" by `taskId`, and mandates
  rejecting exactly one mismatch: a `contextId` differing from that of the
  referenced task. §3.4.1 says a `contextId` "logically groups multiple `Task`
  objects". The implementation treated a context as holding one. It now
  resolves against the task the message actually names and refuses only a
  genuine cross-context reference.

- **A `taskId` that names no existing task now returns `TaskNotFound`**
  (§3.4.2) rather than `InvalidParams`, whether or not the context happens to
  hold some other task. The handler already returned `TaskNotFound` for that
  input when the context was empty; the only thing deciding between the two
  answers was whether an unrelated task existed nearby.

- **The in-memory event queue's documentation described the wrong lag
  behaviour.** It said a slow SSE consumer "receives `Lagged(n)` and skips
  missed events". The consumer does not resume: `InMemoryQueueReader::read`
  turns the lag into `A2aError::stream_lagged`, which `streaming::sse` writes
  as an `event: error` frame before closing the stream. A lagging tail gets a
  contiguous prefix and an announced end, never a silent hole — the stronger
  of the two behaviours, and the one a reader can recover from. Documentation
  only; no code changed.


### Internal

- **`scripts/check_panic_paths.py` counts `unreachable!`**, which panics as
  `panic!` does and was invisible to the gate. The one in library code — the
  JSON-RPC dispatcher's batch branch, after an `is_array()` test that made
  it redundant — is gone: the branch now matches the array directly.
- **`scripts/preflight.sh` builds with `CARGO_INCREMENTAL=0`, as CI does.**
  `ci.yml` does not set it, but both actions every CI job starts with export
  it (read at their pinned SHAs). Without it a `--full` run kept 16 GB of
  incremental state CI never builds and nearly filled a 38 GB allowance.
  `prove_gates_fail.sh` already turned it off; its comment saying CI did not
  is corrected.

### Correction, 2026-09-24: claims that were not true when published

An audit of every public claim from 0.6.0 onward — the READMEs inside the
published `.crate` files, the crate documentation, the book, the GitHub
release notes and this file — found claims their own artifacts contradict.
Those in the repository are corrected in this release. Those in published
artifacts cannot be changed: crates.io and docs.rs go on serving them for
each version below until the next release replaces the current one. Each
was checked against the published `.crate` for that version.

- **`a2a-protocol-types` README, 0.7.0 to 0.13.0:** "`A2A_VERSION` —
  `"1.0.0"`". The constant is `"1.0"` in every one of those versions.
- **`a2a-protocol-types` README, every version:** "zero `unsafe` blocks
  anywhere in this crate". From 0.12.0 the published crate's `build.rs`
  contains `unsafe { std::env::set_var("PROTOC", …) }`, as do the client's
  and server's; `#![forbid(unsafe_code)]` covers `src/` only. The 0.12.0
  release notes counted four such build scripts in the repository; there
  are five.
- **`a2a-protocol-client` README, every version:** documents
  `resubscribe()`, `get_authenticated_extended_card()` and
  `ClientBuilder::with_transport()`. None exists in any published version;
  the methods are `subscribe_to_task`, `get_extended_agent_card` and
  `with_protocol_binding` / `with_custom_transport`.
- **`a2a-protocol-client` and `a2a-protocol-server` READMEs, every
  version:** the `signing` feature is described as agent card signature
  verification ("signing verification" on the server). In both crates it
  only forwards `a2a-protocol-types/signing`; neither crate signs or
  verifies anything on its own. This is a security-relevant capability
  claim, and it was false.
- **The book's introduction**, as deployed: "zero surviving mutants
  enforced via `cargo-mutants` CI gate" (the gate covers only functions a
  pull request changes; the weekly sweep has recorded survivors), "the
  entire codebase is free of unsafe code", "no panics on any caller input or
  I/O failure" (no gate measures that), "exhaustive pattern matching"
  (protocol enums are `#[non_exhaustive]`), "zero-cost abstractions" (the
  extension points are trait objects and boxed futures), and "all defaults
  … are overridable". Each is rewritten to what its evidence supports.
- **0.7.0 release notes:** "Official-SDK interop is now proven, both
  directions … 20/20 on both JSON-RPC and REST for every one". The 20/20
  for the JavaScript and Java SDKs was scored after skipping checks they
  fail (`list_tasks_basic` and `a2a_media_type_accepted` for JavaScript,
  `a2a_media_type_accepted` for Java); the entry mentions that `--skip`
  exists but not that it was used on them. The reverse direction was the
  Python SDK's client only.
- **0.8.0 release notes:** "MUST compatibility **100%**" and an empty
  conformance baseline. True of what that run graded; four MUSTs have failed
  against the suite's stale v1.0.0 table since A2A v1.0.1, and the figure
  to quote is 88 of 114 passing and 4 failing.
- **0.12.1**, raised by an adopter: the change to what a new task on an
  existing context inherits (#130) was filed under **Fixed**. The entry
  said it changed the wire shape, but not where someone scanning for such
  changes looks; **Behaviour Changes** above exists for that.
- **`CITATION.cff`**: "A complete Rust implementation" — rewritten to what
  is measured.

## [0.13.0] - 2026-09-20

### Correction, 2026-09-23: nineteen entries that shipped in 0.13.0

The `v0.13.0` tag is on `391f0df`, the merge of #138, not on the release
preparation that wrote this section. The crates published as 0.13.0 were
built from that tag (their `.cargo_vcs_info.json` says so), so they carry
#138's changes — but at that commit this file still listed them under
`[Unreleased]`, and until today it went on doing so. Moved here verbatim; the
GitHub release notes for 0.13.0, extracted from this section at the time, do
not include them. Found by `cargo semver-checks` reporting nothing to do for
a tree whose `[Unreleased]` claimed a breaking change (audit N7).

#### Breaking Changes

- **`RetentionPolicy` and `PurgeReport` are `#[non_exhaustive]`.** Construct a
  policy with `RetentionPolicy::new` and the `with_*` setters, and read a
  report's fields rather than destructuring it exhaustively. `STABILITY.md` §4
  lists the configuration structs that carry this marking so a new option is
  additive; `RetentionPolicy` was missed by the 0.12.0 conversion that
  introduced the rule, and the list said otherwise until now. Marking both is
  what makes the two additions below — and the next one — non-breaking.

#### Added

- **`RetentionPolicy::idempotency_key_max_age`** (default 24 hours) and
  **`TaskStoreConfig::idempotency_key_ttl`** (default 24 hours), with
  `DEFAULT_IDEMPOTENCY_KEY_MAX_AGE` and `DEFAULT_IDEMPOTENCY_KEY_TTL`.
  `PurgeReport::idempotency_keys_deleted` reports what a sweep removed.

  **Behaviour change, and it is a trade rather than a pure fix.** Nothing ever
  removed an idempotency key. A key is released when the send holding it
  fails, and otherwise kept deliberately — it has to outlive the task it names
  or a late retry would execute the send a second time — so neither the
  retention sweep, which deletes tasks, nor the in-memory store's `task_ttl`
  and `max_capacity`, which bound tasks, touched the key index. A busy
  deployment accumulated one row per keyed send for the life of the database
  or the process.

  What expiring a key costs is exactly what the key was preventing: a retry
  arriving *after* the key expires re-executes the send. That is inherent to
  any expiring idempotency key, and it is why the default is a full day rather
  than something tidier — it has to exceed the longest window in which a client
  might still retry, and this SDK's own `RetryPolicy` is bounded in the low
  tens of seconds.

  The sweep will not delete a key younger than `terminal_max_age`
  (`effective_idempotency_key_max_age` clamps it). A key expiring while the
  task it names is still retained does not produce a replay and does not
  produce a clear "that task is gone" — it produces a *second* task alongside
  the first, and the caller ends up with two ids for one logical send. The
  invariant holds by construction rather than by the operator having read the
  documentation.

  Set either to `None` to keep the previous behaviour. The four SQL tables
  already carried a `created_at` column, defaulted and never read, so no
  migration is needed.

- **`RequestHandlerBuilder::with_inbound_trace_policy`** and
  `InboundTracePolicy` (`Continue` — the default and the previous behaviour —
  `Restart`, `Drop`). The inbound `traceparent` is joined while the
  `CallContext` is built, which is *before* the interceptor chain, which is
  where authentication happens: so on a public endpoint the peer choosing the
  `trace-id` and the sampling bit is, at that moment, anonymous. W3C Trace
  Context §7.2 names the consequences — forged `trace-id` collisions, and an
  attacker deciding what the operator's tracing vendor is billed for — and
  §3.4 names the remedy. Per handler rather than per process, so one process
  serving both a public front gate and an internal endpoint can hold a
  different policy on each.

- **`TaskStore::earliest_event_seq` and `TaskStore::event_log_covers`**, both
  defaulted, so an out-of-tree store compiles unchanged (`STABILITY.md` §4).
  `event_log_covers` is provided rather than implemented per store, so the
  comparison and its off-by-one live in one place. A resubscribe naming a
  position the log has dropped is now served the snapshot rather than a replay
  that silently begins later than asked.

- **`TaskStoreConfig::max_events_per_task`** (default `Some(512)`) and
  `DEFAULT_MAX_EVENTS_PER_TASK`. **Behaviour change:** the in-memory event log
  was unbounded and is now bounded.

- **`metrics::event_append_error`** with `position_conflict` and
  `task_absent`, both reported under `persistence_operation::EVENT_APPEND`
  alongside a warning. A collision whose stored payload matches what was
  offered is a replay rather than a loss and is deliberately not counted.

- **`metrics::MetricsHandle`**, and `with_metrics` on the five directly
  constructed stores, including `TenantAwareInMemoryTaskStore`, which hands it
  to every partition it creates.

- **`InMemoryTaskStore::is_prunable` and `idempotency_key_count`.**

- **`CHECK (seq > 0)`** on `task_events` and `tenant_task_events`. Postgres
  migration 6 adds it to existing databases. SQLite cannot — `ALTER TABLE` has
  no `ADD CONSTRAINT`, so an existing SQLite database keeps the unconstrained
  table; a database the runner builds from scratch gets it.

- **A `trace_context` fuzz target**, and `scripts/check_fuzz_matrix.py`, which
  fails when a fuzz target exists but no runner executes it. A target reaches
  CI through three files and only the workflow matrix has no build error
  behind it; `trace_context` itself shipped registered in `fuzz/Cargo.toml`
  and absent from the matrix, which is the drift the gate caught on its first
  run.

#### Fixed

- **`TraceContext::parse` could abort the process.** A `traceparent` whose
  55th byte falls inside a multi-byte character panicked, and the root
  manifest sets `panic = "abort"`, so a peer-supplied header on the public
  `on_send_message` path was process death rather than a rejected request.

- **A version-`00` `traceparent` with a trailing field was accepted.** W3C
  §3.2.2.2 gives version 00's grammar exactly; the lenient trailing-field rule
  is §3.2.4's and applies only to a *higher* version.

- **Reserved `trace-flags` bits were propagated.** W3C §3.2.2.5.2 and §4.3
  both require a vendor to zero unknown bits on outgoing requests. `flags()`
  still reports the byte as received, for diagnostics.

- **An oversized or over-32-member `tracestate` was discarded whole**, taking
  every vendor's state with it. W3C §3.3.1.5 requires truncating *entries* —
  those over 128 characters first, then from the end — which is what happens
  now. `MAX_TRACESTATE_LEN` is public and documented because §3.3.1.5 requires
  the maximum to be documented.

- **`TracePropagationInterceptor`'s "never overwrite" guard was
  case-sensitive**, and `http`'s builder appends rather than replaces, so a
  user interceptor setting `Traceparent` put two `traceparent` fields on the
  wire and which one the peer joined was its parser's choice. `tracestate` had
  no guard at all.

- **An append that wrote nothing was discarded in silence.** All four SQL
  stores dropped `rows_affected()` from their `ON CONFLICT ... DO NOTHING`
  insert, and both in-memory stores returned `Ok(())` for a task they no
  longer held.

- **The SQLite retention orphan sweep ran only when a purge deleted a task**,
  so rows stranded by a partly-failed purge survived to be replayed to
  whoever next reused the task id. It runs on every sweep now, batched.

- **A negative stored event position was made positive** by `unsigned_abs`
  instead of refused.

- **`prune_empty_tenants` destroyed live idempotency indexes.** It decided on
  `count()`, which counts tasks, and an idempotency key deliberately outlives
  the task it names. A partition whose tasks had all been swept still held a
  live index, and pruning it discarded that index — so the next retry carrying
  one of those keys found no claim and executed the send a second time, which
  is the one outcome a key exists to prevent.

### Breaking Changes

In the `0.x` series a minor release may break and a patch may not
(`STABILITY.md` §2), and each item below is a minor-only change by that list.
The four crates bump in lockstep.

`bindings/a2a-protocol-slimrpc` moves with them, to `0.5.0` on the `0.13`
pins: `SlimRpcServer::builder` takes `Arc<RequestHandler>`, so
`a2a-protocol-server` and `a2a-protocol-types` are *public* dependencies and
its requirement on them is a tight `0.13`. That bump is in this release rather
than after it because the binding depends on the workspace by `path` as well
as by version, so `^0.12` stops resolving the moment the crates read 0.13.0.
Publishing it remains the separate step `RELEASING.md` describes — and the
one that has never been run.

- **`RequestContext` is `#[non_exhaustive]` and carries a new `call_context`
  field.** Measured with `cargo semver-checks check-release -p
  a2a-protocol-server --baseline-version 0.12.1` **at the time that bullet was
  written**: 196 checks, 195 pass, and one fails —
  `struct_marked_non_exhaustive` on `RequestContext`. That "exactly one" was
  never right even then: the third bullet below renames a public field on
  `PurgeReport`, which is public, unconditionally reachable and not
  `#[non_exhaustive]`, so `struct_pub_field_missing` must fire too. Treat the
  figure as the record of one run, not as evidence that the list is complete —
  the list is complete because each entry below was derived from the diff. The
  added field is not a second finding, because once a struct is
  `#[non_exhaustive]` an added field is no longer separately observable.

  What breaks: constructing a `RequestContext` with a struct literal from
  outside `a2a-protocol-server`. Nothing in this repository did —
  `git ls-files '*.rs' | xargs grep 'RequestContext {'` finds only the
  definition and function signatures, and all 19 construction sites already
  use `RequestContext::new`. `book/src/deployment/testing.md` did teach a
  literal, in a `rust,ignore` fence that nothing compiled; it now uses `new`.

  Migration: replace `RequestContext { message, task_id, context_id, .. }`
  with `RequestContext::new(message, task_id, context_id)` followed by
  `.with_stored_task(..)` / `.with_metadata(..)` for anything else you set.

  The attribute is deliberate rather than incidental. This type grows, and
  every previous growth would have been a break for a literal nobody writes;
  `#[non_exhaustive]` is what makes this the last time.

- **`EventQueueReader::read` now yields a `StreamEvent`, not a
  `StreamResponse`.** The event travels the queue with the position it holds
  in the task's log: `StreamEvent { seq: Option<u64>, event: StreamResponse }`.
  `Reattached::Channel` and the persistence channel carry the same type.

  What breaks: any code that matched on the result of `read()` directly. The
  migration is `.event` for a value, or `.map(|e| e.event)` for the `Result`:

  ```rust,ignore
  // before
  while let Some(Ok(StreamResponse::StatusUpdate(u))) = reader.read().await { .. }
  // after
  while let Some(Ok(ev)) = reader.read().await {
      if let StreamResponse::StatusUpdate(u) = ev.event { .. }
  }
  ```

  Why it is a type change and not an accessor: the position has to be the
  *same number* the store wrote, and the only way to guarantee that is to
  assign it once and carry it. See the `Added` entry below.

- **`PurgeReport::journal_orphans_deleted` is now `orphan_rows_deleted`.**
  The sweep reclaims two side tables now, not one — the artifact journal and
  the event log — so the old name described half of what the number counts.
  Renamed rather than kept and widened: a field whose name names one of its
  two sources is read as the count for that source.

  Migration: rename the field at the read site. Its meaning is unchanged for
  anyone who had only the journal, and it is still normally zero (a non-zero
  count means rows outlived their task, which happens on a pool without
  `foreign_keys=ON`).

- **`build()` refuses an agent card it would have to edit.** The builder
  advertises the extensions this server can honour by appending to
  `capabilities.extensions`, and `canonicalize_card` strips only
  `signatures` — so those entries are inside the bytes a signature covers. A
  card signed before they existed was served canonicalizing to bytes nobody
  signed, and every client that verified it failed, silently, because nothing
  on the server side reads the signature again. Measured on the default
  configuration: the operator's card canonicalized to 360 bytes, the card
  actually served to 767.

  `book/src/deployment/security.md` tells operators to sign the card and hand
  it to `RequestHandlerBuilder`, so the instruction and the code together
  produced the failure.

  What breaks: `build()` now returns `InvalidParams` for a card that carries a
  signature and does not already declare every extension this server
  advertises. A card that declares them is served byte-identical to what was
  signed; an unsigned card, or one with an empty signature list, is
  advertised on as before.

  Migration: declare the extensions on the card *before* signing it —
  `IDEMPOTENCY_EXTENSION_URI` when the configured store reports
  `supports_idempotency()`, and `FAILURE_EXTENSION_URI` always — then sign the
  card that results. The security page shows the order in a block that
  compiles.

- **`IdempotencyClaim` and `KeyError` are `#[non_exhaustive]`.** Both were
  exhaustive public enums that callers must `match`. Every out-of-tree
  `TaskStore` matches on the first, and the outcomes it can report are not a
  closed set — an expiry, or an "in flight, wait" that would let a second
  caller be told to retry instead of handed a `TaskNotFound`, are both
  plausible additions. `STABILITY.md` §4 promises new variants will not break
  external implementations, and without the attribute they would have. Their
  siblings added in this same release — `RecordedEvent`, `StreamEvent`,
  `Outcome`, `CheckResult`, `TraceContextError` — were all marked; these two
  were missed.

  What breaks: a `match` on either from outside its crate needs a wildcard
  arm. `IdempotencyClaim` is also re-exported from `store` now, where its
  sibling `RecordedEvent` already sat — it is the return type of a public
  `TaskStore` method and was reachable only at
  `store::task_store::IdempotencyClaim`.

- **`FailureClass::ALL` is `&'static [FailureClass]`, not `[Self; 5]`.** The
  enum is `#[non_exhaustive]` so that adding a variant is not breaking, and
  then exported a constant whose *type* encoded the variant count — so the
  sixth variant would have changed `ALL`'s type and broken every caller that
  bound it, which is the exact break the attribute three lines above it
  promises not to inflict.

  Migration: `for c in FailureClass::ALL` becomes `for &c in
  FailureClass::ALL`; `.into_iter()` becomes `.iter().copied()`.

- **A keyed `message/send` is retried only against a peer known to honour the
  key.** `RetryTransport` treated any valid key in `Message.metadata` as
  making the send retryable, on the stated grounds that a server without the
  extension "refuses a keyed send outright". That is true of this SDK's
  server and of nothing else: the key rides in `Message.metadata`, which A2A
  defines as free-form, and the extension is deliberately not part of A2A
  v1.0 — a conformant Python, Java, Go or JavaScript server ignores it and
  runs the send. Retrying there starts a second task, which is the exact
  double execution `is_idempotent_method` exists to prevent. There is no
  handshake that would fix it: A2A's `A2A-Extensions` header is the server
  reporting what it activated, not a demand a server must reject.

  What breaks: a client built for a bare endpoint no longer retries a keyed
  send after an ambiguous failure. `ClientBuilder::from_card` reads the
  advertisement off the agent card and enables it automatically;
  `ClientBuilder::with_peer_honouring_idempotency(true)` asserts it for a peer
  you know implements the extension.

- **`message.id` is validated at ingress.** It is now checked for being
  non-empty after trimming, and bounded in length. It was checked nowhere,
  while being stored verbatim in `idempotency_keys.message_id`, keying history
  de-duplication, and interpolated into the error a caller sees when an
  idempotency key is held by a different message. A spec-invalid empty id was
  accepted, stored, and echoed in task history, where an empty id collides
  with every other empty id.

  **The length bound is not `max_id_length`**, and the difference matters.
  That field is documented as covering task and context ids, which are
  commonly short and often chosen by the deployment; a message id is minted by
  the client and is conventionally a UUID — 36 characters, which is what
  `Message::id`'s rustdoc tells a caller to send. Holding both to one number
  meant a deployment tightening `max_id_length` below 36, a reasonable thing
  to do for the two ids it was documented to cover, refused *every* message
  from a conformant client. `HandlerLimits::effective_max_message_id_length`
  is `max_id_length` floored at `MIN_MESSAGE_ID_LENGTH` (36), so the bound
  still tightens with the field but never past what the protocol requires.

  What breaks: a send whose `messageId` is empty, whitespace-only, or longer
  than `effective_max_message_id_length()` is rejected with `InvalidParams`
  instead of accepted.

### Added

- **SSE frames carry an `id:`, and `Last-Event-ID` resumes from it.** This is
  what the event log was for. A reconnecting client sends back the position of
  the last frame it saw and receives exactly what it missed, after the `Task`
  snapshot and before the live stream.

  The measured problem: in `mcp-bridge`'s demo the sample agent emits three
  progress steps 120 ms apart and the caller sees **one**, because a snapshot
  is a fold and a poller only ever observes the latest one. Three events
  arriving as one is not a rendering detail — it is the intermediate states
  being unobservable.

  **The position is assigned once, in `InMemoryQueueWriter::write`, and
  carried on both channels.** That is the whole design. The writer is the
  single fan-out point — it feeds the persistence channel that writes the log
  and the broadcast channel that feeds SSE — so the `id:` a subscriber reads
  and the `seq` the store holds are the same value by construction. The
  alternative, counting frames at the SSE layer, agrees with the log right up
  until the first lagged consumer, snapshot frame, or failed append; and a
  resumption offset that is off by one drops an event with nothing to
  indicate it. `tests/event_log_tests/resumption.rs` asserts the two
  sequences are equal, end to end through `build_sse_response`.

  **Frames the server synthesized carry no `id:`** — the `SubscribeToTask`
  snapshot, and the terminal frame the reattach hook rebuilds from stored
  state. Neither is in the log, so neither can be replayed from, and giving
  either one a position would hand the client an offset that loses an event.

  **A continuation resumes the numbering.** The queue is per-turn but the log
  is per-task, so the writer is seeded from `last_event_seq` before the
  executor starts. Without it a second turn would restart at 1 and, because
  appends are idempotent *by position*, have every one of its events silently
  swallowed as a replay.

  **The offset is client-supplied, so it is bounded and parsed.** Replay is
  capped by the new `HandlerLimits::subscribe_replay_limit` (default 1,000),
  and truncation is not loss: each replayed frame carries its own `id:`, so a
  client continues from the last one it got. A `Last-Event-ID` that is not a
  position is ignored rather than rejected — a client may echo one back from
  an unrelated stream, and failing the reconnect over it would turn a
  harmless mistake into a dropped connection. A store that keeps no log is
  the same: the header is ignored and the client gets the snapshot, which is
  the pre-resumption behaviour.

  `CorsConfig`'s default `Access-Control-Allow-Headers` gains `last-event-id`
  for the same reason `a2a-version` and `a2a-extensions` are already there: a
  cross-origin client that cannot send the header cannot resume.

  WebSocket, gRPC and SLIM streams are unchanged. Resumption is the SSE
  binding's `id:`/`Last-Event-ID` pair; inventing a spelling for the others
  would be a protocol extension this server made up. Note that
  `bindings/a2a-protocol-slimrpc` is outside the root workspace and takes
  `RequestHandler` as a public dependency, so `cargo check --workspace` does
  not compile it — its `event_stream` adapter needed the same one-line change
  and only the binding's own gates caught it.

- **An append-only event log, so what the agent emitted is recorded and not
  just what it folded into.** `TaskStore` gains `supports_event_log`,
  `append_event`, `last_event_seq` and `read_events`, all defaulted.

  A task's state is a fold: a stored snapshot folded together with deltas.
  Issue #130 happened because that fold was wrong in a way nothing could
  observe — artifacts from one task appeared on another, and the only record
  was the folded result, which is to say the bug itself. There was nothing to
  check it against. This adds the thing there was nothing to check against.

  The snapshot stays and stays authoritative for reads; the log is a
  parallel, ordered record of the events themselves. That makes a wrong
  snapshot *detectable*, gives a reconnecting subscriber the events it missed
  rather than a fold it cannot interpret, and is the substrate anything like
  a signed execution receipt would need.

  **`seq` is a position, not a counter.** The same `(task_id, seq)` written
  twice leaves one event, so a retried or overlapping append is safe without
  a read first — the property `sqlite_store::journal` already relies on, for
  the same reason. That makes `last_event_seq` load-bearing rather than
  convenient: a task parked at `input-required` and then continued gets a
  *second* processor, and a `seq` restarting at 1 would collide with
  positions the first one wrote, so the continuation's events would be
  silently dropped. Both processors seed from the store instead.

  **Both fold paths record, which is not one site but two.** A blocking send
  folds in `sync_collector`; a streaming one folds in the background
  processor. A log covering only streaming sends would be a history whose
  completeness depended on which method the caller happened to use. Each
  records *before* folding, so what the agent emitted is durable before the
  state derived from it is.

  A failed append is logged and counted under the new `event_append`
  persistence-error label, never fatal. The log is a record of the run, not a
  precondition for it — and a gap is visible, because the sequence skips a
  position.

  All four methods default to refusing rather than succeeding. A custom store
  that does not implement them reports no log, which is inconvenient;
  defaulting `append_event` to `Ok(())` would advertise a log that silently
  lost every event, which is worse. `supports_event_log()` is what a caller
  checks rather than discovering it by getting nothing back.

- **The event log is backed by every store this crate ships**, not only the
  in-memory one: `SqliteTaskStore`, `PostgresTaskStore`,
  `TenantAwareSqliteTaskStore`, `TenantAwarePostgresTaskStore` and
  `TenantAwareInMemoryTaskStore` all report `supports_event_log() == true`.

  Two tables, `task_events` and `tenant_task_events`, created both by the
  migration runners (`SQLite` migration 7, `PostgreSQL` migration 5) and by
  each store's `from_pool` DDL. Both paths, because a store built by one of
  them and not the other would still answer `supports_event_log() == true` —
  the flag is a property of the type, not of the schema — so the failure
  would be an empty history rather than an error at startup. That is the
  mistake the artifact journal shipped with, and there is now a test for each
  path on each backend.

  The tenant tables are keyed `(tenant_id, task_id, seq)`. Task ids are
  caller-supplied, so two tenants may legitimately use the same one; an
  unscoped log would hand one tenant's resuming subscriber the other's
  messages, which is a cross-tenant read of message content rather than a
  missed deduplication. Unlike `tenant_idempotency_keys` these tables *do*
  carry `ON DELETE CASCADE`: a key that outlives its task is the safe
  direction, an event that outlives its task is not.

  Deletion is explicit as well as cascaded, and the `SQLite` retention sweep
  reclaims orphans by anti-join, because `ON DELETE CASCADE` only fires with
  `foreign_keys=ON` — which this crate's own pool sets and a pool handed to
  `from_pool` may not.

- **A conformance harness for `AgentExecutor`, behind the `conformance`
  feature.** The TCK grades servers. Nothing graded the thing an adopter
  actually writes, and the paths they get wrong are the awkward ones —
  cancellation arriving mid-work, a parked task reported as an error, an
  event emitted after a terminal status. Those are exactly the cases people
  skip when writing tests by hand.

  `conformance::check(Arc::new(MyExecutor)).await.assert_pass()` drives the
  executor against a real event queue — no server, no ports, no model — and
  grades thirteen protocol invariants that hold for any agent whatever it
  does: it ends in a terminal or interrupted state, its transitions are legal,
  nothing follows a terminal status, artifacts carry ids, parking is not
  reported as an error, it returns within the harness's time limit, it
  observes a cancellation token set before it starts *and* one that arrives
  during its first write, it does not report cancellation as a failure,
  `cancel` leaves subscribers a terminal state, and it does not panic — on
  the normal run, the cancelled run, or the mid-run cancellation. It says
  nothing about whether the agent is any good at its job.

  **Three rules keep the score honest, and each is the repository's own.** A
  check that did not apply is *not graded* rather than counted as a pass — a
  report that grades nothing fails rather than reporting full marks, which is
  the failure `tck/`'s README records having shipped once. Every verdict
  carries its reason, so a failure says what was observed and why it matters,
  not just which check. And the executor runs inside `tokio::spawn`, so a
  panicking executor is a graded failure rather than a panic that takes the
  adopter's whole test run down with it.

  Its own tests are deliberately broken executors, each breaking one invariant
  (or, where an executor genuinely breaks two, naming both), asserting the
  harness names those and no others. A conformance
  harness whose failures are untested is a gate that cannot fail, which is
  the thing this repository checks for everywhere else — and the discipline
  paid immediately: the first draft of one fixture broke two invariants
  rather than one, and the harness was right where the test was wrong.

  No new dependencies. Documented on `book/src/deployment/testing.md` with a
  compiled example.

- **A failed task says why, as a class a caller can `match` on.** Shipped as
  the declared extension `https://a2a-rust.com/extensions/failure/v1`, so
  servers stay conformant and the TCK is unaffected.

  A failed task was `TaskState::Failed` plus prose written by whoever wrote
  the executor, which left a caller matching English for decisions that are
  genuinely different: never retry and fix the request; retry with backoff;
  stop and escalate to a person; retry only with more budget. An orchestrator
  without those either retries what can never succeed or abandons what would
  have worked on the second attempt.

  `a2a_protocol_types::failure` holds `FailureClass` — `InvalidRequest`,
  `Transient`, `PolicyRefusal`, `BudgetExhausted`, `Internal` — with
  `is_retryable()` and `needs_human()`, plus `set_class` / `class_of`.
  `Task::failure_class()` reads it off the status message. The prose stays
  exactly where it was, for the human reading the incident.

  **Most classification is automatic and its limit is stated rather than
  papered over.** When an executor returns `Err`, the server maps the error
  code — which can only ever produce `InvalidRequest` or `Internal`, because
  no `ErrorCode` carries the meaning "transient" or "refused on policy".
  Guessing `Transient` there would tell callers to retry things that can
  never succeed. An executor deadline is `BudgetExhausted`, since a deadline
  is a bound that was hit rather than an agent that broke. The two classes no
  error code can express need the agent itself, which is what
  `EventEmitter::fail(class, reason)` is for.

  `BudgetExhausted` is deliberately **not** `is_retryable()`: the identical
  request hits the identical bound. It is retryable with a larger budget,
  which is a different request. Asserted in its own test so the distinction
  cannot be quietly relaxed into "retry anything that is not the caller's
  fault".

  An unrecognised class from a newer peer reads as `Internal` rather than as
  an error. A peer that classifies more finely must not have its failures
  become unreadable, and "something went wrong at the agent" is true of every
  class a future version could add.

  Advertised unconditionally on the card, unlike idempotency, whose
  advertisement is gated on the store: every server classifies, because the
  classification happens in the server's own failure path. Never `required`,
  and an operator's own declaration is left alone.

  New book page, `book/src/client/failure-classes.md`, with both examples
  compiled and run as doctests.

- **W3C trace context crosses an A2A hop.** The defining property of A2A is
  that work crosses process and organisational boundaries, and until now
  nothing survived the crossing: each agent mints its own task id, so even
  correlating by hand failed at the first hop. One identifier now does —
  `traceparent`, which every other ecosystem already speaks.

  `a2a_protocol_types::trace_context` is the wire format: `TraceContext`
  with strict [W3C](https://www.w3.org/TR/trace-context/) parsing, the
  `traceparent`/`tracestate` header names, and `child` / `child_bytes` for
  deriving the next hop. It mints nothing — this crate depends on `serde` and
  `serde_json` and has no random source — so a caller supplies the identifier
  bytes, the same call the `Artifact::new` id already makes.

  The server parses an inbound `traceparent`, advances the span, and exposes
  it as `RequestContext::trace_context()`. The client gains
  `TracePropagationInterceptor`, which writes the ambient `CurrentTrace` onto
  every outbound request on the canonical A2A transports — JSON-RPC, REST and
  gRPC. **The `websocket` transport is the exception, and cannot be
  otherwise:** it carries headers only on the HTTP upgrade, and a
  `traceparent` fixed at connect time would report every request on that
  connection as the same span, which W3C §3.4 forbids. A dropped trace header
  is reported once per connection at `warn`; a dropped *credential* header is
  still reported on every call, because that one can be fixed by reconnecting.
  `CurrentTrace::scope` sets it and
  `CurrentTrace::start_root()` begins a chain. The indirection through a
  task-local is deliberate: a client is built once and reused for many
  delegated calls, so a trace fixed at construction would label all of them
  with the first request's.

  Three decisions worth stating, because each could reasonably have gone the
  other way. **The server propagates and never invents** — `None` from
  `trace_context()` is evidence that the caller was not tracing, not a gap in
  the plumbing, and a chain that wants a trace starts one at its first hop.
  **A malformed `traceparent` is dropped, not repaired** — uppercase hex, an
  all-zero id, a bad width: attaching work to a guessed-at trace is a wrong
  answer where a missing trace is only an absent one. **An explicit header on
  the request wins over the ambient scope**, because silently replacing it
  would move the callee into a different trace than its caller asked for.

  This is propagation, not tracing. Nothing records a span, measures a
  duration or exports anything, and the `otel` feature is still metrics-only.
  What it buys is that whatever *does* record spans can stitch the hops
  together — across languages, since `traceparent` is a wire format and the
  interoperability kit already runs Python, JavaScript, Go and Java agents.

  Proven over a real socket rather than asserted: `tests/trace_propagation_e2e.rs`
  drives two chained agents and checks that the trace id holds across both
  hops while each hop advances the span — the two ways a propagator can be
  wrong while looking right.

- **An executor can see who called it.** `RequestContext` gains
  `call_context`, and with it five accessors: `caller_identity()`,
  `tenant()`, `http_header(name)`, `activated_extensions()` and
  `request_id()`.

  The handler already built a `CallContext` carrying all of this and handed
  it to the interceptor chain — then dropped it. `build_request_context` took
  a message, two ids and a metadata blob, so the seam was severed by
  construction. The consequence was not cosmetic: an executor could not
  enforce "only this tenant may invoke this skill", which rules out a large
  class of real deployments, and the only channel left for anything
  caller-specific was `Message.metadata` — which the *caller* writes, so it
  is not a fact about the caller at all.

  Every accessor returns `None` rather than a default when nobody said.
  An executor that refuses anonymous work needs `None` to mean "nobody
  established an identity", never a value that reads like an answer.
  `call_context` is `None` when an executor is driven directly, as a unit
  test or a conformance harness does, rather than being given a synthetic
  context claiming a call that never happened.

  `CallContext` also gains `tenant()` and `with_tenant()` — private field,
  so additive — populated from the tenant scope the handler has already
  entered.

- **Constructors for `Message`, `Task` and `MessageSendParams`, and a
  `# Construction` pointer on the three `agent_card` structs.** Measured on
  this tree, the workspace holds 105 `Message` struct literals, 75
  `MessageSendParams` literals and 82 `AgentCard` literals. `Message` had two
  inherent methods and no constructor; `Task` and `MessageSendParams` had no
  `impl` block at all, so sending one line of text cost a ten-line `Message`
  literal — five of its eight fields written as `None` — inside a four-line
  `MessageSendParams` literal, and reading the answer back out cost a
  hand-written `artifacts → first → text` walk that six files under
  `examples/` had each written separately.

  `Message::new`, `user`, `agent`, `user_text`, `agent_text`, and `with_*` for
  all five optional fields. `MessageSendParams::new` plus `with_tenant`,
  `with_configuration`, `with_metadata`. `Task::text` and `Task::texts`.
  Nothing is breaking: every field stays public and every literal still
  compiles.

  The id stays a parameter rather than something the constructor invents.
  `a2a-protocol-types` depends on `serde` and `serde_json` and nothing else,
  so it has no random source, and a clock-derived id is not unique under
  concurrency — the same reason
  [`Artifact::new`](https://docs.rs/a2a-protocol-types) already takes one.

  `Task::text` skips artifacts that carry no text rather than stopping at the
  first, so it answers where the hand-written `artifacts.first()` chain
  returns `None`. That divergence is deliberate, documented on the method, and
  asserted in its test; it is the rule `Message::text` already applies across
  parts.

  The `# Construction` sections exist because discoverability, not absence,
  was the `AgentCard` problem: the twelve `with_*` builders have shipped since
  0.10 in `agent_card/builders.rs`, one file over from the field definitions,
  and three examples written in a single session on 2026-09-19 each reached
  for a literal instead — 38, 43 and 45 lines, verified by counting the three
  functions.

- **Client-supplied idempotency keys on `message/send`, as a declared
  extension.** A send whose connection drops after the request bytes are on the
  wire is ambiguous: the task may exist or it may not. The client rightly
  refuses to retry it — `message/send` creates server-side state — which left a
  caller with no recovery but to list a context's tasks and pattern-match
  message content to guess whether the send landed. With a key the server
  deduplicates on, a retry returns the task the first attempt created, in
  whatever state it reached, and the agent runs exactly once.

  Not part of A2A v1.0, and shipped as
  `https://a2a-rust.com/extensions/idempotency/v1` so servers stay conformant
  and the TCK is unaffected. `Message.extensions` is a list of extension URIs
  and cannot carry a value, so the key travels in `Message.metadata` under
  `a2a-rust.com/idempotency-key` while `extensions` declares the URI.
  `a2a_protocol_types::idempotency` holds the constants, `set_key`, `key_of`
  and `validate_key`.

  Keys are scoped to the **tenant**, not the context: a retry of a send whose
  `context_id` the server assigned would otherwise mint a fresh context and
  miss the deduplication, which is the case the key exists for. They must be
  unguessable — within a tenant the key is the handle to the task — so
  `MIN_KEY_LEN` rejects the short mistakes and the character set is restricted
  to ASCII alphanumerics and `-`, `_`, `.`, `:`, because a key reaches store
  keys and log lines.

  Three outcomes, and the third is the one worth stating. A free key is
  claimed; the same message replays to the first task without executing
  anything; a *different* message on the same key is refused with
  `InvalidParams`. A genuine retry resends the identical message, so a
  differing message id means the key was reused across two distinct sends, and
  returning the first task there would answer a message that was never sent —
  a silent wrong answer, which is the failure a caller can least detect.

  The claim sits under the per-context lock, after everything that can reject
  the request on its own terms and before the send's first side effect:
  claiming later would let two racing duplicates each lease an event queue,
  claiming earlier would burn a key on a request that was never going to run.
  Every failure path after it releases the key, the discipline the queue lease
  and cancellation token already follow — a key held by a send that never
  created a task would make the caller's own retry replay to a task that never
  existed. A streaming replay attaches to the original task's live queue as
  `SubscribeToTask` does, rather than returning a snapshot and closing, which
  would read as a task that had finished emitting.

- **Agent cards advertise idempotency support, derived from the store.** The
  builder adds the extension exactly when `TaskStore::supports_idempotency()`
  is `true`, so the card and the behaviour cannot drift: swapping in a store
  without the index removes the advertisement in the same change. An operator's
  own declaration is left untouched, `required` flag included; the generated
  entry is never `required`, since a client that sends no key must not be
  locked out. A server whose store cannot honour a key refuses a keyed send
  rather than running it undeduplicated, so the failure mode is a loud refusal
  and never silent at-least-once delivery.

- **Every bundled store honours keys**: in-memory, `SQLite`, `PostgreSQL` and
  the tenant-aware variant of each. The SQL stores claim inside one
  transaction — `INSERT ... ON CONFLICT DO NOTHING` picks the winner and the
  loser reads the holder under `SQLite`'s write lock or Postgres's
  `FOR UPDATE` — because a claim that neither inserted nor found a holder must
  never pass for a successful one. The tenant-aware SQL stores key the table
  `(tenant_id, key)` to match `tenant_tasks`: without the tenant in that key
  one tenant's key would collide with another's and the second tenant's send
  would replay to the first tenant's task. No table carries a foreign key to
  the tasks table, deliberately — a cascade would free the key when a retention
  sweep removed its task, letting that send run a second time.

  `TaskStore` gains `supports_idempotency`, `claim_idempotency_key` and
  `release_idempotency_key`. All three have defaults, so existing custom stores
  keep compiling; the defaults report no support rather than a successful
  claim, so forgetting to implement them cannot produce a send that quietly
  runs twice.

- **A keyed send is retried on the ambiguous failure it was built for**
  (client). `SendMessage` and `SendStreamingMessage` remain non-idempotent by
  method, but a send carrying a valid key now joins the idempotent set for that
  call, so a dropped connection is retried instead of surfaced. The safety
  argument needs no capability negotiation: either the key is honoured and the
  retry is deduplicated, or the server refuses the keyed send outright rather
  than running it undeduplicated. The key is validated rather than merely
  found, so the predicate answers "this send will be deduplicated" and not
  "something is present under that name".

- **Book: "Idempotent Sends"** (`book/src/client/idempotency.md`), covering how
  to attach a key, how to choose one, what the three outcomes mean, how to tell
  whether a server honours keys, and the two obligations on a custom store —
  an atomic claim, and never deleting a key with its task. Every Rust block on
  the page is compiled and run as a doctest.

- **Tool calling, in `examples/rig-agent`.** Nothing in this repository showed
  it: `grep -ril 'tool_call\|ToolCall\|tool_choice\|function_call'` over
  `examples/`, `crates/` and `book/src/` returned zero files, and the rig
  example's own comment said "no tools, no history". An A2A SDK whose examples
  never call a tool cannot show an adopter how to build an agent that does
  anything, which is the gap this closes.

  It is deliberately **not** in the crates. A2A has no tool concept — the
  protocol carries messages, tasks and artifacts *between* agents and says
  nothing about what happens inside one — so tool calling belongs between an
  executor and its model, and `examples/rig-agent/src/tools.rs` imports no
  `a2a-protocol-*` type at all. Many model turns, one A2A task. Putting a tool
  abstraction in `a2a-protocol-server` would make it a framework and put it on
  a model-provider's release treadmill; hosting one costs neither.

  `src/agent.rs` writes the loop out rather than delegating to `rig-agent`'s
  runner, because the loop is the part an adopter has to get right. Three
  things it gets right, each a bug if the shape is copied without them: the
  catalogue goes on *every* request, since a provider holds no state between
  turns; a tool error is returned to the model as that call's result rather
  than failing the task, so an unknown service is recoverable; and the loop is
  bounded (`MAX_TURNS`, 6), because unbounded it holds the A2A task open until
  the server's hour-long executor timeout. `src/tools.rs` declares two tools
  over a fixed inventory — one with no arguments, one with a required string —
  which covers both JSON Schema shapes a provider must encode and, because a
  model must discover the inventory before querying it, produces a *two*-round
  loop. One round is the case that still works when the loop is written wrong.

  When tools ran, the task carries a second `tool-trace` artifact naming each
  call and its result. Eleven new tests drive the loop against a scripted
  model that records what it was sent, covering the tool-error and turn-limit
  branches a live provider reaches only by chance.

- **`examples/mcp-agent` — tools over MCP, agents over A2A.** The A2A project's
  own guidance is that the two protocols compose: *"A2A handles inter-agent
  collaboration and MCP handles tool integration."* Nothing showed what that
  looks like, so this does. The agent spawns an MCP server as a child process,
  asks it what it offers with `tools/list`, builds its catalogue **and its
  agent-card skills** from the answer, and calls it with `tools/call`. A2A
  never carries a tool call; MCP never carries a task; they meet in one
  process and nowhere else.

  It is `rig-agent` with exactly one thing changed, which is the point. Diff
  the two `src/agent.rs` files: the loop is the same — same three rules, same
  bound — and only the tool source moves. Nothing in the crate knows what the
  tools are, so `MCP_SERVER_BIN` repoints it at any other MCP stdio server, in
  any language, with no code change.

  The distinction the bridge exists to keep is between a tool that **ran and
  refused** and a session that is **gone**. The first is information for the
  model — it can read the reason and try something else — so it returns as a
  tool result and the A2A task stays alive. The second cannot be re-prompted
  away, and answering regardless would hand the caller a guess dressed as a
  researched answer, so the task fails. The model-unreachable fallback the
  sibling examples use is deliberately not extended to a dead MCP server, and
  an agent that cannot reach its tools at startup refuses to start rather than
  serve a card advertising them.

  Built on `rmcp` 3.4, the official Rust MCP SDK, for both halves: the client
  and the bundled `mcp-tool-server` binary. Fourteen tests, and the split
  matters — ten drive the bridge over an in-process `tokio::io::duplex` pipe,
  which is real MCP framing and reaches the branches (a refusal, a session
  dying mid-call, a model that never stops calling); four spawn the *shipped*
  binary the way a real MCP client does, which is what proves its schemas
  reach the wire from the `JsonSchema` derive and that its `main` writes
  nothing but MCP to stdout. A stray `println!` there desynchronizes every
  frame, and no in-process test would notice.

  Verified end to end against llama.cpp `b23701f` serving Qwen3-1.7B: both
  demo questions answered over A2A with tools fetched over MCP, including the
  recovery path — the server refused an unknown service, the refusal crossed
  back as a tool result, the model read the hint the server wrote and called
  the tool it named. Worth noting from that run: the first question took *two*
  MCP round trips where the same question against `rig-agent`'s in-process
  tools took one. The tool count is the model's choice, not a property of the
  code, which is why the bound is not optional.

- **`examples/mcp-bridge` — the reverse direction: an A2A agent, callable as
  an MCP tool.** `mcp-agent` serves A2A agents; this serves everyone else. An
  MCP client needs no A2A library, no A2A concept and no new code to call a
  remote agent — it needs a command in its server list. The bridge discovers
  the agent card at startup, publishes one MCP tool per advertised skill with
  the agent's own descriptions, and forwards each call as an A2A task.

  More of A2A survives the crossing than expected, and that is the finding
  worth recording. `Completed` is a tool result and `Failed`/`Rejected`/
  `Canceled` carry `isError`, which is unsurprising. What is new is that
  MCP's `2026-07-28` specification added long-running tasks (SEP-2663), so an
  A2A task now maps onto an MCP task rather than a blocked request: the
  caller polls `tasks/get`, the agent's own status messages cross to it, and
  `tasks/cancel` reaches the agent as `CancelTask`, cooperative on both
  sides. Before that extension this bridge could not have been built without
  lying about one side or the other.

  Three asymmetries are stated where they happen rather than papered over.
  A2A skills carry no argument schema — A2A's calling convention is a
  `Message` — so every published tool takes one string and the skill's
  description is what steers a caller's model; inventing a schema per skill
  would publish a contract the agent never agreed to. A2A has no skill
  selector, so the chosen tool travels as message metadata the agent may
  route on or ignore. And `input-required` is reported rather than bridged:
  MCP has a counterpart, but wiring it needs the bridge to hold an A2A task
  id across MCP calls, and a half-built version would strand tasks with no
  way to answer them, so a paused task returns an error saying what the agent
  is waiting for — a caller can tell "waiting" from "broken".

  It refuses to start when the agent is unreachable, when the card advertises
  no skills, and when two skill ids collide as MCP tool names (naming both).
  An MCP server that came up with an empty or wrong tool list is
  indistinguishable, to its caller, from an agent with nothing to offer.

  Fifteen tests. The mapping is pure, so its awkward cases are asserted on
  values; the bridge needs a real agent, so those tests start one on
  localhost and drive the bridge with a real MCP client over an in-process
  pipe. `bridge-demo` covers the child-process spawn by standing up a sample
  agent, spawning the bridge against it and acting as an MCP client — no
  model, no network, deterministic.

  Its transcript is in the README, and it earns its place: the sample agent
  echoes back the skill hint, proving the metadata arrived; the agent's own
  progress message crosses two protocols into `tasks/get`; and it arrives
  *coarsened* — three agent steps 120 ms apart, of which the caller sees one,
  because the bridge polls at 250 ms. The cost of polling rather than
  streaming, visible in the output rather than discovered later.

- **The OTLP export sequence is documented.** `book/src/deployment/observability.md`
  covered metrics conceptually and the instrument catalogue, and said nothing
  about how to make bytes leave the process. It now carries the call order as
  a compiled example, and the four things that bite:

  `init_otlp_pipeline` **must be called from inside a Tokio runtime** — tonic
  spawns onto the ambient runtime while building the channel — and since the
  release profile sets `panic = "abort"`, getting that wrong aborts the
  process rather than returning an error. The transport is **gRPC only**, so
  `OTEL_EXPORTER_OTLP_PROTOCOL` has no effect and an endpoint pointed at a
  collector's `:4318` produces silence. **`OTEL_SERVICE_NAME` is overridden**
  by the `service_name` argument: `Resource::builder()` installs
  `EnvResourceDetector`, which reads it, and `.with_attributes` then
  overwrites what it found — the opposite of what the OTel
  environment-variable specification prescribes. And the install is
  **last-write-wins, process-wide**.

  A table now says which `OTEL_*` variables reach the exporter (endpoint,
  headers, timeout) and which do not (protocol, service name). `book-tests`
  gains the `otel` feature so the sequence compiles as a doctest rather than
  being an `ignore`d block, which is the same reason `postgres` was added for
  the horizontal-scaling page.

  The catalogue section gains **the Prometheus name for every instrument**,
  measured by rendering the catalogue through `opentelemetry-prometheus`
  0.32.0 rather than derived from the translation rules: `a2a.server.requests`
  arrives as `a2a_server_requests_total`, `a2a.server.latency` as
  `a2a_server_latency_seconds`, and gauges keep their names unsuffixed. The
  dotted names are *correct* — OTel names metrics
  `http.server.request.duration` — and nothing in the book had previously
  said what they translate to, which is the likeliest reason mapping them
  took guesswork.

  Two deviations are now stated there rather than changed, because the page
  calls these names a contract and changing a published contract needs its
  own release and upgrade note: the units are not UCUM (`request` should be
  `{request}`), which the same measurement shows costs nothing observable —
  UCUM and non-UCUM units render byte-identical exposition through this
  exporter — and `a2a.server.latency` should conventionally be
  `a2a.server.request.duration`, which *is* visible, since no OTel dashboard
  template will match `a2a_server_latency_seconds`.

### Changed

- **Seven example files and five book pages now use the new constructors.**
  Net 113 lines removed from `examples/` and 108 from `book/`: `rig-agent`'s 43-line
  `AgentCard` literal, `mcp-bridge`'s 45-line one and `mcp-agent`'s 38-line
  one become chained builders; `hello-agent`'s 16-line send becomes
  `MessageSendParams::new(Message::user("test-msg", parts))` and its
  five-line artifact walk becomes `task.text()`. The examples are what an
  adopter copies, so leaving them on the literal would have shipped the
  ergonomics and taught around them.

  Three cards stopped setting `AgentCard.url`. That field is
  `#[serde(skip_serializing)]` — a served card never carries it, so no client
  could ever read what those agents were setting — and its own doc says to
  publish an address through `supported_interfaces`, which all three already
  did. `rig-agent`'s test asserted on it; it now asserts on
  `supported_interfaces[0].url`, which is the address a client actually sees.

  `examples/mcp-agent` and `examples/mcp-bridge` each carried a `uuid_like()`
  helper — a nanosecond timestamp, not unique under concurrency and
  sequential enough to guess — written to avoid a `uuid` dependency the
  workspace already carries and seven other examples already use. Both now
  take `uuid` and call `Uuid::new_v4()`.

  One book snippet did not compile. `book/src/deployment/testing.md` set a
  `context_id` field on `MessageSendParams`, which has no such field — the
  snippet was inside a `rust,ignore` fence, so nothing ever built it. It is
  now three lines that would.

- **`examples/rig-agent` defaults to `qwen3:1.7b`, not `qwen3.5:0.8b`.**
  Measured 2026-09-19 against llama.cpp `b23701f` with `--jinja` and the
  example's own catalogue: Qwen3.5-0.8B-Q4_0 answers in prose, and forced with
  `tool_choice: "required"` still emits no tool call, running to
  `finish_reason: "length"` after 7,400+ tokens. Qwen3-1.7B-Q4_K_M returns
  `finish_reason: "tool_calls"` on the first request. The sibling LLM examples
  call no tools, so their 0.8B default stays right for them; the README
  records both results so the next person does not rediscover this.

  The end-to-end run is in `examples/rig-agent/README.md` as two verbatim
  transcripts. The first is kept **with the model's arithmetic error intact** —
  it reports an uptime the tool never returned, and the trace line beneath it
  shows the real figure. That is the argument for the trace artifact stated as
  evidence rather than as a claim: grounded and verifiable are different
  properties, and only the second is one the protocol can carry.

### Fixed

- **The idempotency extension's card description carried 30 literal
  spaces.** Someone reflowed a multi-line string literal without stripping
  the indentation, so the description a client reads off the card said "a
  retried&nbsp;&nbsp;…&nbsp;&nbsp;send returns". Noticed while adding the
  failure extension beside it.

- **Three documents said the SDK had no `traceparent`, and now say what it
  does have.** `otel/pipeline.rs`'s "no part of this workspace reads or
  writes W3C `traceparent`", `book/src/deployment/observability.md`'s "There
  are no traces", and the `docs/rust-sdk-assessment.md` comparison row were
  all accurate on 2026-09-19 and were made false by the change above. Each
  now separates the two claims that were being run together: no spans are
  exported, which is still true, and trace context is not carried, which is
  no longer. The assessment row has now been corrected twice in opposite
  directions, which is recorded there rather than quietly overwritten.

- **The spawned executor ran under the empty tenant.** `spawn_executor` used
  a bare `tokio::spawn`, and `TenantContext` is a `tokio::task_local`, which
  a spawned task does not inherit. So every store call an executor made ran
  with `TenantContext::current() == ""` — against a tenant-aware store, that
  partitions the executor's writes away from the request that caused them.
  The handler's own doc comment recorded the mechanism and the measurement
  ("The same probe's executor saw `\"\"`") without the spawn being fixed.

  Two other spawns in the same crate already did the right thing — the
  background event processor and the sync collector both capture the tenant
  and re-enter `TenantContext::scope`. This one now does too.

  The regression test is `the_spawned_executor_runs_inside_the_tenant_scope`
  in `tests/request_context_tests.rs`, and it was checked against the defect
  rather than merely written: with the scope removed it fails naming the
  empty tenant, with it in place it passes.

- **A test's panic hook was silencing every other test in its binary.** Three
  tests — two in `a2a-protocol-server`'s `agent_card/hot_reload.rs`, one in
  `a2a-protocol-client`'s `auth.rs` — wrapped an expected panic in
  `take_hook()` / `set_hook(Box::new(|_| {}))` / `set_hook(hook)`. `set_hook`
  is process-global and libtest runs a binary's tests as parallel threads in
  one process, so each of those windows silenced *every* thread for its
  duration: a genuinely failing test that raced one was listed under
  `failures:` with no `---- stdout ----` section and no message, and the build
  went red with its reason absent from the log, non-deterministically.

  It had already cost something measurable. Three gates in
  `scripts/prove_gates_fail.sh` —
  `cargo test -p a2a-protocol-server --features {sqlite,postgres,auth-jwt}` —
  reported `INCONCLUSIVE` rather than `PROVEN`, because the injected defect
  failed correctly and its panic message never reached the output the prover
  greps.

  The fix is a deletion at all three sites. `catch_unwind` returns the payload
  each test asserts on, and libtest already captures panic output per test and
  discards it when the test passes, so the swap was buying a suppression it
  already had. Measured on the pattern in isolation: an unrelated failing test
  in the same binary lost its marker in 3 of 3 parallel runs with the swap and
  kept it in 3 of 3 without, and the expected panic's own text appeared in 0 of
  3 runs either way.

- **`examples/rig-agent`'s executor was undocumented.** Its doc comment had
  run together with the one below it, so both attached to `SLOW_PREFIX` and
  `RigAgentExecutor` carried none.

- **Three documents claimed OpenTelemetry trace export that does not
  exist.** The `otel` feature is metrics-only: `opentelemetry_sdk` is
  compiled with `features = ["metrics", …]`, `opentelemetry-otlp` with
  `["grpc-tonic", "metrics"]`, there is no `TracerProvider`, and
  `grep -rni 'traceparent\|tracestate' crates/` matched nothing before this
  change — it now matches exactly the one doc comment added here that says so.
  Corrected in
  `book/src/deployment/troubleshooting.md` ("Metrics / **traces** over
  OTLP"), `docs/rust-sdk-assessment.md` (`✅ otel feature: **traces** +
  metrics`, in a row comparing this project against `a2a-rs` — an overclaim
  in the worst possible place, now `◑ metrics only` with a §10 recording the
  change), and `book/src/deployment/observability.md`, which told readers
  that identifiers on spans let one incident "be followed across the
  delegation chain when agents call agents". It cannot: separate processes
  produce separate span trees, and each agent mints its own task id, so the
  identifier changes at every hop.

- **`init_otlp_pipeline_with_endpoint`'s rustdoc said
  `OTEL_EXPORTER_OTLP_PROTOCOL` still applies.** It cannot —
  `MetricExporter::builder().with_tonic()` is hard-coded and the
  HTTP/protobuf exporter is not compiled in. A reader who set
  `http/protobuf` and pointed the endpoint at a collector's `:4318` would
  get gRPC spoken at an HTTP port, silence, and a doc line saying that
  should have worked.

### Internal

- **`scripts/check_panic_hooks.sh` forbids process-global panic hooks.** The
  regression test for the defect above, and for its class: a grep gate is the
  only mechanical check available, because "some other test lost its message"
  is not observable from inside the test that lost it. Registered in `ci.yml`'s
  Format job, paired with an injection in `scripts/prove_gates_fail.sh`, and
  listed in the gate-reachability input table. Proven both ways before
  shipping — exit 0 on the fixed tree, exit 1 naming all six lines on the tree
  as it stood, and `prove_gates_fail.sh --only check_panic_hooks` reporting
  PROVEN with the tree clean afterwards. The harness counts 66 gates now.

  The three gates the hook had made INCONCLUSIVE —
  `cargo test -p a2a-protocol-server --features {sqlite,postgres,auth-jwt}` —
  were re-run afterwards and all three report PROVEN, "gate exited 101 citing
  the injected defect". With a local PostgreSQL installed, three gates that
  had been PRE-BROKEN for want of one prove as well, so the selection is
  9 proven, 0 unproven.

- `check_doc_versions.py` gates dependency snippets in prose against the
  current release line. `a2a-protocol-sdk = "0.7"` means `^0.7`, which resolves
  to nothing in the 0.12 line, so a reader copying it got an SDK without any
  fix released since — including the #130 artifact-duplication fix. Measured at
  the 0.12.1 tag, 28 snippets across 14 files named 0.7, 0.8 or 0.11 against
  exactly one that was current. `release.yml` could not catch it: it verifies
  the four crate manifests against the tag, and prose is not a manifest.
  Historical snippets are allowlisted with a reason each, and an entry matching
  no snippet fails the gate, so the allowlist cannot rot either.

- **`tck.yml` gates the three example agents' TCK grades.** Those figures were
  dated manual measurements printed beside "no CI job gates it", and one had
  already drifted to "20/20" before a person reading the page caught it. The
  blocker was recorded as a CI-cost decision; it was not one. The TCK grades
  protocol conformance, which never reaches the executor's brain, and all three
  agents are written to serve without a model — measured 2026-09-17 with no
  `OPENAI_API_KEY`, no `OPENAI_BASE_URL` and nothing on any model port, all
  three score 21/21 graded, 0 failed, 1 N/A, the same figure the pages claimed.
  The new `tck-example-agents` matrix costs a build per agent and no secret.
  Eight documentation sites now name the job instead of disclaiming one.


## [0.12.1] - 2026-09-17

### Changed

- **SLIMRPC branch-spec triage: the broadcast-live design is now
  *collaborative task*.** The nightly `Official TCK` run of 2026-09-12
  ([34671180590](https://github.com/tomtom215/a2a-rust/actions/runs/34671180590))
  failed at `scripts/check_slimrpc_spec.sh`, as designed, on two untriaged
  files. Upstream `cb245fc` (2026-09-11) reframed the design on
  `feat/slimrpc-collaborative-channel`: `spec/v1/a2a-broadcast-live.md` gave way
  to `spec/v1/a2a-collaborative-task.md`, `spec/v1/slimrpc-broadcast-live.md`
  was renamed `spec/v1/slimrpc-collaborative-task.md`, and the profile was
  rewritten seven further times the same day. Both are triaged as not followed —
  but the SLIMRPC profile's **reason has changed and the old one is not
  reused**: its §§3.1 and 4 now accept 1.0's `SendStreamingMessage` as the
  activation call, so "requires A2A 1.1 and `SendLiveMessage`" is no longer true
  of that text. What blocks it now is the base spec's §4.3, which makes
  appending peer messages to A2A 1.1's `timeline` as `TimelineEntry(Message)` a
  MUST; its native mode's requirement for SLIM shared-responses group channels,
  which this binding does not implement; and the document never having reached
  upstream `main`. Recorded in `KNOWN_BRANCH_SPECS`, the vendored spec's README
  table, the binding's README and the book chapter.
- **Correction: the 2026-09-02 rename was `daddfb2`, not `0c38776`.** Entries
  written for the previous triage — including the 0.12.0 note below — credited
  the `slimrpc-collaborative-channel.md` -> `slimrpc-broadcast-live.md` rename
  to `0c38776` (2026-09-03), which changes only upstream's `examples/` tree. It
  was the branch *tip* when `check_slimrpc_spec.sh` observed the move, and that
  check clones `--depth 1`, so a tip is all it can see. The live documents now
  cite `daddfb2` (2026-09-02); the released 0.12.0 entry is left as published.

### Fixed

- **A new task on an existing context no longer inherits the previous
  task's artifacts, history and metadata** (server). When a client sent a
  second message on a context without a `taskId`, the server minted a fresh
  task id — correctly, that is a new round — and then built that task with
  the previous task's `artifacts`, `history` and `metadata` copied in. The
  agent's own output was merged on top, so every round after the first
  returned the whole context's accumulated artifacts, growing by one each
  turn ([#130](https://github.com/tomtom215/a2a-rust/issues/130)).
  `a2a.proto` scopes all three fields to the task, not the
  context: artifacts are "a set of output artifacts for a `Task`", history
  "the history of interactions from a `Task`", metadata "custom metadata
  about a task". `build_initial_task` had keyed the carry-forward on "a task
  exists for this context" when `resolve_task_id` had already decided the
  opposite; it now carries state forward only when the ids match — a genuine
  `InputRequired` continuation of the same task (A2A spec §3.4.3), where
  wiping the accumulated state would be the bug. Only `artifacts` was
  visible on the send response, because history is omitted from it unless
  the client asks for a `history_length`; `history` and `metadata` leaked
  the same way and are observable through `GetTask`. Affects 0.11.0 and
  0.12.0. Executors are unaffected: a multi-turn executor reads the previous
  task from `RequestContext::stored_task`, which is passed to it separately.
  This changes the wire shape for a given input and is therefore
  patch-eligible only as the specification correction `STABILITY.md` §2
  carves out; the requirement it corrects is `Task.artifacts`,
  `Task.history` and `Task.metadata` being task-scoped in `a2a.proto`.
- **Three wrong claims about the `rig-agent` example, and a stale TCK figure
  in seven places.** The prose described messages being passed to
  `rig_core::agent::Agent`, a type the example does not use — rig 0.41 moved
  the run loop into a separate crate, so the example defines its own
  single-turn `RigAgent<M>` over `rig_core::completion::CompletionModel`. The
  quickstart said the model defaults to `gpt-4o-mini`; it defaults to
  `qwen3.5:0.8b`. And "passes the TCK 20/20" was carried in seven places
  against a measured 21/21 graded with 1 not applicable. Each TCK site now
  carries a dated measurement, the reproducing command, and the fact that no
  CI job gates it — which is how the figure drifted, since `tck.yml` points
  only at `echo-agent` and `a2a-tck-sut`, never at the example agents.
- **Six markdown table rows rendered with missing columns.** Four rows in the
  two example tables (`examples/README.md`, `book/src/examples/overview.md`)
  were short of the declared four columns; `benches/README.md` was missing a
  bench module's File cell and had listed 14 of the 15 `[[bench]]` targets
  registered in `benches/Cargo.toml`; and a retired waiver row in
  `book/src/reference/conformance-history.md` had five cells against a
  six-column header, its `Why` cell having swallowed the `Removable when`
  content. A repo-wide scan that discounts pipes escaped inside code spans now
  reports zero mismatched rows in any markdown file.

### Security

- **`rustls` advanced to 0.23.45 (RUSTSEC-2026-0285).** rustls below 0.23.45
  accepted TLS 1.3 handshake messages sent at the wrong encryption level when
  they followed a key-changing message in the same record, contrary to
  RFC 8446 §5.1, which requires terminating the connection with
  `unexpected_message`. The advisory states the handshake transcript stays
  authenticated, so this is a conformance failure rather than a
  handshake-forgery or confidentiality break. The workspace lockfile moves
  0.23.44 → 0.23.45; no manifest changed, because every crate here already
  requests `rustls = ">=0.23, <0.24"`.
- **The SLIMRPC binding cannot take that upgrade yet, and now says so.**
  `bindings/a2a-protocol-slimrpc` reaches rustls with default features, so
  `aws-lc-rs` is in its graph, and rustls 0.23.45 requires `aws-lc-rs ^1.18`
  while `mls-rs-crypto-awslc 0.23.0` — the only release in the `^0.23` range
  that `agntcy-slim-auth 0.15.4` admits — pins `aws-lc-rs =1.16.2`. No
  published version of either resolves it, verified against the crates.io
  index rather than assumed. The binding's `deny.toml` therefore carries a
  dated `RUSTSEC-2026-0285` ignore recording the full chain, the severity as
  the advisory states it, and the two upstream releases that would let the
  entry be deleted. The four published crates are unaffected: they select the
  `ring` backend and are on 0.23.45.

### Internal

- **`check_slimrpc_spec.sh` now prints only the remedy that applies.** Every
  failure — including a branch-only specification nobody has triaged, which
  touches no vendored file — ended with "read the diff above",
  `./scripts/check_slimrpc_spec.sh --update` and "refresh the hashes". The
  2026-09-12 nightly failed exactly that way: no diff, every vendored copy
  matching, and instructions to re-vendor. The vendored-copy verdict is now
  tracked separately from the run's overall verdict, so that block prints only
  when a vendored file really has drifted. Exit codes are unchanged, and both
  paths were exercised: removing a `KNOWN_BRANCH_SPECS` entry exits 1 with the
  triage instructions and no re-vendor advice; appending a line to
  `spec/slimrpc_v1/slimrpc.md` exits 1 with the diff and the re-vendor advice.
- **The shutdown warning tests now say why a capture failed.** `Test (stable,
  windows-latest)` in
  [34679831264](https://github.com/tomtom215/a2a-rust/actions/runs/34679831264)
  failed `hung_cleanup_is_warned_about` with `got []`, while the behavioural
  half of that same test passed — the timeout fired and the report was correct,
  and only the WARN capture came back empty. Two guards replace that message.
  `warnings_during` no longer discards the result of `set_global_default`: if
  another global subscriber won the race, `WarnCapture` is not in the dispatcher
  and no capture can ever fill. The comment there denied this ("the sink below
  is what decides whether we capture anyway"); it is false, and it is now a
  panic that names the cause. Second, a canary WARN emitted from the test file
  with the sink armed, and cleared before the code under test runs, separates
  "the capture never worked" from "the callsite under test emitted nothing" —
  the latter being the 2026-08-19 interest-cache failure the file already
  documents. The canary is deliberately a different callsite, so it proves the
  subscriber, sink and level filter and nothing about that callsite's cached
  interest. Both guards proven to fire by injection: dropping every event in the
  layer trips the canary; installing a competing global subscriber first trips
  the install check. **The intermittent Windows failure itself is neither fixed
  nor explained by this** — fifteen runs of the full `--all-features` lib suite
  on Linux (nine before the change, six after) did not reproduce it — so the
  next occurrence will say which half is broken instead of `got []`.
- **Fixed: the shutdown warning capture collected other threads' events.**
  The guards above did their job on the first try. `Test (1.88,
  windows-latest)` in
  [34681613416](https://github.com/tomtom215/a2a-rust/actions/runs/34681613416)
  failed `clean_shutdown_warns_about_nothing` with `got ["executor cleanup did
  not finish within the shutdown timeout"]` — and the canary did not fire, so
  the capture was working. That narrowed it to one thing: the warning was not
  this test's. Its own `shutdown_with_timeout` reported
  `executor_cleanup_completed == true`, so the code under test never reached
  that `trace_warn!`. The sink is process-wide and the subscriber global, while
  `CAPTURE_LOCK` serialises only the tests in that one file — so a WARN from
  any of the other ~1100 tests running in parallel landed in it. `ACTIVE_SINK`
  now records the capturing thread alongside the sink and the layer ignores
  events from any other thread; the subscriber stays global, which is what the
  callsite-interest reasoning there requires. Regression test
  `a_warning_from_another_thread_is_not_captured` reproduces the CI failure
  exactly — without the fix it fails with the identical message. Note the
  constraint this adds, recorded at the static: every test in that file drives
  a current-thread runtime, so a future test on a multi-threaded runtime would
  have to widen the check. **The earlier `got []` failure in run 34679831264 is
  a different symptom and is still unexplained** — it is not this bug, since an
  empty sink is not a contaminated one.

## [0.12.0] - 2026-09-10

### Breaking Changes

- **Public configuration structs gain fields; struct-literal construction
  breaks.** `HandlerLimits` gains `push_delivery_budget` and
  `executor_drain_timeout`; `WebSocketTransportConfig` (client) gains
  `max_pending_requests`. Code that built either as a struct literal must
  use `Default::default()` and the `with_*` setters — both structs are also
  `#[non_exhaustive]` from this release (next entry), so adding the fields
  to the literal is not the fix. This release is the deliberate breaking
  minor before the two clean minors `STABILITY.md` §7 requires, so every such
  future-proofing change lands here rather than one per release.
- **Fifteen public configuration structs are `#[non_exhaustive]`**, so the
  next field any of them gains is an addition rather than a break. Server:
  `HandlerLimits`, `DispatchConfig`, `CorsConfig`, `GrpcConfig`,
  `CacheConfig`, `PushRetryPolicy`, `RateLimitConfig`, `TaskStoreConfig`,
  `TenantStoreConfig`, `PerTenantConfig`, `TenantLimits`. Client:
  `ClientConfig`, `RetryPolicy`, `WebSocketTransportConfig`,
  `GrpcTransportConfig`. (`ServeConfig` already was.) The wire types in
  `a2a-protocol-types` are deliberately not included: they are the spec's
  data types and a literal is their intended construction. **Migration:**
  a struct literal of any shape, `..Default::default()` included, stops
  compiling outside the crate; build the value with `Default` (or the
  documented constructor — `CorsConfig::new`/`permissive`,
  `CacheConfig::with_max_age`) and chain the `with_<field>` setter for each
  field you set. Every public field on every struct in the list now has one;
  the ones added for this are `DispatchConfig::with_require_version_header`,
  `CorsConfig::with_allow_origin`/`with_allow_methods`/`with_allow_headers`/
  `with_max_age_secs`, `RateLimitConfig::with_requests_per_window`/
  `with_window_secs`/`with_trusted_proxy_hops`/`with_max_buckets`,
  `TaskStoreConfig::with_max_capacity`/`with_task_ttl`/
  `with_eviction_interval`/`with_max_page_size`,
  `TenantStoreConfig::with_per_tenant`/`with_max_tenants`,
  `PerTenantConfig::with_default`/`with_overrides`/`with_override`,
  `TenantLimits::with_max_concurrent_tasks`/`with_executor_timeout`/
  `with_event_queue_capacity`/`with_rate_limit_rps` (and a deprecated
  `with_max_stored_tasks`, matching the field), and ten on `ClientConfig`
  (`with_preferred_bindings`, `with_accepted_output_modes`,
  `with_history_length`, `with_return_immediately`, `with_request_timeout`,
  `with_stream_connect_timeout`, `with_connection_timeout`,
  `with_max_response_size`, `with_tls`, `with_tenant`). The upgrading guide
  has a before/after. `STABILITY.md` §4 no longer carries the "still
  exhaustive as of 0.11" exception.
- **`ClientError` gains `TooManyPendingRequests { limit }`.** The enum is
  `#[non_exhaustive]`, so exhaustive matches already carried a wildcard.
- **`GrpcTransportConfig` is `#[non_exhaustive]`** (client) — one of the
  fifteen above, listed on its own because it also gained fields
  (`bare_address_scheme`, `tls_config`; under Changed below) and because
  `cargo-semver-checks` grades the attribute a major change and this heading
  is where STABILITY.md says those go.
- **`InMemoryQueueWriter` is no longer `UnwindSafe` or `RefUnwindSafe`**
  (server). It now holds the `Arc<dyn Metrics>` it reports dropped events
  through, and a trait object without those bounds removes the auto-impls.
  Only code that wrapped a writer in `catch_unwind` or `AssertUnwindSafe`
  can notice; the handler that owns the writer never had either bound.

### Added

- **`AgentCard::with_streaming(bool)`** (types): the one capability nearly
  every card sets, without a `with_capabilities(AgentCapabilities::none()
  .with_streaming(true))` of its own. With it, `AgentCard::new(name, version,
  interface)` and the `with_*` methods from 0.10 are enough to write every
  card in this repository's examples: `examples/multi-lang-team`'s
  `make_worker_card` is now eleven lines instead of a thirty-line literal,
  and **`examples/hello-agent` publishes a card** — it had none, so the
  "smallest complete agent" answered `404` at
  `/.well-known/agent-card.json` and the CLI's discovery path could not
  reach it (found building `tools/a2a-cli`). A fourth test resolves the card
  and greets the agent through a client built from it. `resolve_agent_card`
  is in `a2a_protocol_sdk::prelude` so that test, like the agent, needs no
  fully-qualified path.
- **`CardFetchOptions`, `resolve_agent_card_with_options` and
  `fetch_card_from_url_with_options`** (client, `discovery`): headers and a
  budget for the agent-card fetch. `resolve_agent_card` and
  `fetch_card_from_url` sent no headers and allowed a fixed 30 seconds, so a
  card behind authentication could not be fetched and the budget could not
  be shortened; both now delegate to the `_with_options` variant with
  `CardFetchOptions::default()`, which is those same values. Tests: a server
  that answers `401` without an `Authorization` header refuses the plain
  call and admits the options one; a server that sends headers at two thirds
  of a 600 ms budget and then stalls fails at one budget, not two. The CLI's
  discovery now carries `--header` and is bounded by `--timeout`.
- **`ClientBuilder::chosen_interface()`** (client): the card interface
  `from_card` picked — the first the caller prefers, else the card's first,
  and following `with_protocol_binding` when that moves the endpoint —
  as `Option<&AgentInterface>`, `None` for a builder from `ClientBuilder::new`
  or after switching to a binding the card does not advertise. gRPC and
  WebSocket need constructors other than `build()`, so a caller that has to
  choose one no longer re-implements the preference rule to learn which; the
  CLI's `connect.rs` had, and now reads it here.
- **`tools/a2a-cli`** (unpublished, built from the repository): `a2a
  card|send|stream|task get|cancel|list` over `a2a-protocol-client`, with
  `--binding`, `--timeout`, `--header`, `--tenant` and `--grpc-plaintext`;
  exit 0/1/2; eleven end-to-end tests against an in-process server. It
  is the first client written against the SDK from outside, and the three
  API gaps it hit are the `CardFetchOptions`, `chosen_interface` and
  `with_streaming` entries above.
- **`examples/resilient-agent`**: durability over a SQLite handler restart,
  failure injection (an executor failing its first N attempts, a webhook
  refusing its first M deliveries, a client retrying transport faults), and
  two replicas over a shared PostgreSQL store and rate-limit counter — each
  act measured and reported, with `[NOT RUN]` when the database is absent
  and `RESILIENT_REQUIRE_ALL=1` to make that exit 4. Runs in CI's
  `example-surface` job against its PostgreSQL service.
- **A Rust worker in `examples/multi-lang-team`** (`--bin rust-worker`,
  port 9104), so the example shows the worker side of the SDK next to the
  coordinator; the coordinator's demo now prints the fan-out artifact.
- **`book/src/reference/upgrading.md`**: one section per breaking boundary
  from 0.7 to 0.12 with compiling "after" snippets, and how to read a
  release's breaking section under STABILITY.md.
- **SLIMRPC binding**: `SlimRpcTransportBuilder::with_slow_consumer_timeout`
  (default 30 s) — a unicast stream whose consumer reads nothing for that
  long is abandoned with one final `ClientError::Timeout` instead of the
  process buffering the agent's frames without bound in the upstream
  per-call channel; a runnable `examples/in_process.rs`; the binding's
  tests under `coverage.yml` (measured 65.61% line coverage on
  2026-09-10); packaging verified through a config-level `[patch]` of the
  SDK pins rather than `--no-verify`; a README "Backpressure" section with
  file:line evidence for every path.
- **Four CI gates that the review backlog called for**, each proved able
  to fail by `scripts/prove_gates_fail.sh` injections:
  `check_gate_reachability.py` (every gate runs on the events that can
  break it — B13), `check_timeout_nesting.py` (an inner timeout fits the
  bound enclosing it, the push sender's schedule included — B14),
  `check_inert_bounds.py` with a reasoned allowlist (a configurable bound
  honoured by one sibling and not the others — B21), and the reverse
  direction of `check_api_reference.py` (every root-level export is on the
  API Quick Reference, which gained 123 rows).
- **`scripts/check_dco.sh`**, the DCO check as a script. `dco.yml` now runs
  it on every push to `main` as well as every pull request — the
  reachability gate's first finding was that a commit reaching `main` by
  any other route than a pull request was never graded — and
  `benchmarks.yml` runs it over the commit it pushes with `GITHUB_TOKEN`,
  which creates no workflow run. That bot identity is exempt only while the
  commit touches the two generated benchmark pages (PROVENANCE.md §3.2).
- **`HandlerLimits::push_delivery_budget`** (default 30 s): the total time
  one event's push deliveries may take across every registered config, per
  request batch on the blocking path. It was a `Duration::from_secs(30)`
  literal in two places, so the term that decided how many of an operator's
  webhooks were called was not a knob (review backlog B3).
- **`HandlerLimits::executor_drain_timeout`** (default 5 s): a blocking
  `SendMessage` whose executor returned without a terminal state, on an event
  queue that never closed, used to wait forever. It now answers with the task
  as collected — what a closed queue would have produced — and reports
  `Metrics::on_error("SendMessage", "executor_drain_timeout")`.
- **`GrpcDispatcher::with_max_connections`**: a ceiling on served gRPC
  connections, the one bound `with_http2_keepalive` and
  `with_max_connection_age` did not provide. The permit is taken before
  `accept()`, as the WebSocket dispatcher's has been, and rides with the
  socket until tonic drops it.
- **`WebSocketTransportConfig::max_pending_requests`** (client, default 64):
  the `N+1`-st request awaiting a response on one connection is refused up
  front with the new retryable `ClientError::TooManyPendingRequests` rather
  than queued invisibly in an unbounded map (B17).
- **`HttpPushSender::with_dns_timeout`** (default 5 s): the SSRF pre-flight's
  hostname lookup had no bound of its own, so a hung resolver spent the whole
  `push_delivery_timeout` before any request was made and was then reported
  as a webhook timeout. The bound is counted in `max_delivery_duration`, so
  the shipped default schedule is now 98 seconds against the 5-second
  `push_delivery_timeout`, not 93.
- **A dropped persistence event is counted.** When the background
  processor is gone, `InMemoryQueueWriter` reports the drop through
  `Metrics::on_persistence_error("queue_handoff", "channel_closed")`; until
  now the only report was a trace line that compiles to nothing without the
  `tracing` feature (B18).

### Fixed

- **A blocking caller sees the executor's error text** (server). When an
  executor returned `Err`, the `Failed` status update carried the text only
  as `metadata.error` on the streamed event; `sync_collector.rs` copies
  `status.message` onto the task and nothing else, so a blocking
  `SendMessage` returned a `Failed` task with no message and no metadata,
  and `GetTask` afterwards said no more (`examples/resilient-agent`, Act 2).
  The failure event's `status.message` is now an agent-role `Message` with
  one text part holding the error — the spec's field for "additional status
  updates for the client" — so the blocking response, the stored task and
  every `GetTask` after it carry the text; `metadata.error` stays on the
  event for streaming callers. An executor timeout reports the same way.
  Found and fixed while splitting `handler/messaging/mod.rs`'s
  `send_message_inner` into phases (`validation`, `continuation`,
  `admission`, `eviction`, `create`, `execute`), which took the file off
  `.file-length-baseline`; the split changes no error, message, metric or
  eviction order.
- **One deadline per token request and per stream start** (client).
  `OAuth2ClientCredentials::refresh` and `discover_token_endpoint` read the
  body under a second full request timeout after the headers had spent
  most of one, and the JSON-RPC and REST streaming transports read a
  failed stream start's error body under a second full
  `stream_connect_timeout`: a server that answered late and then stalled
  held the caller for close to twice the documented bound. Found by
  `scripts/check_timeout_nesting.py`; each path now takes one deadline for
  headers and body, with a regression test that fails against the old
  code (1.0 s against a 600 ms bound).
- **The blocking path reports every push outcome** (server). Push
  deliveries made for a blocking `SendMessage` reported nothing but trace
  lines, which a default build compiles away; they now report
  `delivered`, `failed`, `timeout`/`timeout_truncated` and `skipped`
  through `Metrics::on_push_delivery` exactly as the background path does.
- **DCO ran on pull requests only.** A commit reaching `main` by a direct
  push, a merge made outside a pull request, or a workflow's own push was
  never graded; `dco.yml` now runs on pushes to `main` too, and
  `benchmarks.yml` grades its own commit before pushing it (see the
  `scripts/check_dco.sh` entry above).
- **`Arc<T>` forwards every `Metrics` hook.** The blanket impl for `Arc<T>`
  forwarded six of the eight methods and defaulted `on_persistence_error` and
  `on_push_delivery` to no-ops, so a handler built with an `Arc<dyn Metrics>`
  — which `RequestHandlerBuilder::with_metrics` accepts through that impl —
  dropped every persistence error and every push outcome silently. Found by a
  test that passed against a bare implementation and failed against an `Arc`
  of the same.

- **An empty `contextId` or `taskId` on an incoming message is "unset", not
  an invalid id.** The A2A JSON bindings are ProtoJSON, and both fields are
  proto3 strings without presence, so `""` is the unset value. a2a-java's
  JSON-RPC transport prints every field (`alwaysPrintFieldsWithNoPresence`)
  and therefore sends `"contextId": ""` for none; this server answered
  `InvalidParams: context_id must not be empty or whitespace-only`, which
  failed every JSON-RPC pairing with the Java SDK in the first official ITK
  nightly (4 of 60 scenarios; the same peer passed over gRPC and HTTP+JSON,
  whose printers omit defaults). The server now maps an exactly-empty id to
  absent before validation and generates the context as for an omitted one;
  whitespace-only ids are still rejected, because no printer produces them
  for "unset". Regression test in
  `a2a-protocol-server/tests/proto3_empty_ids.rs` posts the raw JSON-RPC
  body the Java client sends.
- **The weekly mutation sweep's 55 survivors are addressed** (run
  34078555778 at `d3ebb72`, all in `a2a-protocol-server`). Twenty-six sat in
  `parse_numeric_ipv4`, merged on 2026-08-27 while the per-PR gate was
  passing over a failed baseline (the defect `db795c2` fixed); the function
  is rewritten to fill the leading octets and bound the tail, which retires
  the shift-and-or operators whose mutants were equivalent, and its packing
  is pinned by a table naming the exact address of every `inet_aton` form
  and the first value each part cannot hold. `labelled_constant_time_match`
  selects with an XOR-swap for the same reason, still branchless.
  `build_jwks_client` is one function with the feature split inside it, so
  its plaintext body is no longer a `#[cfg]` twin that `--all-features`
  compiles out. The rest are tests named for the mutant each kills: the
  tenant-slot sweep, the Postgres rate-limit counter's sweep cadence (live
  database, `#[ignore]`d like the store suite), the WebSocket keepalive sent
  once per silence, `IdleTimeout` forwarding flush and shutdown, the push
  truncation boundary, the SQLite delta payload and statement choice, and
  `rows_for_append` on a delta covering every part. Three survivors remain
  by construction — `TenantLimits::builder`, `PerTenantConfig::builder`,
  `SseBodyWriter::close` — and the ledger, `CONTRIBUTING.md`, ADR 0006 and
  `ROADMAP.md` now carry the current figures and the two false-green sweeps.

### Changed

- **`a2a-protocol-server` depends on `mutants`** (0.0.4, MIT, zero
  dependencies, attribute macros that return their input unchanged) so the
  three mutants no test can distinguish — `TenantLimits::builder`,
  `PerTenantConfig::builder`, `SseBodyWriter::close` — carry
  `#[mutants::skip]` in source with the equivalence argument as the comment,
  instead of surviving every sweep. ADR 0006 records the supply-chain
  review; the two files grade at 38 mutants, 0 missed with the sweep's own
  command line.
- **Dependencies refreshed for 0.12.** Both lockfiles updated
  (`cargo update`: 238 workspace entries, 57 in the SLIMRPC binding), and
  the majors: opentelemetry/opentelemetry_sdk/opentelemetry-otlp 0.32
  (the SDK's removed `rt-tokio` feature is the only change needed),
  tokio-tungstenite 0.30 everywhere it appears (tungstenite 0.30 rejects
  malformed `Sec-WebSocket-Key` server-side), rig-core 0.42 (the example's
  `Agent` moved out of rig-core; a thirty-line local shim keeps the example
  tool-less and behaviour-identical). genai stays at 0.5: every 0.6.x and
  the 0.7 betas depend on the unmaintained `paste` (RUSTSEC-2024-0436),
  which `cargo deny check advisories` rejects. base64 stays at 0.22
  because hyper-util still requires it and `deny.toml` denies duplicate
  versions. `deny.toml` gains seven reviewed skips for splits the ecosystem
  is mid-way through (RustCrypto digest 0.10/0.11 via tungstenite against
  sqlx, syn 2/3 via async-trait, base64 through the examples' reqwest and
  rcgen, redox_syscall through libredox) and loses the two `thiserror` skips
  that no longer matched anything.
- **MSRV lowered from 1.93 to 1.88; edition 2024.** The workspace had
  never needed anything newer than 1.88 — `cargo check --workspace
  --all-features --all-targets` on 1.88.0 passed without a code change —
  and 1.88 is the oldest toolchain the dependency tree (`time`,
  `serde_with`, `darling`) declares support for. Lowering an MSRV is not a
  breaking change. With it the crates move to edition 2024 and the
  MSRV-aware resolver (`resolver = "3"`), so `cargo update` keeps
  dependency versions compatible with the declared floor. `cargo fix
  --edition` made the mechanical changes (`$x:expr_2021` in macro
  matchers, `ref` patterns, `unsafe { set_var }` in the four build scripts,
  now carrying `SAFETY` notes); the edition's tail-expression drop-order
  change was reviewed at every site the migration lint reported and the
  full suite passes on both 1.88.0 and stable. Two places that seeded the
  environment for a reader — the OTLP pipeline test and the
  incident-response observability check — now pass the endpoint explicitly
  through the new `init_otlp_pipeline_with_endpoint`, and the rig example
  builds its client from explicit settings, so no library or example code
  writes the process environment. CI's clippy matrix now runs on stable
  only: 0.1.88's `similar_names` and `cognitive_complexity` verdicts differ
  from current clippy's, and lint verdicts are a property of the linter,
  not of the compatibility floor; the Test matrix keeps its 1.88 leg on all
  three platforms.

### Added

- **`grpc-tls` feature on `a2a-protocol-server`: TLS on the gRPC listener
  itself.** Until now the dispatcher was plaintext-only and the book said to
  terminate TLS in a proxy or mesh, which the official Go, Python and Java
  SDKs also expect — but a2a-rs serves gRPC TLS in-process, and a deployment
  without a mesh had no in-tree answer. `GrpcDispatcher::with_tls` takes a
  `tonic::transport::ServerTlsConfig` (re-exported from `dispatch::grpc`
  with `Identity` and `Certificate`, so no tonic dependency of your own):
  the server certificate and key, and for mutual TLS a client CA, with
  `client_auth_optional` to admit clients that present none. Every `serve*`
  method applies it; `into_service` still hands the bare service to a server
  the caller configures. The listener then speaks TLS only — a plaintext
  client is refused at the handshake, with no fallback. tonic builds its
  acceptor from the process-level rustls provider and panics when none is
  installed and both `ring` and `aws-lc-rs` are linked (this workspace's
  `--all-features` build is such a binary); the dispatcher installs `ring`
  when nothing is installed, which is what a single-provider build does
  implicitly, and never overrides an installed one. A rejected
  configuration (a key that does not match its certificate) is an
  `std::io::Error` from the `serve*` call, not a panic. The SDK crate's
  `grpc-tls` now enables both sides. Proved end to end in
  `a2a-protocol-sdk/tests/grpc_server_tls_e2e.rs` against the client's
  `grpc-tls` transport with `rcgen` certificates: pinned-CA round trip,
  plaintext refusal, mutual TLS admitting a CA-signed client and rejecting
  one without a certificate, optional client auth, and the
  certificate/key-mismatch error; `grpc_server_tls_provider_e2e.rs`, in its
  own binary because the provider is process state, proves the
  install-when-absent path.
- **`init_otlp_pipeline_with_endpoint`** (`a2a_protocol_server::otel`):
  `init_otlp_pipeline` with the collector endpoint given explicitly instead
  of read from `OTEL_EXPORTER_OTLP_ENDPOINT`.
- **Coverage measures the PostgreSQL stores instead of failing to exclude
  them.** The coverage workflow now runs the same live-database suites the
  `test-postgres` CI job runs (`postgres_store_tests`, `multi_replica`)
  under instrumentation against a `postgres:16` service and merges the
  profiles into one report. The seven PostgreSQL files had sat at 0-8% on
  the dashboard — 42% of every uncovered line in the repository — behind an
  ignore list that never took effect (`codecov.yml` keeps the history).
  Measured locally the same way: the PostgreSQL files at 94.55% in
  aggregate. Two live-database tests were added for the paths the
  isolation tests did not reach — the tenant-aware task store's
  `context_id`, `status` and `statusTimestampAfter` filters, cursor
  pagination, delete and count, and the push-config stores' list, delete
  and count — proving along the way that a delete under the wrong tenant is
  a no-op that leaves the other tenant's row intact.
- **Nightly run in the official ITK, with published metrics.** Until now
  the only cross-SDK evidence graded by someone other than this project was
  the official TCK; the upstream Integration Testing Kit job was
  dispatch-only and had never completed. `itk/run_itk.sh` now adopts the
  shim every SDK repository carries over a2a-itk's shared driver, and
  `.github/workflows/itk-nightly.yml` runs the shared nightly set at 02:00
  UTC with this repository mounted as the system under test against every
  peer in the ITK's `matrix.yaml` — official Python, JavaScript, Go, Java,
  and a2a-rs — over JSON-RPC, gRPC and HTTP+JSON. Results are uploaded as a
  run artifact and, from `main`, to the rolling `nightly-metrics`
  prerelease the ITK dashboard reads, the same mechanism the other SDKs
  use. The workflow also diffs the vendored `instruction.proto` against
  upstream so the agent cannot silently drift from the schema it is graded
  against. Not a PR gate. The first run reported 56 / 60 and found the
  empty-`contextId` bug fixed under "Fixed" above; the run on the final
  commit of this change reports 60 / 60.
- **Feature-matrix CI job.** `cargo hack clippy --each-feature` now lints
  every feature of each published crate on its own, plus no-default-features
  and all-features (39 combinations, 8 minutes warm), so a `#[cfg(feature)]`
  gap is caught before a downstream build enables an unusual subset. The
  hand-picked combination list stayed as it was; this is the exhaustive
  complement. `scripts/preflight.sh` picks the gate up automatically.
- **Dependabot** (`.github/dependabot.yml`): weekly grouped minor/patch bumps
  for the workspace, the SLIMRPC binding's own lockfile and GitHub Actions;
  majors arrive as separate pull requests. Its commits are exempt from the
  DCO gate by exact author identity — a version bump carries no authored
  content to certify, and the review and merge stay human — recorded in
  `PROVENANCE.md` §3.2.
- **`STABILITY.md`**: the API stability policy — what "breaking" means for
  these crates, deprecate-then-remove with a one-minor window, at most one
  breaking minor per month, `### Breaking Changes` changelog headings,
  `cargo-semver-checks` as the proof, the MSRV rule, what is and is not
  covered, and the criteria for `1.0`. Linked from the README's Stability
  section.
- **crates.io Trusted Publishing.** The release workflow's publish job now
  exchanges its GitHub OIDC token for a short-lived crates.io token
  (`rust-lang/crates-io-auth-action`), falling back to the environment
  secret with a warning until every crate has a trusted publisher
  configured, and failing if neither credential exists. The one-time
  crates.io setup is in `RELEASING.md`.

- **`grpc-tls` feature (client, forwarded by the SDK crate): gRPC over TLS.**
  `grpc` alone never had a TLS connector — tonic's is a feature, and none was
  enabled — so an `https://` gRPC endpoint opened a plaintext HTTP/2 stream to a
  TLS port and failed on the handshake with an error that read like a network
  fault. With `grpc-tls`, `https://` endpoints are verified against the same
  bundled Mozilla roots (`webpki-roots`, ring) the HTTP transports use, or
  against a `tonic::transport::ClientTlsConfig` supplied through the new
  `GrpcTransportConfig::with_tls_config` for a private CA or client
  certificate. Without the feature an `https://` endpoint is now refused up
  front with a message naming it, instead of failing after the fact. No new
  crate versions: tonic's `tls-ring` and `tls-webpki-roots` resolve to the
  `rustls 0.23` / `tokio-rustls 0.26` / `webpki-roots 1.0` already in the
  tree. Proved in `a2a-protocol-sdk/tests/grpc_address_e2e.rs` against a
  tonic TLS listener presenting an `rcgen` certificate: a pinned CA round-trips
  over a bare target and over `https://`, the bundled roots reject that CA,
  and TLS against a plaintext listener does not fall back. The connector
  never touches the process-level rustls crypto provider: tonic under
  `tls-ring` uses an installed default or else `ring` explicitly, so a binary
  that links both `ring` and `aws-lc-rs` (this workspace's `--all-features`
  build is one) gets a connection error, not a panic, and an application that
  wants `aws-lc-rs` installs it itself — `grpc_tls_provider_e2e.rs`, in its
  own test binary because the provider is process state, proves both. The
  feature does not pull in `tls-rustls`; a gRPC-only client gets no
  hyper-rustls. The tonic types a caller needs (`ClientTlsConfig`,
  `Certificate`, `Identity`) are re-exported from `transport::grpc`, so no
  direct tonic dependency is required, and `ClientBuilder::with_grpc_tls_config`
  carries a pinned CA or client certificate through `from_card(..).build_grpc()`.
  A bare target that the policy dialled with TLS and that refused the
  handshake fails with the policy named and `GrpcBareAddressScheme::Http`
  suggested, since a plaintext peer on a private network is the usual cause.
- **`GrpcBareAddressScheme`** (`a2a-protocol-client`), with
  `GrpcTransportConfig::with_bare_address_scheme` and
  `ClientBuilder::with_grpc_bare_address_scheme`: how a bare `host:port` gRPC
  target is dialled. `HttpsExceptLoopback` (default) uses TLS for every host
  except `localhost` / `127.0.0.0/8` / `::1`; `Https` and `Http` force one
  scheme. See the fix below for why a bare target is accepted at all.
- **§7.6.4 conformance test** (`auth_required_state_tests.rs`). Upstream
  added *In-Task Authorization Scope* on 2026-07-30 (`6550d34`): the
  `TASK_STATE_AUTH_REQUIRED` transition "MUST NOT" be treated as authorization
  for anything. This server has always satisfied that by construction — the
  interceptor chain runs before every method and nothing reads the task's
  state for an authentication decision — and the test pins it: a continuation
  of an `AUTH_REQUIRED` task with no credential is refused on the
  interceptor's terms, runs no executor, and leaves the task's history
  untouched, while the same continuation with the credential is processed.
  `SPEC_COMPLIANCE.md` carries the row.

### Changed

- **`GrpcTransportConfig` is `#[non_exhaustive]`** (breaking for code that
  built it as a struct literal; `GrpcTransportConfig::default()` plus the
  `with_*` setters covers every field). It gained `bare_address_scheme` and,
  under `grpc-tls`, `tls_config`, and a public-fields config struct that grows
  by a field on every such change would break its callers each time. No code
  in this repository, its examples or its bindings constructed it literally.
- **`GrpcTransport::connect` rejects a non-target address by name.** An
  address with a scheme other than `http`/`https`, a path, a query, or userinfo
  is an `InvalidEndpoint` that quotes the input (`grpc://…`, `agent:50051/a2a`,
  `user:pw@agent:50051`), where before anything without `http(s)://` was
  refused with the same one-line message.

- **Official TCK: two more `a2a-tck` checks baselined, same stale-specification
  cause as the first two.** The nightly of 2026-09-01
  ([33467431712](https://github.com/tomtom215/a2a-rust/actions/runs/33467431712))
  went red on `CORE-CANCEL-002` [`http_json`] and `STREAM-SUB-003` [`grpc`]
  while the nightly 22 hours earlier had passed **on the identical commit**
  (`b6f3afb`). `a2a-tck` landed
  [`de6af18`](https://github.com/a2aproject/a2a-tck/commit/de6af188d2d65779719c88d4f6bb5b180a4fa91d)
  (PR #207) in between, which replaced "any error is acceptable" assertions
  with assertions on the error each requirement names — a good change that
  reached two rows §5.4's table has stale in the copy the suite vendors:
  `TaskNotCancelableError` (`409` there, `400` current) and
  `UnsupportedOperationError` (`UNIMPLEMENTED` there, `FAILED_PRECONDITION`
  current). Both requirements pass on every binding where the two copies agree
  and fail only on the one where they do not, which is the shape of a stale
  table rather than of a defect here. No SDK behaviour changed;
  `tck/conformance-baseline.json` now carries four entries and
  `docs/official-tck-findings.md` §21 records the evidence.
- **Corrected: the specification divergence is a pinned release, not an
  in-place amendment.** 0.11.0's changelog entry and §20 both said upstream "amended the
  document in place under the same `1.0.0` version string". It did not. A2A
  released the change as **v1.0.1** (2026-05-28, commit `757f0ec`, PR #1627);
  the `v1.0.0` tag still carries the old table, and `a2a-tck`'s vendored copy is
  byte-identical to it — its own `specification/version.json` names that tag and
  a 2026-03-13 download. The earlier reading came from a reproduction that
  compares a vendored file against `main`, through which a tagged patch release
  and a silent rewrite look the same. Every conclusion drawn from the divergence
  is unaffected; the mechanism, and so the right thing to ask upstream for, is
  not. Full timeline and commands in `docs/official-tck-findings.md` §21.1.
- **Reported upstream as
  [a2aproject/a2a-tck#231](https://github.com/a2aproject/a2a-tck/issues/231).**
  §20 had left "reporting it upstream is the obvious next step" undone for two
  days; it is done, and it was worth the wait, because §21.1's correction
  changed what there was to report — "your vendored copy is a superseded release
  and `make spec` moves it" is a different request from "upstream rewrote a
  published document". The submitted body, the duplicate check and the limits of
  that check are kept in `docs/upstream/a2a-tck-231-spec-pin-report.md`.
- **The root cause is reported to the specification as
  [a2aproject/A2A#2200](https://github.com/a2aproject/A2A/issues/2200).** §3.6
  promises patch releases do not affect protocol compatibility; `v1.0.1` changed
  six wire-observable §5.4 rows. Reading `757f0ec` and `v1.0.1`'s release notes
  before filing narrowed the claim and improved it: the change was announced and
  was made deliberately, to align the HTTP codes with `google.rpc.Code`, so the
  issue asks how §3.6 is to be reconciled and how an implementer is meant to
  notice — not for a revert. Record, including what it deliberately does not
  claim, in `docs/upstream/a2a-2200-patch-versioning-report.md`.
- **SLIMRPC branch-spec triage: `slimrpc-broadcast-live.md`.** Upstream moved
  `feat/slimrpc-collaborative-channel` at `0c38776` (2026-09-03), replacing the
  spec that branch had been triaged against, so
  `scripts/check_slimrpc_spec.sh` — which surveys every upstream branch on every
  run — began failing as designed. The new spec is not followed by this binding,
  and that is a fact rather than a preference: its own §3 requires A2A 1.1 and
  the `SendLiveMessage` method, and neither exists in any released A2A
  specification. `a2aproject/A2A` is tagged `v1.0.1`, `SendLiveMessage` appears
  nowhere in its docs, and this SDK implements the ratified 11-method v1.0
  surface that `check_method_denominator.py` holds it to. Re-triage if A2A 1.1
  ships.
- **Official TCK: the `--deselect` on the minimal-capability profile is gone.**
  [a2aproject/a2a-tck#225](https://github.com/a2aproject/a2a-tck/issues/225),
  filed from this repository, was fixed upstream on 2026-08-31 by PR #226. The
  minimal profile now grades the same 66 MUST requirements with nothing
  excluded, measured with and without the flag.

### Fixed

- **gRPC client: a card's `host:port` gRPC address is accepted.** Since A2A
  `cfc9d34` (2026-07-21) the proto's `AgentInterface.url` comment says a gRPC
  interface's address "should be in the format `hostname:port`", example
  `grpc.example.com:443` — a gRPC target, which carries no scheme. That is what
  every official SDK's agent advertises, and what this repository's own
  conformance SUT advertises (`grpc.insecure_channel("http://…")` fails on the
  Python side). `GrpcTransport::connect` refused anything that did not start
  with `http://` or `https://`, so `ClientBuilder::from_card(..).build_grpc()`
  could not reach a spec-format gRPC agent at all; the examples worked only
  because each one prepends `http://` by hand. A bare target is now normalised
  per `GrpcBareAddressScheme` — the same decision the official SDKs leave to
  a channel factory (Python) or make as plaintext-unless-configured (the A2A
  project's own Rust SDK); this one defaults to TLS and exempts loopback, and
  says so. Proved in `grpc_address_e2e.rs`: `from_card` on `127.0.0.1:{port}`
  and on `localhost:{port}` builds and talks; the `Http` policy dials a
  non-loopback spelling in plaintext; without `grpc-tls` a non-loopback target
  is refused with the feature named rather than downgraded to plaintext.
- **Examples and the ITK agent advertise spec-format gRPC targets.** The
  `agent-team` example's gRPC card said `http://127.0.0.1:{port}`, and its
  gRPC tests bypassed the card with a hand-built transport; the ITK current
  agent's card said the same and its peer client prepended `http://` to any
  scheme-less address it met — a workaround for the limitation fixed above,
  written on the assumption that scheme-less was the peer's quirk rather than
  the specification's form. Both cards now advertise `127.0.0.1:{port}`, test
  56 reaches the gRPC agent through `ClientBuilder::from_card(..).build_grpc()`,
  and the ITK workaround is gone; the in-repo ITK traversal self-test (one-hop
  GRPC, two-hop JSONRPC→GRPC) passes against the rebuilt agent.
- **Book: `tls-rustls` was documented as off by default for the client and
  the SDK crate.** It has been on by default since the feature existed
  (`default = ["tls-rustls"]` in both manifests; the client's own README said
  so). `reference/configuration.md` now agrees with the manifests.
- **Client: the Agent Card's `tenant` now rides on every request, not only on
  `SendMessage`.** A2A §8.3.2 rule 4 says a client **MUST** "set the `tenant`
  field in every request message to exactly the value declared in the selected
  `AgentInterface` entry". `ClientBuilder::from_card` has carried that value in
  `ClientConfig::tenant` since the multi-tenancy work, and the book said it was
  "applied to all requests" — but only `send_message` read it. `GetTask`,
  `ListTasks`, `CancelTask`, `SubscribeToTask`, the four push-config methods
  and `GetExtendedAgentCard` all went out with no tenant, so a task created
  under `acme` was then looked up under the default partition and answered
  `TaskNotFound`, and against a server that resolves the tenant itself every
  follow-up call was a cross-tenant request. Found by diffing the client's
  eleven methods against the rule while re-reading the specification at
  upstream `main` (`f63dbb4`), not by a report. A per-request `tenant` still
  wins over the client default, and an absent tenant still leaves the field
  out (the parameterless `GetExtendedAgentCard` stays `null` on the wire).
  Proved three ways: a capturing transport asserts the field on each of the
  nine methods by name; `tests/tenant_round_trip_tests.rs` drives a client
  built from a tenant-bearing card through send → get → list → cancel →
  push-config create/get/list/delete against a real server with
  tenant-partitioned stores over both JSON-RPC and REST; and the same test's
  control shows the task is `TaskNotFound` to an untenanted client, so the
  suite cannot pass against a server that ignores tenants.
- **REST binding: the tenant travels as the `/{tenant}/…` path prefix on the
  client, and the server accepts every place the proto lets it arrive.**
  `a2a.proto` binds each method twice — a primary pattern with no tenant in
  the path and an `additional_bindings` pattern with it as the leading segment.
  The reference SDKs' REST clients send the prefix form (`a2a-python` 1.1.4
  `_get_path`), and the reference server reads *only* that form. Ours did the
  reverse on the client (tenant as `?tenant=` on GET/DELETE and in the body on
  POST — the primary-pattern transcoding of §11.5) while the server, like the
  reference, read only the prefix, so the two halves of this SDK disagreed
  with each other and every tenanted GET lost its tenant. Now the client emits
  `/{tenant}/tasks/{id}` (percent-encoding the segment, keeping the field in
  POST bodies exactly as the Python SDK does), and the server honours, in
  this order, the path prefix, then `?tenant=` on GET/DELETE, then the body
  on POST — including `POST /tasks/{id}:cancel`, whose `body: "*"` was never
  read at all, dropping `metadata` too. The path tenant is percent-decoded
  before it names a partition and is injected into POST bodies that omit it
  (`/acme/message:send` with no `tenant` in the body lands in `acme`). Seven
  server tests in `tests/rest_tenant_binding_tests.rs` pin each arrival path
  and the precedence, each with a `TaskNotFound` control.
- **Official TCK: one failure no longer reports as three.** The
  minimal-capability and required-extension runs inherited the default
  `success()` condition, so a red gate skipped them — while their own gates
  carried `always()` and ran anyway, dying on reports that were never written
  (`error: TCK report not found`). Both profiles are now measured whenever the
  SUT built, and each gate runs unless its own run step was skipped, so a
  nightly reports every profile's drift in one run instead of one per night.
- **`sustained_load_tests` was intermittent, and the tolerance was hiding it.**
  Both leak tests sampled their late probe the instant the load loop exited.
  The loop awaits each request, but the work it starts finishes asynchronously —
  event queues and cancellation tokens are released when a *task* completes, not
  when its request returns — so the last few in-flight tasks were counted as
  growth. Both allowed `early + 1`, a tolerance with no derivation behind it, and
  on 2026-09-04 `cancellation_tokens_do_not_accumulate_under_sustained_load` read
  `0 -> 2` and failed on CI while the identical commit passed in the sibling run
  and in five consecutive local runs. The late probe now polls until two reads
  agree, bounded by a five-second timeout. This costs no detection power, which
  is why it is the fix rather than a wider tolerance: a leaked entry is never
  released, so the count a real leak settles on *is* the leaked count. Measured
  after the change — the two leak probes read `0 -> 0` where one had read
  `0 -> 2`, and the capacity test's real numbers (`50 -> 200`, against a ceiling
  of 200) are unchanged.
- **`clippy::redundant_clone` on nightly.** `eviction/fixtures.rs` cloned a
  `TaskId` into a map insert and dropped the original unused. Caught by the
  `Nightly (informational)` canary, which floats with the nightly toolchain
  precisely so a lint reaches this repository before it reaches stable and
  blocks everything.

## [0.11.0] - 2026-08-30

### Added

- **`WEBSOCKET_BINDING_URI`** (`a2a-protocol-types`) —
  `https://a2a-rust.com/bindings/websocket/v1`, this project's identifier for
  its WebSocket binding. §5.8 says a custom binding (§12) **SHOULD** be
  identified by a URI rather than a bare name, so that two projects cannot
  define incompatible bindings under the same word; `"WEBSOCKET"` is exactly
  the collision that rule exists to prevent. `AgentInterface::protocol_binding`
  is a free-form string the caller supplies and the crates never match on, so
  nothing breaks by adopting it — but a card advertising the bare name should
  move, and readers should accept both for as long as cards in the wild carry
  the old spelling. The conformance SUT now advertises the URI while the
  examples keep `"WEBSOCKET"`, so CI exercises both paths rather than
  asserting the migration is complete.

  This repository's other custom binding was already compliant:
  `a2a-protocol-slimrpc` advertises upstream's
  `https://a2a-protocol.org/bindings/experimental-slimrpc/v1`. WebSocket was
  the outlier, which is the sort of thing a spec diff surfaces and an
  inventory of your own bindings would have surfaced sooner.

### Changed

- **BREAKING (wire format): six §5.4 error mappings corrected.** A server on
  0.11 answers some errors with a different HTTP status and gRPC code than one
  on 0.10. Anything asserting the old values — a client matching on status, a
  test, a gateway rule — needs updating.

  | A2A error | HTTP was → is | gRPC was → is |
  |---|---|---|
  | `TaskNotCancelableError` | `409` → **`400`** | — |
  | `ContentTypeNotSupportedError` | `415` → **`400`** | — |
  | `InvalidAgentResponseError` | `502` → **`500`** | — |
  | `PushNotificationNotSupportedError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |
  | `UnsupportedOperationError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |
  | `VersionNotSupportedError` | — | `UNIMPLEMENTED` → **`FAILED_PRECONDITION`** |

  `ErrorCode::http_status()` and `ErrorCode::grpc_status()` keep their
  signatures and return the new values, so this is a behavioural break that
  `cargo-semver-checks` cannot see: it compares API surface, and no surface
  changed. It is classified here by hand, which is the only way this class of
  change gets classified.

  Why the old values were wrong: `docs/implementation/v1.0.0-specification-complete.md`
  was a snapshot of the specification taken 2026-03-31 and never refreshed.
  Upstream amended the document in place under the same `1.0.0` version string,
  so the vendored copy and the published spec disagreed in six of §5.4's nine
  rows while both claimed to be v1.0.0. The SDK implemented the stale table and
  the TCK graded against it, so they agreed with each other and failed
  conformant third-party agents — the symmetric misreading the TCK's own README
  warns about. Found by running the kit against an agent built on the official
  Python SDK (`a2a-sdk` 1.1.2), which answered `400` where the kit demanded
  `409`; the SDK was right.

  **If you grade against the official `a2aproject/a2a-tck`, expect two MUST
  failures** — `HTTP_JSON-STATUS-001` and `GRPC-ERR-002`. They are not defects
  in your agent. That suite vendors its own copy of the specification and it
  carries the same stale table, disagreeing with the published document in the
  same six rows. Both entries are baselined here with the evidence and a
  two-command reproduction in `docs/official-tck-findings.md` §20, and they
  clear when the suite refreshes its copy.

- **BREAKING (wire format): the Axum adapter now agrees with the REST
  dispatcher.** It carried a second, hand-written copy of §5.4's table which had
  drifted from the shared one in three further places: `TaskNotCancelable` and
  `InvalidStateTransition` answered `409` and `PushNotSupported` answered `501`,
  where the table says `400` for all three. The duplicate is deleted and the
  adapter defers to `ErrorCode::http_status()`; a test now fails if the two
  disagree again. `PayloadTooLarge` (`413`) and `Overloaded` (`503`) remain
  adapter-specific, because A2A has no error code for either.

- **BREAKING (wire format): push notifications are delivered as
  `application/a2a+json`.** §4.3.3 specifies the A2A media type for the
  webhook `POST`; the sender was using `application/json`. A receiver that
  matches the header exactly — a framework body parser keyed on
  `application/json`, a gateway content-type rule, a WAF — will stop accepting
  deliveries until it also accepts `application/a2a+json`. The body is
  unchanged, and `+json` structured-suffix parsers already handle it.

### Fixed

- **The vendored specification is current again.** Re-vendored from upstream
  with a provenance header naming its source, retrieval date, and the one-line
  command to refresh it.

  The refreshed document carries four normative changes beyond §5.4's table,
  and 0.11.0 absorbs all four. Two were already implemented and needed only to
  be confirmed against code rather than against the diff: `PushNotificationConfig`
  is already the flat `TaskPushNotificationConfig` in both `proto/a2a_v1/a2a.proto`
  and `a2a-protocol-types`, and `AuthenticationInfo` already carries `scheme` +
  `credentials` rather than `schemes[]` + `token`. The other two ship here — the
  push-webhook `Content-Type` and §5.8's URI-identified bindings, both above.

  Worth recording that the first reading of this diff reported all four as
  unimplemented. A specification diff says what the document changed, not what
  the code does; two of the four had been implemented ahead of the vendored
  snapshot, which is invisible from the diff alone.

- **The TCK no longer fails conformant agents on `application/a2a+json`.** The
  `a2a_media_type_accepted` check ran against JSON-RPC, where §9 specifies
  `Content-Type: application/json` and §14.1.1 scopes the A2A media type to
  §11's REST binding. It is REST-only now. Two official SDKs (`@a2a-js/sdk`,
  `a2a-java`) were carried in the ITK's `--skip` list as divergent for as long
  as the check was mis-scoped; both were conformant.

- **The TCK's availability classifier no longer reads a refusal as an
  absence.** `-32004` (`UnsupportedOperationError`) counted as "this binding
  does not offer the method", which is what the stale §5.4 table implied by
  mapping it to gRPC `UNIMPLEMENTED`. Under the corrected table it is
  `FAILED_PRECONDITION` — served, and refused — so only `-32601` (JSON-RPC
  *Method not found*) now means absent, matching REST's `404`/`501` and gRPC's
  `UNIMPLEMENTED`. This was fabricating `BIND-EQUIV-001` violations against
  agents that offer an operation on both bindings and decline it on both.

### Internal

Not shipped in the published crates; recorded because the conformance claims
rest on them.

- **Every third-party SDK pin was behind upstream**, and all three moved:
  `@a2a-js/sdk` 1.0.0 → 1.1.0, `a2a-go/v2` v2.3.1 → v2.5.0, `a2a-java`
  1.0.0.CR1 → 1.3.0.Final. Three of the four entries in the ITK's divergence
  table went with them; one genuine divergence remains (`a2a-java` rejecting
  `application/a2a+json` on REST, re-verified at 1.3.0.Final).

- **`itk/agents/java-sdk` produces a permissive `TaskAuthorizationProvider`.**
  a2a-java 1.3.0.Final enforces a fail-closed default for task authorization,
  so an agent with no provider answers `TaskNotFoundError` to every task read —
  a denied read reported as "not found" rather than leaking that the task
  exists. Granting explicitly keeps the authorization path executing, where
  `a2a.authorization.required=false` would switch it off.

- **`pin-freshness.yml`** re-resolves every SDK pin on a three-week cadence and
  re-grades against the current skip list. Every mechanism needed to catch the
  stale pins already existed — the kit exits 1 on a skipped test that passes —
  and never fired, because nothing moved the pins.

## [0.10.0] - 2026-08-27

### Deprecated

- **`TenantLimits::max_stored_tasks`** (`a2a-protocol-server`). It named a cap
  on stored tasks and nothing ever read it, for a structural reason: it sits on
  `PerTenantConfig`, which the handler holds, and a store is constructed
  independently and handed to the builder — a store never sees one. The working
  equivalent is the new `TenantAwareInMemoryTaskStore::with_tenant_override`,
  which gives a named tenant its own `TaskStoreConfig` and so its own
  `max_capacity`.

  Deprecated rather than removed: removing a public field is a semver break, and
  this crate's version is bumped as step 1 of a release (`RELEASING.md`), not
  mid-branch. The deprecation carries the whole point anyway — every use site
  now gets a compiler warning naming the replacement, which is louder than the
  silence the field had before.

### Added

- **`TenantAwareInMemoryTaskStore::with_tenant_override`** — gives a named
  tenant its own `TaskStoreConfig`, so `max_capacity`, `task_ttl`,
  `eviction_interval` and `max_page_size` can all differ by tenant. It lives on
  the store rather than on `TenantStoreConfig` because that config is
  exhaustively constructible through its public fields, so adding one would
  break every downstream struct literal.


- **Task retention for the persistent stores.** `purge_expired` on
  `SqliteTaskStore`, `PostgresTaskStore` and their tenant-aware variants deletes
  terminal tasks older than a [`RetentionPolicy`], in batches, and returns a
  `PurgeReport` describing what it did. **Nothing is deleted unless you call
  it.** The in-memory store has always forgotten tasks after an hour and the
  persistent stores kept them forever; the divergence between the two was the
  defect, not the growth, and neither behaviour was written down. Growth is
  modest either way — measured at 826 B/row on PostgreSQL and 781 B/row on
  SQLite, so under a GiB per million tasks. Retention is deliberately not wired
  to a timer inside the store: a sweep that fires on its own fires during your
  traffic peak, and the store does not know when that is. Call it from whatever
  already schedules work.

- **Deployment-wide rate limiting.** `RateLimitInterceptor::with_shared_counter`
  accepts any `RateLimitCounter`, so N replicas enforce one limit rather than N
  copies of it. `PostgresRateLimitCounter` is the bundled implementation.

- **A server that can be stopped without cutting live calls.** `Server`,
  `ServeConfig` and `serve_with_shutdown` bind a listener, serve until a
  shutdown signal, and drain in-flight requests within a deadline, returning a
  `ServeReport`.

- **Connection-level timeouts**, so a slowloris costs the attacker something:
  `ServeConfig` bounds header-read time, idle time and shutdown drain
  (`with_header_read_timeout`, `with_idle_timeout`, `with_drain_timeout`) and
  caps concurrent connections (`with_max_connections`).

- **`set_caller_identity` / `with_caller_identity`** on the rate-limit
  interceptor, so a per-caller limit is keyed by the caller.

- **`TaskState::ALL`** — the nine protocol states as an array, for exhaustive
  iteration over a `#[non_exhaustive]` enum.

- **Constructors for `AgentCard`, `AgentInterface` and `AgentSkill`.** Building
  a card required a struct literal naming all fifteen fields — no `new`, no
  builder, no `Default` — which is the first thing anyone has to do with this
  SDK. `AgentCard::new(name, version, interface)` takes exactly what
  `AgentCard::validate` requires, so a constructed card is valid by
  construction, and the rest is chained `with_*` in the style
  `AgentCapabilities` already used. `AgentInterface::jsonrpc/grpc/rest` spell
  the spec's own binding names and default `protocol_version` to
  `A2A_VERSION`. This repository contained 122 `AgentCard { .. }` literals when
  the constructors were written.

- **Connection bounds for `GrpcDispatcher`:** `with_http2_keepalive` and
  `with_max_connection_age`, both opt-in. `GrpcConfig` bounded message size and
  per-connection concurrency and nothing bounded a connection that simply
  exists: measured, 400 TCP connections opened against the dispatcher and left
  silent were all accepted, none refused, and the oldest was still alive twelve
  seconds later. HTTP/2 keepalive is the gRPC-native answer and distinguishes a
  peer that has stopped answering from one that is merely quiet, because a
  conformant client's HTTP/2 stack answers a PING without the application being
  involved — so a streaming RPC waiting for its next event is left alone. They
  live on the dispatcher rather than on `GrpcConfig` because that type has
  public fields and no `#[non_exhaustive]`, so adding a field to it would break
  every struct-literal construction.

- **Connection bounds for `WebSocketDispatcher`:** `with_max_connections` and
  `with_idle_timeout`. The handshake timeout — documented in that module as the
  slowloris defence — covers only the part before the upgrade completes;
  nothing bounded a peer that completed the handshake and then went quiet.
  Measured: 400 idle handshaken connections accepted and held with none
  refused, and a connection idle for 12 seconds still being served, because the
  read loop awaited the next frame with no bound and the accept loop spawned a
  task per socket with no ceiling. Both default to **off**, so nothing changes
  unless you ask. `max_connections` takes its permit before `accept()`, so
  excess load waits in the kernel's listen backlog rather than as unbounded
  tasks. `idle_timeout` counts traffic in *both* directions and sends a
  WebSocket Ping at the halfway mark, which conformant clients answer
  automatically — so it closes peers that are unresponsive rather than peers
  that are merely quiet. That distinction is why it is opt-in and why it is
  safe to opt in: an A2A subscription may legitimately be silent for hours.

- **`push_outcome::SKIPPED`**, reported once per push-notification config the
  per-event delivery budget never reaches.

- **A CI gate for the cancellation class:**
  `scripts/check_cancellation_release.py`. A file that claims a guard slot with
  `compare_exchange(false, true, ..)` must contain an `impl Drop` that releases
  one. Three of this release's defects were that shape — state claimed on one
  path and released on another, with an `.await` in between, so dropping the
  future runs neither — and all three were found by a person reading code.
  `clippy::await_holding_lock` covers the inverse case; nothing covered this
  one.

- **A reusable over-the-wire adversarial probe suite** (`scripts/adversarial/`),
  the third leg of this project's negative-input testing beside the
  unit/property tests and the libFuzzer harnesses. Seven black-box probes — one
  per request binding (JSON-RPC, WebSocket, gRPC), plus authentication,
  sustained concurrency, SSE streaming and outbound push delivery — send
  malformed, hostile and edge-case requests to a *running* server backed by a
  real model and, after every request, re-probe the agent card to confirm the
  process stayed up. They are standard-library only (the gRPC leg compiles the
  repo proto at startup) and exit `0`/`1`, so they double as CI gates.
  Documented in the book under **Testing & Deployment → Adversarial Testing**,
  with the full account of the three defects they found (see Fixed and Security,
  below). The push probe counts how many times the server actually dialed each
  hostile receiver, so a run that exercises nothing fails loudly rather than
  passing vacuously — the trap a wrong push-config field name
  (`pushNotificationConfig` for the correct `taskPushNotificationConfig`) had
  already sprung once.

### Fixed

- **The gRPC binding processed a request with no `a2a-version`.** JSON-RPC and
  WebSocket reject an absent protocol version — spec §3.6.2 reads absent as
  protocol 0.3, and §737 requires `VersionNotSupported` for a version this
  server does not speak, so a 1.x-only server must refuse it — and the gRPC
  binding did not, a cross-binding inconsistency only visible because the
  adversarial probe hits every binding with the same attack. `validated_metadata`
  ran its version check only when the value was present, so absent fell through
  to accepted, under a docstring that claimed it "mirror[ed]" the other bindings
  — which reject it. It now delegates to the shared `validate_version_metadata`
  the other three use, so all four agree. `GrpcConfig` gains
  `require_version_header` (default true, `with_require_version_header` to relax
  it), matching `DispatchConfig`. Verified over the wire: a versionless gRPC call
  now returns the same `VersionNotSupported` the other bindings emit, and the
  client-driven surface sweep stays 44/44, because the client injects the
  version.

- **Non-streaming `SendMessage` delivered webhooks on the request path.** The
  synchronous collector awaited push-notification delivery inline, so a webhook
  the caller registered on that same request delayed the response by up to the
  30-second delivery budget — measured 15 seconds against a hanging webhook for a
  three-event task. Streaming and `returnImmediately=true` were already
  unaffected, and it did not starve other clients (the blocked work is an async
  task, not a thread), but it coupled a caller's latency to a client-controlled
  endpoint and held task-store slots for up to 30 seconds, for a
  fire-and-forget notification whose errors are swallowed anyway — the webhook
  anti-pattern, and inconsistent with the streaming path, which already delivers
  off-path. Delivery is now buffered during collection and handed to a single
  spawned task that sends the events in order under the same budget and
  per-delivery timeout, with the tenant context carried across the spawn exactly
  as the streaming background processor does. Verified over the wire:
  `SendMessage` against a hanging webhook returns in 0.01 s (was 15 s) and the
  webhook is still dialed in the background. A regression test asserts the
  collector returns in under a second even against a sender that would sleep an
  hour.

- **A push-backoff test failed on runner speed rather than on behaviour**
  (`a2a-protocol-server`). `backoff_is_paid_between_attempts_but_not_after_the_last`
  asserted total elapsed in `[1800ms, 2800ms)` around three real loopback round
  trips, so its 800ms of headroom above the correct 2000ms had to absorb every
  scrap of per-request overhead. Measured on a Windows CI runner: **2954ms for
  a correct run**, failing by 154ms.

  It now asserts the gaps *between* request arrivals and the gap from the last
  arrival to `send()` returning. Each spans one round trip instead of three, so
  overhead cannot accumulate into the margin — and the assertions say what the
  test is named for: the second gap must differ from the first (1500ms, not
  500ms), and nothing beyond a response round trip may follow the final
  attempt. Its mutation coverage is unchanged and its messages are sharper: a
  trailing backoff now reports "1.501256836s elapsed between the last request
  arriving and send() returning" instead of a total that could mean anything.

- **`PerTenantConfig` now enforces the limits it declares** (B22). All five
  `TenantLimits` fields were stored, resolvable, and read by nothing in the
  request path, under a module header that advised operators to set
  `max_concurrent_tasks` for noisy-neighbour isolation. Four are now enforced
  and the fifth is deprecated in favour of a store-level equivalent (see
  Deprecated, above):

  | Limit | Where | On exceeding |
  |---|---|---|
  | `max_concurrent_tasks` | per-tenant semaphore; permit taken before any side effect and held for the executor's life | `ServerError::Overloaded` |
  | `executor_timeout` | resolved before the executor is spawned | the task fails |
  | `event_queue_capacity` | at queue creation | the stream buffer is that deep |
  | `rate_limit_rps` | `RateLimitInterceptor`, via the new `with_tenant_config` | the request is refused |

  The permit is taken *before* the queue, the task row and the cancellation
  token exist, so a refused request leaves nothing behind — rejecting later
  would make the limit cost the tenant the resources it protects. A tenant at
  its limit is refused rather than queued: queueing converts a declared bound
  into unbounded latency and unbounded memory.

  Three of the four the handler applies on its own. `rate_limit_rps` is opt-in
  and needs the same config handed to
  `RateLimitInterceptor::with_tenant_config`, because a request is counted in
  an interceptor rather than in the handler. It counts against a bucket keyed
  by tenant *in addition to* the per-caller bucket, so a tenant's allowance is
  not multiplied by its number of callers — and the unit is converted rather
  than reinterpreted: the per-window allowance is `rate_limit_rps ×
  window_secs`, because the field is documented in requests per second and
  `requests_per_window` is not.

  What made this enforceable at all was measuring where the tenant is visible.
  `TenantContext` is a `tokio::task_local!`, and a probe recording it at two
  points of one `SendMessage` saw `"acme"` in the interceptor chain and `""` in
  the spawned executor. So an interceptor can resolve a tenant, and anything
  the executor needs must be resolved before the spawn and moved in — which is
  where the timeout and the permit are now taken.

- **The WebSocket client's connect had no deadline.** `connect_async_with_config`
  was awaited bare, so `WebSocketTransport::connect*` could wait indefinitely on
  a server that completed the TCP handshake and then never answered the HTTP
  upgrade — the connection is established, so no OS timeout applies. Measured
  against a listener that accepts and holds the socket open in silence: still
  pending at 3 seconds, with no deadline to reach. Every sibling transport
  already bounded this (JSON-RPC and REST via `ClientConfig::connection_timeout`,
  gRPC via `GrpcTransportConfig::connect_timeout`), and
  `connection_timeout`'s own documentation names the hazard. `WebSocketTransportConfig`
  now carries `connect_timeout`, defaulting to the same 10 seconds and applied
  around the whole handshake — TCP, TLS, and upgrade. Reverting the fix does not
  fail the new test; it hangs the suite, killed at 120 s having never returned.

- **`ClientConfig::preferred_bindings` selected nothing.** The field documented
  an ordered client preference — *"the client tries each in order, selecting the
  first one supported by the target agent's card"* — and no code read it.
  `ClientBuilder::from_card` took `supported_interfaces.first()`, which is the
  **agent's** first choice, the inverse of the preference the field describes;
  the builder consulted a different, singular field with almost the same name.
  A caller who ranked `GRPC` and met a card advertising `[JSONRPC, GRPC]`
  silently got JSONRPC. `from_card` now honours the default preference list, and
  the new `ClientBuilder::from_card_preferring(card, preferences)` takes an
  explicit one. Matching is ASCII-case-insensitive, because the spec's binding
  names are upper-case and cards written by hand are not always. The resulting
  config records the preference that was applied rather than the default.

- **`with_protocol_binding` moved the binding and left the endpoint behind.** An
  agent card gives each binding its own URL, so the two are a pair. Measured
  against a card advertising JSONRPC at `:1111` and GRPC at `:2222`,
  `from_card(card).with_protocol_binding("GRPC")` produced binding `GRPC` at
  endpoint `:1111` — a client that would speak gRPC to the JSON-RPC port, with
  no error anywhere. A builder created from a card now retains that card's
  interfaces and moves the endpoint and tenant with the binding. Builders made
  with `ClientBuilder::new` are unaffected, which is every
  `with_protocol_binding` call site in this repository and its book.

- **One slow multicast consumer stalled every other member's stream**
  (`a2a-protocol-slimrpc`). `stream_message` gave each invited agent its own
  bounded channel — under a comment saying "a slow or silent agent cannot hold
  up another's events" — and then awaited `send` on those channels from a single
  shared loop. The split handles a *silent* agent, which produces no frames; it
  does nothing for a *slow consumer*, whose full channel parks the one loop
  every member's frames arrive through. Measured with two agents emitting 300
  events each and one consumer never polled: the live member's stream reached
  151 events in 25 seconds and never resumed. It now reaches 300 in 220 ms. The
  send is non-blocking, and a member that falls behind is handed an error naming
  how many events it missed rather than a stream with a silent hole in it —
  the same contract the SSE fan-out already has through broadcast's `Lagged`.

- **The Axum router ignored `body_read_timeout`.** `DispatchConfig` carries two
  body bounds; this router honoured `max_request_body_size` — wired in
  deliberately, with a comment explaining that Axum's `Bytes` extractor would
  otherwise fall back to its own `DefaultBodyLimit` and make the knob a no-op —
  and never applied the timeout sitting beside it. Measured with
  `body_read_timeout(1s)`, announcing a 1000-byte body and sending 8:
  `JsonRpcDispatcher` answered at 1.002 s and the Axum router had said nothing
  after 12 seconds. A slowloris body is what the knob is for, and the one shape
  a size cap cannot catch — the bytes never arrive, so the cap is never
  reached. A `TimedBody` extractor now reads the body under the configured
  deadline and answers `408 Request Timeout`; measured at 1.002 s.

- **`TaskStoreConfig::max_page_size` did nothing on any SQL store.** The four
  SQL stores hold a connection pool and no config, and each carried its own
  hardcoded `n.min(1000)` — the same number `TaskStoreConfig` uses as its
  default, so the configurable bound and the hardcoded one agreed by
  coincidence. Measured with the cap set to 10 against 60 stored tasks and a
  client asking for 100: `InMemoryTaskStore` returned 10 and `SqliteTaskStore`
  returned all 60. The book documented the field as capping `list` generally,
  under Design Considerations rather than under any one store. Each SQL store
  now takes `with_max_page_size`, all five sites default to the new
  `DEFAULT_MAX_PAGE_SIZE`, and the book says which config applies to which
  store. Third instance of one shape: a configurable bound whose default equals
  the hardcoded fallback, inert until the person who cares tightens it.

- **Concurrent creates walked straight through the per-task push-config cap.**
  `max_push_configs_per_task` is enforced by reading the task's configs,
  deciding, then storing — two `.await` points apart, with nothing held across
  them. Every concurrent caller read a count under the cap and every one of
  them stored. Measured against a cap of 5 with 32 concurrent creates, three
  runs stored 12, 17 and **32** — the last being all of them, a documented
  ceiling doing nothing at all. Unlike the task store's transient overshoot
  this was permanent: nothing re-checks a stored config. The sequence now holds
  a per-task lock, reusing the same bounded facility `SendMessage` already used
  against the identical race on `context_id`; the same three runs now store
  exactly 5. The *global* cap remains approximate across distinct tasks
  (measured 10, 5, 5 against a cap of 5) — making it exact means one
  server-wide lock on every create, which is a throughput decision rather than
  a bug fix, and is recorded as backlog B20.

  Worth knowing who this bit: `InMemoryPushConfigStore` has its *own* per-task
  cap, default 100, enforced atomically under its own lock, and the builder
  never passes `HandlerLimits` to it. At the shipped defaults both are 100 and
  the store's correct check masked the handler's racy one — so the defect
  appeared for an operator who *lowered* `max_push_configs_per_task`, and for
  every SQL-backed deployment at any setting, since those stores do not
  self-enforce and the handler's check is the only one there is.

- **`with_stream_connect_timeout` did nothing on the gRPC transport.**
  `ClientBuilder` carries three timeouts and `build_grpc` passed two, dropping
  the third; the gRPC transport then bounded a stream's first event on the
  *unary request* timeout instead. Both default to 30 seconds, so the knob only
  failed once you set it — and the sync `build()` path even rejects a zero
  value for it while `build_grpc` silently accepted one. `build_grpc` now
  validates and passes it, and the transport gained
  `with_stream_connect_timeout` (on the transport rather than on
  `GrpcTransportConfig`, whose public fields make adding one a breaking
  change). REST and JSON-RPC were already correct; the WebSocket transport has
  no such knob and consistently uses its own.

- **A hot-reload SIGHUP watcher could abort the process, minutes after
  startup.** `spawn_signal_watcher` registered its handler *inside* the spawned
  task, and `tokio::signal::unix::signal` panics — it does not return an error
  — when the runtime has no signal driver, which is what a hand-built runtime
  without `enable_all()` gives you. The panic therefore arrived after the
  function had returned a `JoinHandle`: nothing for the caller to see coming,
  nothing to catch, and under this workspace's release `panic = "abort"` a
  process abort rather than one lost feature. Registration now happens
  synchronously in `spawn_signal_watcher`, so the failure lands in the caller's
  own startup path where it is deterministic — and the `# Panics` section is now
  attached to the function that actually panics. The loop also stops discarding
  the `Option` from `recv()`, which was a latent hot loop: `None` means no
  further signals can arrive, and it answered by asking again immediately,
  forever.

- **`InMemoryCredentialsStore`'s `Debug` panicked on a poisoned lock.**
  Formatting runs while somebody is diagnosing the first failure, often from a
  logging path, so a panicking `Debug` replaces the diagnosis with a second
  failure — and a process abort in release. It now reports `<lock poisoned>`.
  The accessors on the same type still propagate poisoning, deliberately: a
  credentials lookup that quietly returns `None` is a silent auth downgrade,
  which is worse than a panic. Different answers for different call sites, both
  now written down.

- **`HotReloadAgentCardHandler` turned one panic into permanent failure.** Both
  accessors `expect`ed on their `RwLock`, and poisoning is sticky, so a single
  panic anywhere under the write lock made `current()` — the function that
  answers `GetAgentCard` — panic from then on, and abort the process in release.
  Both now recover from poisoning, which is correct rather than merely
  convenient here: the only write is a whole-value assignment of an
  already-constructed `AgentCard`, so no reader can observe a half-updated one.

- **`max_tenants` was documented by the half of its effect that is
  comfortable.** "Prevents unbounded memory growth from tenant enumeration
  attacks" is true and stops one step short: tenant ids come from resolvers
  that all read client-controlled input, so an enumerator does not get memory —
  it gets a lockout of every tenant that arrives afterwards. The reclamation
  path, `prune_empty_tenants`, is not automatic and only removes partitions
  whose task count is zero, so at the shipped one-hour TTL a burst of 1,000
  junk tenant ids locks out new tenants for an hour even with pruning
  scheduled. Both methods now document that, with a test pinning it.

- **`InMemoryTaskStore`'s documentation described a design it does not have.**
  It said eviction "runs as a background task" and that "writers are not
  blocked during the O(n) cleanup". There is no `spawn`: the sweep is awaited
  inside `save` and holds the write lock for its whole duration, so the write
  that triggers it pays for it and every concurrent writer waits. Measured at
  50,000 terminal tasks with `eviction_interval` 1000, the quietest of 1,000
  consecutive saves took 3.99 µs and the one that swept took 4.54 ms — about
  1,100×. The behaviour is reasonable and unchanged; what was wrong is that the
  paragraph an operator reads to size a latency budget named the two properties
  that would have made the number not matter. Corrected with the measurements
  and a note that `eviction_interval` is a tail-latency knob. The observable
  half — `save` never returns with the store over capacity — now has a test.

- **A cancelled WebSocket request leaked its entry in the client's pending
  map, and so did an abandoned stream.** The map is keyed by JSON-RPC request
  ID, has no capacity bound, and lives as long as the connection. An entry was
  removed on exactly three paths — a routed response, the explicit timeout
  branch, and connection teardown — and a caller whose future is *dropped*
  takes none of them; neither does a consumer that walks away from a stream the
  server never fed, because all three streaming removals need a frame to
  arrive. Measured: five cancelled requests left five entries, five abandoned
  streams left five more, each pinning a channel sender. Registration moved to
  the caller and both ends are now owned by a `Drop` guard, which travels with
  the `EventStream` for the streaming case. A `select!` racing a request against
  a shutdown signal is an ordinary way to write a client, so on a long-lived
  connection this was unbounded growth on the request path.

- **The event queue's `write_timeout` was never applied.** `DEFAULT_WRITE_TIMEOUT`
  is public, `new_in_memory_queue_with_options` takes it, `EventQueueManager`
  prints it in `Debug` — and the field was `#[allow(dead_code)]`. The
  persistence channel is a bounded `mpsc` whose `send` waits with no deadline of
  its own, so a stalled background processor parked the executor indefinitely:
  measured, `write` blocked after 1,024 events and was still blocked eight
  seconds later, with no metric and no trace. The deadline is now enforced and a
  full channel returns an error naming the cause. A *closed* channel is still
  not an error. The surrounding documentation, which said this channel "will
  never lose events" / "will never lag" / that the processor "must never miss
  events", now says what it does: full past the deadline is an error naming the
  stalled processor, closed drops the event and returns `Ok`, and the second is
  reported only by a `trace_warn!` that compiles to nothing without the
  non-default `tracing` feature.

- **Push-notification configs the delivery budget never reached were invisible.**
  At the shipped defaults — 100 configs per task, a 5-second per-delivery
  timeout, a 30-second per-event budget — a webhook estate that is timing out
  receives 6 of 100 and the other 94 were reported by a single `trace_warn!`,
  which compiles to nothing without the non-default `tracing` feature. Each
  skipped config now increments `push_outcome::SKIPPED`. The `Semaphore::new(16)`
  built per event, described in a comment as a cap on concurrent deliveries that
  the sequential loop never performed, has been removed.

- **`shutdown_with_timeout(t)` could take `2t`.** The drain phase ran to
  `now + t` and the executor's cleanup hook was then given a *fresh* full `t`.
  Measured: `shutdown_with_timeout(30s)` took 60 seconds. The number an operator
  puts here is the number they put in `terminationGracePeriodSeconds`, so
  overrunning it means `SIGKILL` part-way through the cleanup graceful shutdown
  exists to perform. The two phases now share one deadline.

- **The push retry policy could not run at the shipped defaults.**
  `HttpPushSender::new()` schedules three attempts at a 30-second request
  timeout with `[1s, 2s]` backoff — 93 seconds — inside a
  `push_delivery_timeout` that defaults to 5. Measured against a real socket:
  one attempt of three reaches the webhook. The defaults are unchanged, because
  choosing between them is a deployment decision; what is new is
  `PushSender::max_delivery_duration()` (a defaulted trait method) and
  `push_outcome::TIMEOUT_TRUNCATED`, so a truncated schedule is counted rather
  than mistaken for a slow endpoint, and the arithmetic is documented on
  `HandlerLimits::push_delivery_timeout`.

- **Agent-card discovery applied its 30-second timeout twice.** The request and
  the body read each had their own, so a server that stalled on headers and then
  drip-fed the body held the caller for both: measured at **55 seconds** against
  a documented 30. One deadline now covers the whole fetch.

- **The JWKS and OIDC-discovery body read had no deadline at all.** The
  30-second budget bounded the request; the body was bounded by a size cap,
  which bounds memory and not time. A server dripping one byte every 300ms held
  the fetch open indefinitely — measured still running at 45 seconds. This sits
  on the request path, because a `kid` that misses the cache forces a JWKS
  refetch inside token validation.

- **`codecov.yml`'s PostgreSQL exclusion has never applied.** All five listed
  paths are still in the coverage denominator, together with two never listed:
  957 lines and 881 of 2,096 missed lines — 42% of every uncovered line in the
  repository. Reported coverage is 94.06%; without them it is 96.46%. The
  patterns are deliberately unchanged (Codecov *accepts* them, so validation
  proves nothing); `scripts/check_codecov_ignores.py` is the check that will
  settle the next attempt.

- **Four broken rustdoc intra-doc links** in `rate_limit/` that resolved only
  under workspace feature unification, so `cargo doc -p a2a-protocol-server`
  emitted four warnings while CI's `cargo doc --workspace` emitted none.

- **`signing` feature descriptions overstated what they enable.** Neither
  `a2a-protocol-client` nor `a2a-protocol-server` contains any
  `#[cfg(feature = "signing")]` code; both forward the types-crate feature so
  you can call `sign_agent_card` / `verify_agent_card` yourself. The client's
  read "Enable agent card signing verification", which claimed an integration
  that does not exist.

- **Per-caller rate limiting was not per-caller.** `caller_identity` existed on
  the interceptor and no code path ever set it, so every caller shared a single
  bucket and one noisy client could exhaust the limit for everyone. The identity
  is now settable and is set from the authenticated principal.

- **The SQLite artifact journal was created only by `from_pool`**, so a store
  opened through the migration path lacked the table the incremental artifact
  writer depends on.

- **SLIMRPC did not transmit or check the protocol version**, and leaked
  internal transport metadata into the A2A header map that applications see.

- **The SLIMRPC binding's packaging gate could not pass during a release.** The
  binding depends on the SDK crates by `version` *and* `path`; `cargo package`
  strips the path, so the pin resolves against crates.io — where the version
  being prepared does not exist yet. Reverting the pin broke the binding's build
  instead, so no pin value was green between the version bump and publication,
  and the 0.10.0 prep was reverted rather than accommodated. `ci.yml` now runs
  `scripts/package_binding.py`, which skips registry resolution alone for
  exactly that state — every pin naming the in-tree version, and that version
  absent from the index — proves the rest of packaging with `cargo package
  --list`, and fails on everything else, a typo'd pin included. The skip is a
  warning annotation on the job, and it closes itself once the SDK is published.

- **The nightly canary tested but never linted**, so it could not give notice of
  the breakage it exists for. Rust 1.98.0 turned three clippy lints red on code
  identical to `main`, two of which (`map_or_identity`,
  `unused_async_trait_impl`) did not exist in 1.94.1 at all — and the canary ran
  only `cargo test`. It now runs `cargo clippy --all-targets --all-features -D
  warnings` as well, on the same informational job, which is where a lint
  arrives before it reaches stable. The toolchain is still not pinned: a
  `rust-toolchain.toml` takes precedence over the toolchain the CI action
  selects, which would silently redirect the MSRV leg of the `1.93` matrix at
  the pinned version instead — a worse failure than the one it would prevent.

- **The SLIMRPC spec drift check was blind to any spec file it had not been
  told about.** It fetched two files by URL and hash-compared them, so it could
  report agreement about those two while upstream added a third — which is what
  happened. `spec/v1/slimrpc-collaborative-channel.md` has existed on an
  upstream branch, and the official `a2a-slimrpc` crate has implemented it,
  without anything here noticing. The check now clones upstream and takes its
  inventory from `main`, so an added or withdrawn spec file fails as loudly as a
  changed one, and it surveys the other branches: a branch-only spec must carry
  a written disposition or CI fails until somebody triages it.

- **The provenance manifest had gone stale, in the project's favour.**
  `docs/provenance-manifest.md` is written for a downstream project's counsel —
  the A2A project and the Linux Foundation are named in it — and nothing forced
  it to be regenerated. It still reported 749 commits and 19.4% of history
  passing the project's own DCO gate; re-measured at `7093af3` the figures are
  977 commits and **39.2%**. A counsel-facing document that understates the
  project is the harder defect to notice, because nobody is motivated to check
  it. `release.yml` now runs `scripts/check_provenance_manifest.py`, which fails
  a release whose manifest was measured at a different commit or whose headline
  figures do not match a fresh run, and refuses outright on a shallow clone —
  the truncation that made the two previous hand-derived figures wrong by 2.3x.

- **`SECURITY.md` understated release-artifact verifiability.** It said all
  release tags were lightweight and unsigned, so there was "nothing to verify".
  `v0.8.0` and `v0.9.0` are annotated tag objects carrying a tagger and a date —
  `release.yml`'s annotated-tag gate working, in the two releases cut since it
  landed. Signing remains a genuine gap and is still stated as one. The same
  stale claim is corrected in `RELEASING.md`, `ROADMAP.md` and `PROVENANCE.md`.

- **`prove_workflow_gates_fail.py` could not see a new script-invoking step.**
  It discovered gates by looking for an explicit `exit` in the step body;
  steps whose verdict is a checker's exit code were reachable only through a
  hand-maintained list, so *adding* one left it silently unproven. Discovery
  now also matches a step running one of this repository's own scripts, which
  turns a forgotten registry entry into a failed build.

- **The SQLite connection pragmas were written out four times.** `journal_mode`,
  `busy_timeout`, `synchronous` and `foreign_keys` appeared byte-identically in
  `store::sqlite_store`, `store::tenant_sqlite_store`, `push::sqlite_config_store`
  and `push::tenant_sqlite_config_store`, with nothing asserting they agreed and
  three of the four also hard-coding the same pool size. Correcting one in
  response to a bug report would have left the other three at the old value with
  every test still passing — the shape `DEFAULT_MAX_PAGE_SIZE` already cost this
  repository once. They now live in one private module, with a test that asks
  `SQLite` for the effective values on a real file rather than asserting the
  builder was called. Measured while writing that test: removing `journal_mode`
  or `synchronous` fails it, and removing `busy_timeout` or `foreign_keys` does
  not, because `sqlx` already defaults both to the values being asked for. Both
  facts are now recorded next to the pragmas.

- **The `unwrap`/`expect` count in library code is now a CI ratchet** rather
  than a number typed into `CONTRIBUTING.md` that no method could reproduce
  (B5). `scripts/check_panic_paths.py` strips comments and string literals with
  a state machine and follows `#[cfg(test)]` gating transitively — a file
  declared by a test-gated module is test code too, even though it carries no
  attribute of its own and no "test" in its name. The measured surface of the
  published crates: **0 `.unwrap()`, 10 `.expect(`, 0 `panic!`, 0 `todo!` in
  runtime library code**, with `build.rs` reported separately. Adding one now
  fails the build until the baseline is updated deliberately.

- **`AgentExecutor::cancel`'s documentation described behaviour it stopped
  having in 0.7.** The rustdoc said "the default implementation returns an error
  indicating the task is not cancelable. Override this to support task
  cancellation." The default has cancelled since 0.7 — it emits the terminal
  `Canceled` status — and the inline comment beside it says the refusing default
  was removed precisely because it left `Working` tasks uncancelable out of the
  box. A reader of docs.rs would have believed the opposite of what the code
  does, and would have written an override to get behaviour they already had.

- **Four book chapters, and a gate that could not see whether they compiled.**
  `deployment/{troubleshooting,observability,multi-tenancy,security}.md` close
  B10. Separately: `a2a-book-tests` registers pages by hand, and nothing checked
  the list — an unregistered page's Rust was never compiled and nothing said so,
  because a page with no `ignore`d blocks looks exactly like a page being
  compiled. Four pages were already unregistered. `check_book_code.sh` now fails
  on an unregistered page, and on an exclusion whose page no longer exists.

- **The example coverage matrix could print a total larger than the grid.**
  `a2a-example-harness` scores which A2A methods ran over which bindings, and
  its summary took "exercised" from one collection and "not applicable" from
  another — two collections a single cell can be in at once. A cell that was
  both excused and then exercised, or excused twice, was counted twice, so the
  line read `1 exercised, 1 not applicable, 43 missing, of 44 cells`. Both
  numbers now come from one classification of the grid, so the three buckets
  partition it by construction; `excuse` deduplicates as `record` always has;
  and an excuse for a cell that was then exercised no longer prints under "Not
  applicable" while the grid shows it as `ok`.

- **Four examples each carried their own copy of the agent card's interface
  list.** `Endpoints` and `interfaces()` were byte-identical in `genai-agent`,
  `rig-agent` and `multi-lang-team`, and `echo-agent` wrote the same four
  `AgentInterface` literals inline. Each copy hand-wrote `"HTTP+JSON"` — the
  string the SDK ships `AgentInterface::rest` to avoid, its own documentation
  saying a typo there "is a card that lies". All four now use one
  implementation in `a2a-example-harness`, built from the SDK's constructors,
  and a test asserts the card's `protocolBinding` values match the coverage
  matrix's column labels, which were two independent spellings of the same
  four names.

- **The three LLM-backed examples had no tests, and their success paths were
  assumed untestable.** `genai-agent`, `rig-agent` and `multi-lang-team` now
  have 19 tests between them, none of which touch a network or a model.
  `genai`'s service target is overridable, so the unreachable-provider branch
  runs against a dead endpoint identically on a laptop with `llama-server` up
  and on a CI runner with nothing; `rig`'s executor is generic over
  `CompletionModel`, so a fake that answers makes the *success* path testable
  with no provider at all — including that a real answer is not dressed as a
  mechanical fallback. Every example in the repository now has tests.

- **Every example was run end to end against a real model** — `Qwen3.5-0.8B`
  under `llama.cpp` — rather than only against fakes: 44/44 protocol cells
  each, 102/102 for `agent-team`, and zero mechanical fallbacks in any LLM
  run, so the fallback paths the unit tests exercise really are the degraded
  path. Recorded with the model's size and sha256 in
  `docs/lf-readiness-review.md` so the run is reproducible.

- **`rig-agent`'s README pointed at a llama.cpp download that 404s.** Its
  quickstart fetched `releases/latest/download/llama-bin-ubuntu-x64.tar.gz`;
  llama.cpp's release assets carry the tag in the filename, so no tag-less
  `latest/download` URL can resolve. Replaced with a from-source build,
  verified by running the commands exactly as written into a clean directory.

### Performance

- **Streaming artifact appends no longer pay for the stream so far.** SQLite
  journals appended artifact parts instead of rewriting the whole task on every
  delta.

### Security

- **The registration-time webhook filter now normalizes numeric IPv4
  encodings.** `validate_webhook_url` accepted the non-canonical forms the C
  resolver (`inet_aton`, and so `tokio`'s `lookup_host`) maps to private
  addresses — `http://2852039166/`, `http://0xA9FEA9FE/` and
  `http://0251.0376.0251.0376/` all denote `169.254.169.254`, the cloud metadata
  endpoint — because `Ipv4Addr::from_str` only accepts dotted-quad. Delivery
  already blocked them (`lookup_host` resolves the integer and the IP-range check
  re-runs on the resolved address), so this was a defense-in-depth gap rather
  than a live SSRF; it is nonetheless the exact attack class the filter targets,
  which already rejected the IPv4-mapped and NAT64 spellings of the same address.
  The static check now decodes the decimal, hex and octal forms and applies the
  same private-range test; public numeric hosts still pass, matching delivery.
  Found by an over-the-wire adversarial run against a live model — 20/20 hostile
  webhook URLs rejected with the guard on, up from 19/20.

- **`examples/genai-agent` no longer ships with the SSRF guard disabled.** Its
  server mode — labelled "the real deployment shape" in its own comment —
  hard-coded `allow_private_urls()`, silently turning the guard off for anyone
  adopting the example as a deployment template. It is now secure by default;
  `A2A_ALLOW_PRIVATE_WEBHOOKS=1` re-enables loopback webhooks for local testing,
  and the active posture is printed at startup. (Example crate, `publish =
  false`, so no published crate changed — but the template did.)

- **RUSTSEC-2026-0258** — h2 raised to 0.4.16, without the dependency downgrade
  the advisory's own suggested fix would have pulled in.

### Changed

- **The SLIMRPC binding now states where it diverges from the official
  `a2a-slimrpc` crate**, in its README, the book chapter and
  `spec/slimrpc_v1/README.md`. Both implement all eleven A2A methods, but
  neither is a superset of the other: this crate implements the multicast
  specification on upstream `main` and the official one does not; the official
  one implements Collaborate, whose specification is on an unmerged upstream
  branch, and this one does not. The two are different operations rather than
  two names for one, so "which is more complete" is the wrong question — the
  docs now say so with a comparison table instead of leaving a reader to infer
  it. Tracked as B24.

- **`PerTenantConfig` and `TenantLimits` now document that nothing enforces
  them** (`a2a-protocol-server`). All five per-tenant limits are stored,
  resolvable through `PerTenantConfig::get`, and read by no code in the request
  path — verified by enumerating every mention of the type outside its own
  module: an import, a field, the `tenant_config()` accessor, a `Debug` line, a
  builder field and its setter, and otherwise tests. `executor_timeout` and
  `event_queue_capacity` share their names with live process-wide fields on the
  handler and its builder, which is what made them look wired; those fields are
  not these fields.

  The module header previously advised operators to *"set per-tenant
  `max_concurrent_tasks` so that the sum across active tenants stays within the
  process-wide caps if noisy-neighbor isolation matters"* — advice to rely on a
  field nothing reads. Enforcing all five is a feature with open design
  questions (B22 in `docs/v0.9.0-post-release-review.md`); enforcing some of
  them would leave live knobs beside inert ones, which is the defect shape
  itself. So the behaviour is unchanged and the documentation is now exactly
  true: the module states what it stores and resolves, each field names what
  would enforce it, and the two builder setters no longer claim to enable
  per-tenant limits. **Data isolation is unaffected** — it runs through the
  tenant-aware stores' partitioning, not through any limit here.

- **`ClientConfig::max_response_size` now states what it reaches.** It governs
  every transport `ClientBuilder` constructs — JSON-RPC and REST directly, gRPC
  as its `max_message_size` — and not a transport supplied to
  `with_custom_transport`. Two shipped transports are in the latter position.

  `WebSocketTransport` is the nearer one, and the more misleading:
  `WebSocketTransportConfig::max_message_size` defaults to the *same constant*
  as `max_response_size`, and its doc said it was "the same response-size
  ceiling the HTTP and gRPC transports apply" — true of the default, and
  reading as though the two were connected. They are not. Tightening
  `max_response_size` to 1 MiB and connecting over WebSocket still admits
  32 MiB. Both ends now say so, and point at the setting that works.

  `SlimRpcTransport` is the other, and its real bound was traced through the
  stack rather than assumed:
  the binding's codec, `slim_rpc` 2.3 and `agntcy-slim-datapath` 0.18 all set
  none, so what applies is tonic 0.14's default — **4 MiB on receive**, eight
  times tighter than the 32 MiB documented here, and unbounded on send. Neither
  is settable from this repository; both are now written down where a reader
  hits them.

## [0.9.0] - 2026-08-16

### Breaking

- **`executor_timeout` now defaults to one hour instead of being unbounded.**
  An executor that never returns previously pinned its task, its event queue and
  its cancellation token for the life of the process, and nothing reclaimed
  them; with `max_cancellation_tokens` at 10,000, enough of them eventually stop
  the handler accepting work. The old rationale — that any fixed value would
  fail legitimately long-running tasks — is sound about *short* ceilings and
  wrong about the default, which put the safe configuration behind an action
  nobody is reminded to take.

  An hour cannot plausibly interrupt an interactive or streaming agent turn, and
  a task genuinely running longer should be using push notifications (§7) rather
  than holding an executor and a stream open. If yours legitimately exceeds it,
  set `with_executor_timeout()`, or call the new `without_executor_timeout()` to
  restore unbounded execution explicitly. A task that trips the ceiling fails
  visibly, as a Failed task with a timeout error.

- **`RequestHandler::shutdown()` and `shutdown_with_timeout()` now return
  `ShutdownReport`** instead of `()`. The type is `#[must_use]`, so existing
  callers get a warning rather than an error. Both previously discarded the
  executor-cleanup timeout with no log and no return value, which made a hung
  cleanup indistinguishable from a clean drain.

### Added

- **Two `Metrics` callbacks for the failures nothing else reported.**
  `on_persistence_error` fires when the background processor cannot persist a
  task — the SDK's one silent-data-loss path, since the streaming reader is a
  separate subscriber and receives the event whether or not the store accepted
  it. `on_push_delivery` reports every delivery attempt by outcome. Both were
  previously visible only through `tracing`, which is not a default feature of
  `a2a-protocol-server`, so a default build discarded them entirely. Both are
  exported by the bundled OTLP exporter as `a2a.server.persistence_errors` and
  `a2a.server.push_deliveries`.

- **`GET /ready`** on the Axum router — a readiness probe that actually reaches
  the task store, alongside `/health`, which remains a constant on purpose. A
  liveness probe that follows a downstream down restart-loops every replica
  during that downstream's outage. Backed by the new
  `RequestHandler::task_store_health()`.

- **`TaskStore::save_artifact_delta`**, defaulted to `save`, so every existing
  implementation keeps working. Lets a store persist what changed rather than
  the whole record. Implemented by all three bundled stores.

- **`ErrorCode::metric_label` / `A2aError::metric_label`** — a bounded,
  low-cardinality label for every error code, safe to use as a metric dimension.

- **`a2a-protocol-slimrpc` is publishable**, and the SLIMRPC specification is
  vendored at `spec/slimrpc_v1/` so the binding's method inventory is checked
  against the document it claims to implement.

- **`examples/deploy-agent`** — the smallest agent you can actually ship:
  environment configuration, liveness and readiness endpoints, a `SIGTERM`
  drain, a `0.0.0.0` bind, a two-stage `Dockerfile` and a Kubernetes manifest.

- **`examples/hello-agent`** — 23 lines of code against the umbrella crate
  alone.


- **`EventStream::from_event_channel` on `a2a-protocol-client`, which is what
  made an out-of-tree custom transport possible at all.**
  `Transport::send_streaming_request` must return an `EventStream`, and every
  constructor for one was `pub(crate)` — so a third-party binding could
  implement the unary half of the trait and not the streaming half. The trait is
  `pub`, its parameters are `pub`, its return type is `pub`, and it was still
  unimplementable outside the crate.

  The new constructor takes a `tokio::sync::mpsc::Receiver` of decoded
  `StreamResponse` values, so a transport hands over domain events and never
  has to know that the internal representation is SSE. Sending `Err` delivers
  that error to the consumer, which is how a transport reports a mid-stream
  decode failure — ending the stream silently would be indistinguishable, to
  the consumer, from finishing normally. The returned stream aborts its bridging
  task on drop, exactly as the built-in transports' streams do.

  Purely additive. Found by building `a2a-protocol-slimrpc` against the
  extension points rather than by reading them; `docs/rust-sdk-assessment.md`
  §4.1.1 has the correction to what it previously claimed.

- **`bindings/a2a-protocol-slimrpc` — the SLIMRPC protocol binding.** Carries
  A2A over the [AGNTCY SLIM](https://github.com/agntcy/slim) fabric per
  [`a2aproject/experimental-cpb-slimrpc`][slimrpc-spec], advertising
  `protocolBinding: https://a2a-protocol.org/bindings/experimental-slimrpc/v1`
  and addressing agents as `slim://[node[:port]/]domain/namespace/service`.

  All eleven methods in the spec's inventory — nine unary, plus
  `SendStreamingMessage` and `SubscribeToTask` as unary-request /
  streaming-response. Payloads are the canonical `lf.a2a.v1` protobuf messages
  (SLIMRPC uses the same service definitions as gRPC), so the wire is
  byte-compatible with the official Go, Python and Java SDKs. Error identity
  travels as the spec's `TaskNotFoundError: …` message prefix, because SLIMRPC
  has no `google.rpc.ErrorInfo` equivalent and a status code alone cannot
  distinguish `TaskNotCancelableError` from `ExtensionSupportRequiredError`.

  `SlimRpcServer` drives the same `RequestHandler` every other binding drives,
  so task state, streaming, push, tenancy and authorisation are not
  reimplemented and an agent behaves identically however it is reached.

  **Deliberately outside the workspace, with its own `Cargo.lock`.**
  `agntcy-slim-rpc` brings 379 transitive dependencies including `aws-lc-sys`, a
  native C crypto build; `a2a-protocol-types` has 12. None of that reaches the
  lockfile, `deny.toml` allow-list or audit surface of the four published
  crates, and none of them depends on this one.

  27 tests. The seven end-to-end ones are not mocked: one in-process SLIM
  `Service` hosts an agent app and a caller app and messages cross the real SLIM
  datapath, covering method registration, a unary round trip, a task fetched
  back by id, error identity surviving the fabric, a streaming send running to
  its terminal event, agent-card advertisement, and an unknown method being
  reported rather than hanging.

  [slimrpc-spec]: https://github.com/a2aproject/experimental-cpb-slimrpc

- **SLIMRPC multicast — one message, several agents, one outcome each.**
  Implements the separate [`spec/v1/slimrpc-multicast.md`][slimrpc-spec]:
  `SlimRpcMulticast` opens a SLIM group channel, invites specific agents by
  name, and broadcasts. Only `SendMessage` and `SendStreamingMessage` may be
  broadcast — task management stays point-to-point, because a task id is
  meaningful to exactly one agent.

  `MulticastOutcome` carries **exactly one outcome per invited agent**, which is
  the spec's requirement (*"Clients must wait for outcomes from every invited
  agent"*) and the reason multicast is not a `Transport`: `send_request` returns
  one value, and reducing N attributable answers to one would have to drop
  either the attribution or the failures.

  Two failure kinds stay distinct because they call for different responses. An
  agent that errors or stays silent past the timeout is an isolated per-agent
  outcome and the other agents' answers stand; a member that cannot be *invited*
  fails the whole call, because the group is misconfigured and waiting will not
  fix it. That line is the spec's own: *"Only channel creation, agent
  invitation, or request delivery failures constitute interaction-level
  failures."* `stream_message` gives each agent its own `EventStream`,
  demultiplexed from SLIM's interleaved source-tagged frames.

- **Verified across a real SLIM node.** `tests/remote_node.rs` runs three
  separate SLIM services in one process — an agent, a client, and a node that
  only routes — connected over loopback TCP. The agent and client share no
  `Service` and no memory; every message crosses a socket twice and is routed in
  between. `SlimRpcServer::from_app_with_connection` and
  `SlimRpcTransport::from_app_with_connection` take the connection id
  `Service::connect` returns.

  This found a real bug that in-process testing structurally could not: nothing
  announced a *client's* own name to the node, so while the agent was reachable,
  no route existed for its reply, and every call failed its session handshake
  with the caller's own name reported as unroutable. `Channel` sets a route
  outwards only. The client-side constructor is now `async` and subscribes the
  caller's name over the connection.

- **SLIMRPC verified across every topology a deployment actually uses**, and
  identity generalised beyond shared secrets. The previous entry listed these as
  limitations; they are now covered rather than documented.

  | Suite | Topology | What only this can catch |
  |---|---|---|
  | `remote_node_tls.rs` | one node, **verified TLS** | a TLS path that verifies — proven by refusing an untrusted CA |
  | `remote_node_multihop.rs` | **two peered nodes** | subscriptions crossing a node-to-node link |
  | `out_of_process.rs` | node in a **separate OS process** | anything relying on shared memory or a shared runtime |

  TLS uses a throwaway CA generated per run rather than committed PEM files, so
  nothing long-lived is in the repository and no certificate can expire the
  build. The rejection test is a differential against the same node in the same
  window — the trusted CA connects, the untrusted one does not — because SLIM
  retries a failed handshake rather than returning, and a bounded wait alone
  would only prove slowness.

  Multi-hop needs `ConnType::Peer` between nodes: an `Edge` link carries an
  attached app's traffic but does not share routing state, so an agent behind a
  second node stays invisible without it.

  `SlimRpcServerBuilder::with_identity` and the transport's equivalent now take
  SLIM's own `AuthProvider` / `AuthVerifier`, so JWT, SPIFFE via SPIRE and
  static tokens all work without this crate enumerating mechanisms and lagging
  behind SLIM. `with_shared_secret` becomes a convenience over it and is now
  fallible, since SLIM rejects a secret too short to be a credential. A builder
  with no identity is a build error: SLIM has no anonymous mode, and a default
  would quietly stand in for one.

- **SPIFFE and mutual TLS, both verified rather than asserted.** The previous
  entry listed SPIFFE as "supported but untested" and mutual TLS as absent.

  `spiffe.rs` runs against a **real** `spire-server` and `spire-agent`: the
  agent attests the test process over the Workload API and issues genuine
  JWT-SVIDs. A stub would have proven only that the types line up. Three tests —
  an A2A call carried by SPIFFE identity, the issued SPIFFE ID pinned to the one
  registered, and a verifier refusing a genuinely-issued SVID minted for a
  *different audience*. That last one is what gives the first its meaning: a
  verifier that accepted everything would pass the positive test.

  Two SPIFFE properties surfaced only by running it, both presenting as a
  session that never completes rather than an authentication error, and both now
  documented: `SpireIdentityManager` must be built **once and cloned** for
  provider and verifier (it holds an MLS signature key, so two managers carry
  two different ones), and each app needs its **own** SPIFFE ID (two apps
  sharing one cannot complete an MLS handshake). The testbed therefore registers
  an identity per app and selects between them with `with_target_spiffe_id`.

  `remote_node_mtls.rs` covers mutual TLS: a node that authenticates its apps,
  not merely itself. Three cases, meaningful only together — a client with a
  certificate from the node's client CA connects and A2A works; a client with no
  certificate is refused; a client with a well-formed `ClientAuth` certificate
  from a *different* CA is refused. The first alone would pass just as happily
  against a node ignoring client certificates entirely.

  Every negative case is paired with a control that succeeds in the same window,
  because SLIM retries a failed handshake rather than returning, and "did not
  connect" on its own can mean the fixture was broken rather than the control
  working.

  I had previously recorded that running SPIRE needed an agent this environment
  did not have. That was not checked, and it was wrong — SPIRE runs here fine.

- **SPIFFE trust-domain federation and credential rotation, both against real
  SPIRE.** The previous entry listed these as limitations.

  `spiffe_federation.rs` runs **two** independent SPIRE deployments. An SVID
  from an unfederated domain is refused; after a bundle exchange and entries
  naming each other, the same SVID is accepted — and a full A2A call runs
  between an agent attested by one organisation's SPIRE and a caller attested by
  another's. Both halves are needed: acceptance alone would be
  indistinguishable from a verifier that ignores trust domains, and rejection
  alone from federation simply being broken.

  Federation surfaced an ordering rule worth knowing: a registration entry
  naming `-federatesWith` is rejected outright unless that trust domain's bundle
  is *already* imported. Bundles must be exchanged before entries are created,
  which is why the testbed now splits `start_with` from `register` rather than
  doing both in one call — the ordering is visible at the call site instead of
  being a comment someone can miss.

  `spiffe_rotation.rs` issues 40-second JWT-SVIDs so a rotation happens inside a
  test rather than half an hour later. Three properties: the manager serves a
  renewed credential without being asked; the superseded credential stops
  verifying; and a live A2A agent keeps answering across the rotation —
  including a stream opened afterwards — without being restarted or handed a new
  manager. Each test *proves the rotation happened* before asserting anything
  about it, because a test that merely waited and then succeeded would pass just
  as happily if nothing had rotated at all.

- **`slim-node` — a standalone SLIM node binary.** Routes and runs nothing
  itself. Prints `listening on <addr>` once the socket accepts, so a supervisor
  waits for readiness instead of sleeping; refuses half a TLS configuration
  rather than silently serving plaintext. It exists so the out-of-process claim
  is testable, and because bringing up a node otherwise means installing the
  full AGNTCY SLIM distribution.

- **`benches/send_latency_breakdown.rs`** — attributes the cost of a blocking
  send across three axes (runtime worker count, executor event count, and a
  round trip that runs no executor at all), so a future regression in this
  class is attributable by measurement rather than by inspection.
- **`benches/examples/send_probe.rs`** — a diagnostic that walks the task
  store across its capacity boundary and reports send latency per 1,000-send
  bucket. This is what turned "sends are slow" into "sends are slow past
  exactly 10,000 tasks".

### Changed

- Public traits are documented as **unsealed and staying that way**, with the
  rules for extending them in CONTRIBUTING — including why a defaulted method is
  not free.

- `handler/messaging.rs` (2,395 lines) split into `messaging/{mod,decisions,
  tests}.rs`. `send_message_inner` remains ~530 lines and is recorded in
  `.file-length-baseline` as the outstanding work.



- **Expired tasks are now reclaimed only by the TTL sweep's own interval.**
  Previously an over-capacity write also ran the TTL pass as a side effect, so
  a full store expired terminal tasks on every write. It now runs every
  `eviction_interval` writes (default 64), as its configuration always
  described. `max_capacity` remains a hard cap enforced on every write, so
  memory is bounded exactly as before; only the promptness of TTL reclamation
  changes, and only within the interval that already governed it.

### Fixed

- **Streaming was quadratic in the length of the stream.** The background
  processor saved the whole task on every artifact event, and the in-memory
  store deep-clones, so event *i* copied *i* artifacts. A 502-event stream spent
  43.4 ms against the in-memory store versus 3.2 ms against one that discards
  everything. `save_artifact_delta` makes it linear: **120.6 ms to 3.6 ms** for
  the distinct-artifact shape, **43.4 ms to 2.5 ms** for the append shape, with
  both curves flat.

  The same change on the SQL stores removes the Rust-side serialization of the
  whole task and its transfer as a bind parameter: SQLite 144.5 ms to 127.6 ms,
  Postgres 798 ms to 500 ms with per-event cost flat (853/840/1000 µs at 50/250/
  500 chunks) where `save` grew (874/1183/1597 µs).

- The `Message::text()` helper and the `PartContent` re-exports the README quick
  start needed. That snippet had not compiled for three minor versions.


- **`message/send` cost 32× more on any server that had handled more than
  10,000 tasks.** The default `InMemoryTaskStore` caps itself at
  `max_capacity` (10,000). Once full it is over that cap on *every* subsequent
  write, and every such write ran a sweep that cloned every `TaskId` in the
  store into a `Vec`, sorted it, and then removed — normally — a single task.
  A blocking send performs several saves, so one request paid that O(n log n)
  sweep several times over.

  It is a cliff, not a slope, and it never recovers: the store stays at
  capacity for the life of the process. Measured with
  `cargo run --release -p a2a-benchmarks --example send_probe`, which walks the
  store across the boundary and reports the median per 1,000 sends:

  | Sends so far | Before | After |
  |---|---|---|
  | 10,000 (at the cap) | 65.2 µs | 62.9 µs |
  | 11,000 (past it) | **2.4 ms** | **67.3 µs** |
  | 16,000 | **2.0 ms** | **64.3 µs** |

  Two changes fix it. Capacity eviction now walks `order_index` — already
  sorted oldest-first — and stops as soon as it has enough victims, instead of
  collecting and sorting the whole store; the search for a terminal task to
  prefer is bounded by a scan window, so the sweep is O(1) in the store size
  rather than O(n). And the O(n) TTL pass no longer rides along with it: the
  two passes are now separately triggered, so the amortized sweep stays
  amortized instead of running on every write once the store is full.

  End to end over loopback JSON-RPC, `transport/jsonrpc/send/single_message`
  improves **91%** (2.18 ms → 191 µs, p < 0.05). The control holds: the same
  run measures `get_task` on a missing id — one round trip that performs no
  write — unchanged at −0.25% (p = 0.76), confirming the gain is specific to
  the write path and not machine-wide noise.

  This was previously attributed, in a benchmark comment, to cross-thread task
  scheduling on 4-core CI runners. That hypothesis is disproved here: running
  the same send on a single-worker runtime does not close the gap. Scheduling
  is real but second-order — worth ~50 µs of the remaining 191 µs, and it was
  entirely masked by the eviction cost.

  The new sweep's fallback path — topping up with in-flight tasks when the scan
  window holds too little finished work — was correct but untested at its one
  interesting point. Mutation testing showed it: the first draft sized the
  top-up as `overflow - terminal.len()`, and changing that `-` to a `+`, which
  makes the sweep evict *more* than the overflow, broke no assertion. Every
  capacity test either filled the quota from terminal tasks alone (returning
  before the top-up ran) or found none at all (where the two operators agree),
  so the partial-supply case in between went unexercised. It now has a test,
  and the arithmetic it turned on is gone: the top-up is a second bounded pass
  over the same window, sharing the first pass's exit condition, which also
  drops a `Vec` of candidate ids that were usually collected and discarded.


## [0.8.0] - 2026-08-13

> [!WARNING]
> **If you serve gRPC to 0.6 clients, upgrade order matters — get it wrong and
> it is an outage, not a degradation.**
>
> This release deletes the JSON tunnel that let a 0.6 gRPC client talk to a
> 0.7 server. The service those clients dial, `a2a.v1.A2aService`, is no longer
> registered on the listener, so their calls fail at the gRPC layer — they do
> not fall back to anything.
>
> **Move gRPC clients onto the canonical `lf.a2a.v1.A2AService` first, then
> upgrade servers.** The tunnel existed precisely to keep a mixed fleet working
> during that migration, and 0.8 closes the window.
>
> Unaffected: JSON-RPC, REST and WebSocket deployments, and any gRPC deployment
> already on the canonical binding — the default since 0.7, and what the
> official Go, Python and Java SDKs speak.

**A breaking release, and the deletions are the headline.** Three deprecations
announced in 0.7 come out here, on the schedule their own deprecation notes
named. Nothing is deprecated-but-kept a second time: an item that says "removed
in 0.8" is removed in 0.8.

All four crates are now at **0.8.0**. Until this release
`a2a-protocol-server` alone sat at 0.8.0 while the other three stayed at 0.7.0
— deliberately, because `cargo-semver-checks` had flagged breaking changes in
that crate and only that crate, and bumping just the crate that broke keeps the
check meaningful. That split was always documented as something the next real
release had to reconcile, and this is that release.

### Removed

- **The `grpc-legacy-json` feature and the pre-0.7 JSON-tunnel gRPC service.**
  Releases before 0.7 tunneled JSON inside a protobuf `bytes` envelope on a
  non-standard service (`a2a.v1.A2aService`), served alongside the canonical
  binding behind an off-by-default feature so 0.6 clients survived a rolling
  upgrade. That window has closed. Gone with it: the feature on both
  `a2a-protocol-server` and `a2a-protocol-sdk`, `dispatch/grpc/service.rs`,
  `GrpcDispatcher::into_legacy_service`, the `LegacyA2aServiceServer` /
  `LegacyGrpcServiceImpl` re-exports, the JSON codec helpers
  (`encode_json`/`decode_json`/`reader_to_grpc_stream`), the `a2a.v1` proto and
  its build-script compile step, and the coexistence test.

  **Migration:** none is available in-process — a 0.6 client must move to the
  canonical `lf.a2a.v1.A2AService`, which is the protobuf-native binding the
  official Go, Python and Java SDKs already speak. `GrpcDispatcher::serve` and
  `into_service` are unchanged for everyone already on it.

- **`RequestHandlerBuilder::with_event_queue_write_timeout` and
  `EventQueueManager::with_write_timeout`**, both deprecated no-ops since 0.7.
  Event-queue writes never block — the queue is a broadcast channel, so a slow
  streaming consumer receives an explicit lag error on its reader rather than
  exerting backpressure on the executor — so neither setter ever had an effect.

  **Migration:** delete the call. There is no replacement because there was no
  behaviour. To bound a slow consumer, size the queue with
  `with_event_queue_capacity` and handle the lag error on the reader.

  Said plainly rather than left to be discovered: the two *public setters* are
  gone, but the value they set is still threaded through
  `new_in_memory_queue_with_options` into an `#[allow(dead_code)]` field on
  `InMemoryQueueWriter`, and `DEFAULT_WRITE_TIMEOUT` is still exported. Those
  were not on the announced removal list, and changing that constructor's arity
  would be an unadvertised API break on top of an advertised one. The dead
  plumbing is tracked in `ROADMAP.md` for a later release.

- **The bare `a2a-notification-token` header from the default CORS
  `allow_headers`.** `x-a2a-notification-token` is canonical and stays; the
  unprefixed spelling was this SDK's own pre-0.7 name.

  **Migration:** a webhook receiver still reading the bare header can restore
  it by assigning to `CorsConfig::allow_headers` — it is a plain `String` and
  always was, so this changes a default, not a capability.

### Added

- **Act 5 grows to sixteen checks: every SDK capability that had no example now
  has one.** The previous entry left eight capabilities demonstrated by no
  example — measured by grep, not recalled — each for a stated reason. All eight
  are now covered, and each was proven able to fail by removing what it covers:

  | Capability | Demonstrated by | Proven failure |
  |---|---|---|
  | `ApiKeyAuthInterceptor` | Custom header, three cases (absent / wrong / right key) | `a request with no key SUCCEEDED` |
  | `JwtAuthInterceptor` via remote JWKS | ES256 tokens against a JWKS the example serves on a loopback socket | `a token signed by an unpublished key was ACCEPTED` |
  | `HandlerLimits` | `max_id_length` against a caller-controlled `context_id` | `a 33-char context_id was accepted with max_id_length 32` |
  | `RetryPolicy` (client) | Fault-injecting reverse proxy in front of a real agent | `2 injected 503s were not ridden out` |
  | `TenantAwareSqliteTaskStore` | Two tenants written, handler replaced, both read back | `after the restart acme can see globex's task` |
  | `PostgresTaskStore` | Round-trip through two handlers over one database | `task … did not survive the handler change` |
  | `init_otlp_pipeline` | A collector socket the example owns, asserting HTTP/2 bytes arrive | `3 requests recorded but the collector received 0 bytes` |
  | `HttpPushSender::with_tls_config` | `rcgen` cert + `tokio-rustls` sink, delivered with both trust stores | `delivery to a certificate the sender was told to trust failed` |

  The retry check is the one worth reading: it asserts a `503` on
  `SendMessage` *is* retried and a `502` is *not*. `503` means the request was
  refused up front, so re-sending a non-idempotent method is safe; `502` means
  a gateway may already have forwarded it, so retrying can create a second
  task. A retry layer that treats every 5xx alike passes both other assertions
  and fails only that one.

  New reporting state: `[NOT RUN]`, distinct from `[NOT BUILT]`. "The binary
  cannot do this" and "the binary can and nobody tried" are different facts.
  Only the PostgreSQL check can be `[NOT RUN]` (it names
  `A2A_TEST_POSTGRES_URL`); CI provides the service, and
  `INCIDENT_REQUIRE_ALL=1` makes either state exit `4` so a service that stops
  being provisioned fails the job instead of silently downgrading a check to a
  printed line.

  Two failures reported while writing these were the checks' own fault, not the
  SDK's, and are recorded because the distinction is the point: a JWT that
  expired 30 seconds ago is *correctly* accepted inside `JwtValidator`'s
  60-second clock-skew leeway, and a proxy that forwards a request without its
  headers strips `A2A-Version` and earns a `-32009`.

- **`examples/incident-response` now demonstrates the SDK capabilities a
  deployment needs, over a socket, with assertions.** Tenant isolation,
  authentication interceptors, rate limiting, persistent stores, agent-card
  signing, the `Metrics` hook, OpenTelemetry export and graceful shutdown all
  shipped and were covered by unit and integration tests, but no example
  exercised any of them end-to-end. Act 5 runs eight checks, each naming the
  specific wrong answer it rules out:

  | Check | Fails when |
  |---|---|
  | Tenant isolation (`TenantAwareInMemoryTaskStore` + `HeaderTenantResolver`) | A tenant sees another's task, or a caller authenticated as one tenant writes into another by naming it in `params.tenant` |
  | `BearerTokenAuthInterceptor` | An anonymous request succeeds, **or** a correctly authenticated one is refused |
  | `RateLimitInterceptor` | Every call is accepted, every call is refused, or more than the limit gets through |
  | `sign_agent_card`/`verify_agent_card` | A card whose interface URL was rewritten still verifies |
  | `SqliteTaskStore` | A task written through one handler is not readable through another over the same file |
  | `RequestHandler::shutdown` | It does not return within 5s |
  | The `Metrics` hook | Served requests reach no recorder |
  | `OtelMetrics` | No `a2a.server.requests` datapoint is collected after N requests |

  Runnable on its own as `cargo run -p incident-response -- harden` (exit code
  `3` on a failure), gated by its own step in `ci.yml`'s `example-surface` job,
  and proven able to fail by `scripts/prove_gates_fail.sh`, which removes the
  tenant resolver — a defect under which every request still succeeds and only
  the isolation check notices.

  Two details were verified rather than assumed. The OTel check collects from a
  real `ManualReader` rather than trusting the global provider, which defaults
  to a no-op under which a handler that records nothing looks identical to one
  that records everything. And the signing check tampers with
  `supported_interfaces[0].url`, not the deprecated top-level `AgentCard::url`
  — the latter is `#[serde(skip_serializing)]` because A2A v1.0 removed it, so
  it is absent from the canonical signing payload and rewriting it correctly
  changes nothing.

  Capabilities behind Cargo features print `[NOT BUILT]` with the feature they
  need instead of vanishing, so `--no-default-features` reports a narrower run
  as narrower (measured: 5 passed, 3 not compiled) rather than printing the
  same "all passed" line over fewer checks. `incident-response` gains
  `default = ["sqlite", "signing", "otel"]` so the documented command exercises
  everything.

- **All six examples now drive every A2A method over every binding they serve,
  and fail if the matrix has a gap.** Measured 2026-08-11, all six report
  **44 of 44 cells**. Before the change: `echo-agent` 4 methods / 2 bindings,
  `incident-response` 4 / 1, `genai-agent` 0 / 1 (it printed a URL and waited),
  `rig-agent` 0 / 1, `multi-lang-team` 1 / 1, and `agent-team`'s 100 feature
  tests had no rows for the question at all.

  `agent-team` gains a fifth agent that exists purely to be swept, because its
  four team agents are deliberately split by binding and no single one of them
  can answer the coverage question.

  Three examples depend on something CI does not have — an LLM provider, or
  worker agents in four other languages — and each reports that separately
  rather than letting a full matrix imply it: `genai`/`rig` print
  `LLM leg: NOT EXERCISED` and label every fallback answer
  `[no model reachable — mechanical fallback, not an LLM answer]`;
  `multi-lang-team` prints each worker as `REACHABLE` or `not reachable` and
  its artifact says `[no worker agents reachable — nothing was delegated]`.
  The fallbacks are opt-in per handler: server mode still fails the task on a
  provider error, which is correct for a real agent.

  `multi-lang-team`'s coordinator now probes workers once at startup instead of
  fanning out per request. With all four down every call paid a full timeout
  window, which made the sweep take minutes and told the reader nothing they
  had not been told at startup.

- **Both examples now drive every A2A method over every binding they serve, and
  fail if the matrix has a gap.** Measured 2026-08-11 before the change:
  `echo-agent` drove 4 of the 11 methods over 2 of the 4 transports, and
  `incident-response` 4 over 1 — while `examples/README.md` presented the first
  as demonstrating "the complete request lifecycle". `echo-agent`'s card also
  advertised neither push notifications nor an extended card, so seven methods
  were not merely undriven but unavailable on the server it started.

  Both now report 44 of 44 cells and exit 2 on any gap. `echo-agent` serves
  gRPC for the first time; `incident-response` serves REST, gRPC and WebSocket
  for the first time, on `port`, `port + 10` and `port + 20`. The narrative
  Acts 1-3 are unchanged — the sweep is a fourth act, so the example still
  teaches what an agent is before it measures what the SDK covers.

  Both also run counter-tests against a second agent advertising no optional
  capabilities, because a full matrix only shows the server says yes to
  everything it should. Five refusals are checked by error code: unknown task
  ids on `GetTask` and `CancelTask`, push config and extended card against a
  card lacking those capabilities, and streaming against an agent that never
  advertised it.

  Gated by `ci.yml`'s `example-surface` job, registered in `preflight.sh` and
  `prove_gates_fail.sh`. Proven able to fail: dropping the `ListTasks`
  recording from the shared harness, with every call still succeeding, exits 2
  and names all four cells.

- **`a2a-example-harness`** — the coverage matrix, the method sweep and the
  counter-tests, shared by both examples rather than copied into each. A
  duplicated scorer is one that eventually disagrees with itself: one copy
  loses a row and the example built on it reports a full matrix.

- **The SDK dogfood suite now runs in CI, and its feature table is computed
  from results.** `examples/agent-team` holds ~5,900 lines of end-to-end tests
  and had never been executed by any workflow: it is a `main()`, not
  `#[test]`s, so `cargo test --workspace` compiled it and ran none of it, and
  it appeared in the workflows only inside `cargo package --exclude` lists.
  Measured when first run locally on 2026-08-11: **86 tests, 71 passed, 15
  failed, exit 1**.

  Three independent defects kept that invisible, and all three are fixed:

  * **The feature table could not fail.** "SDK FEATURES EXERCISED" was a
    hardcoded array printed as `[x] <label>` in a loop, with no connection to
    any result. The failing run still printed `[x] Batch JSON-RPC (single,
    multi, empty, mixed, streaming rejection)` with all six batch tests red.
    Feature-gated labels were `#[cfg]`'d out of the *array*, so a build
    without `--features websocket` omitted the row entirely rather than
    reporting it unexercised — absence rendered as completeness. The table now
    lives in `examples/agent-team/src/features.rs`, where each claim names the
    tests that evidence it and renders `[x]`, `[FAIL]` or `[ ] NOT RUN` from
    their outcomes. A bidirectional drift check fails the run if a claim names
    no test that ran, names a test that does not exist, or if a test ran that
    no claim mentions.
  * **No `default` features.** `cargo run -p agent-team` — the command
    `examples/README.md` gave for "exercises every SDK feature" — compiled out
    WebSocket, gRPC, Axum, SQLite, signing and OTel, then printed "SDK dogfood
    complete". There is now a `default` set covering all six, and the binary
    exits 2 listing every unexercised area rather than claiming completion.
  * **Nothing ran it.** `ci.yml` gains a `dogfood` job running the suite with
    `--all-features`, registered in `scripts/preflight.sh` and
    `scripts/prove_gates_fail.sh` so the existing bidirectional job-coverage
    guard accounts for it.

  With all features enabled the suite is now **100 tests, 100 passing**.

- **`A2aError::is_stream_lagged`, `dropped_event_count`, `stream_lagged`, and
  `STREAM_LAGGED_MARKER`** (`a2a-protocol-types`), plus
  `ClientError::is_stream_lagged` / `dropped_event_count`
  (`a2a-protocol-client`).

  A server emits a recoverable lag signal when a streaming consumer falls
  behind; the stream continues and a consumer that keeps polling still
  receives later events. Recognising it required matching the raw
  `data.streamLagged` JSON key by hand, because the only predicate was
  `pub(crate)` inside `a2a-protocol-server`. Every out-of-tree consumer faced
  that, and the dogfood suite's own backpressure test got it wrong — it broke
  out of the read loop on the lag signal, then asserted on an event it had
  just stopped waiting for. The marker and message now have one definition in
  the types crate, which the server's `lag_error`/`is_lag_error` delegate to.

- **`extended-card-requires-auth`** (dogfood Test 68b) — asserts that an agent
  advertising `extendedAgentCard` with no authenticating interceptor refuses
  to serve it (spec §13.3). This path is bypassed on the shared analyzer agent
  by `allow_unauthenticated_extended_card()`, so without a dedicated test the
  suite would prove the card is served and never that it is protected.

- **`BIND-EQUIV-004` is now graded behaviourally, not just structurally.**
  §5.1 requires every binding to support the same authentication schemes. The
  runner has been checking the half a card can answer — v1.0 gives
  `AgentInterface` no security fields, so schemes declared once at card level
  bind every binding. Whether each binding *enforces* them was recorded as
  unmeasured, because no target here required credentials to withhold.

  `tck/sut` gains `SUT_PROFILE=secured`, whose card declares a bearer scheme
  and whose handler enforces it with a single `BearerTokenAuthInterceptor`.
  One interceptor above the dispatchers is the property under test: all four
  bindings are guarded by one implementation reading one `CallContext`, so a
  binding that stopped forwarding the header would fail rather than quietly
  serve anonymous traffic. `a2a-tck --equivalence --auth-token <t>` sweeps
  twice — every binding must refuse an uncredentialed request, and every
  binding must serve a credentialed one — and `tck.yml` gates both.

  The acceptance sweep caught a defect in the probe itself during development:
  an early draft sent the JSON-RPC method as `tasks/list` where this SDK's name
  is `ListTasks`, which authenticated and then failed dispatch, so two bindings
  reported a refusal and the check declared an asymmetry that did not exist. On
  the rejection sweep alone, a probe that can never succeed is
  indistinguishable from enforcement working. Recorded in
  `book/src/reference/conformance-history.md` rather than quietly fixed.

- **Four governance files**: `MAINTAINERS.md`, `.github/CODEOWNERS`,
  `SUPPORT.md` and `TRADEMARKS.md`. `GOVERNANCE.md`'s duplicated maintainer
  table is removed in favour of `MAINTAINERS.md`, so the two cannot drift.
  `CODEOWNERS` states in its own header that with one maintainer it is inert
  for that maintainer's own pull requests — real for outside contributions,
  and not a claim that these paths are independently reviewed today.

- **`docs/provenance-manifest.md` and `scripts/provenance_manifest.sh`** — the
  reproducible account of what this repository's history contains and what a
  DCO history rewrite would cost, for a downstream project's counsel. The
  script refuses to run on a shallow clone, which is what produced two
  successive wrong measurements.

### Fixed

- **The per-PR mutation gate could not see 41 of 141 library source files.**
  `mutants.yml`'s `Build PR source diff` step scoped with
  `git diff -M … -- 'crates/*/src/**/*.rs'`. A git pathspec is matched by
  `fnmatch` *without* `FNM_PATHNAME` unless it carries `:(glob)` magic, so `*`
  crosses `/` and the `**/` collapses to "one or more directories" — the
  literal slash after it still has to match something. The pattern therefore
  reached only files in a *subdirectory* of `src/`, and never a file sitting
  directly in `crates/<crate>/src/`. Measured with `git ls-files` at `6ebf821`:
  100 files matched, against **141** for `:(glob)crates/*/src/**/*.rs`.

  The 41 invisible files include `rate_limit.rs`, `serve.rs`, `builder.rs`,
  `executor.rs`, `method.rs`, `signing.rs`, `client.rs`, `retry.rs` and all
  four `lib.rs`. When the diff came back empty the job set `skip=true`, its own
  `if:` skipped the mutation step, the check went green, and the summary
  printed "No Rust source files changed in `crates/*/src/` — nothing to
  mutate", which was untrue.

  Proven on real history rather than a synthetic defect: nine of the last 120
  commits changed `crates/*/src` sources *only* in the invisible set. Replaying
  the step gives `ADDED` = 0 for `a9e1235` (a `rate_limit.rs` fix), `a116bc5`
  (another) and `e6aa9e1` (a feature commit) under the old pathspec, and 57,
  24 and **376** under the fixed one.

  The weekly full sweep is unaffected, and that was checked rather than
  assumed: it never passes `--in-diff`, and its `examine_globs` are consumed by
  cargo-mutants' glob crate where `**/` does mean "zero or more directories" —
  demonstrated by run `31352927429` finding 6 survivors in `rate_limit.rs`, a
  top-level file. So the headline mutation score is unchanged; what was
  narrower than it looked is every green *incremental* check on a PR touching
  only top-level modules.

- **The WebSocket client forwarded its end-of-stream control frame to the
  consumer.** The binding closes a stream with
  `{"result":{"status":"stream_complete"}}`; the reader task wrapped *every*
  frame as an SSE line and delivered it, consulting `is_stream_terminal` only
  afterwards for pending-map cleanup. The sentinel is not a `StreamResponse`,
  so it surfaced to callers as
  `unknown variant 'status', expected one of 'task', 'message', 'statusUpdate', ...`.

  The common case hides it: when a task reaches a terminal state the stream
  ends on that event and the sentinel is never parsed. It only bites when a
  stream ends *without* a terminal state — most obviously a task parked in
  `INPUT_REQUIRED`, i.e. any agent that asks a clarifying question over
  WebSocket. Found by driving all eleven methods against exactly such an agent
  while expanding `incident-response`; `echo-agent`, whose tasks always
  complete, never hit it.

  Fixed with a narrow `is_stream_complete_sentinel` consulted *before*
  delivery. Deliberately narrower than `is_stream_terminal`, which also treats
  a terminal task status as end-of-stream: suppressing those would truncate
  every stream at its most important frame, a worse bug than the one being
  fixed. Three regression tests pin the split, including one asserting the
  sentinel genuinely cannot deserialize as a `StreamResponse`.

- **The 15 failing dogfood tests**, all in the suite rather than the SDK, with
  one exception noted below. Root causes, each verified against a live server
  rather than inferred:

  * **9 tests** — `post_raw`/`get_raw` sent no `A2A-Version` header. The server
    negotiates the version per request and defaults to `0.3`, so every raw-HTTP
    test was asserting against a `VERSION_NOT_SUPPORTED` envelope instead of a
    real response. Both helpers now send it, sourced from
    `A2A_VERSION_HEADER`/`A2A_VERSION` so they cannot drift, and both return
    `Err` on a version rejection so the failure names its own cause.
  * **3 tests** (`resubscribe-rest`, `resubscribe-jsonrpc`, `push-delivery-e2e`)
    — each read one event and pattern-matched `StatusUpdate` to get a task id.
    The first stream event is a full `Task` snapshot; the observed order is
    `task` → `artifactUpdate` → `statusUpdate`. All three had never passed.
    Replaced with a shared `helpers::first_task_id`, which reads until an event
    names a task — the same correction already applied to `BIND-EQUIV-003`.
  * **`extended-agent-card`** — the only one that was a genuine configuration
    gap rather than rot: the analyzer's card never set
    `extended_agent_card`, so the handler correctly answered
    `UnsupportedOperation` per spec §3.1.11 and the test could not have passed
    on any commit. The capability is now advertised and the §13.3
    authentication requirement satisfied explicitly.
  * **`backpressure-lagged`** — asserted that a lagged consumer still receives
    the terminal event. It does not, by design: the SDK's own lag message says
    to resubscribe, and events are dropped for that consumer only while the
    store stays authoritative. The test now asserts that invariant — that
    backpressure costs events, never the task — by confirming via `GetTask`
    that the task still reached `Completed`.

- **A vacuous pass.** `wire-part-flat-oneof` asserts the *absence* of a
  `"type":"text"` discriminator, and passed against the version-error envelope,
  which also lacks it. Verified by injection: with the header removed it now
  reports `[FAIL] … request failed: server rejected protocol version …`
  instead of `[PASS] response uses v1.0 flat Part…`.

- **WebSocket dogfood tests aborted the process.** Tests 51-60 are
  `#[cfg(feature = "websocket")]`, and with no `default` features and no CI job
  they had never been compiled by an automated run. Enabling them showed the
  handshake omitted `A2A-Version` (spec §3.6.1 — for a WebSocket that is the
  upgrade request), and the test `.expect()`ed on connect, so one broken test
  killed the other 86. The handshake now carries the header, mirroring
  `a2a-protocol-client`'s own WebSocket transport, and the panic sites are
  `TestResult::fail`.

- **`push-delivery-e2e` asserted nothing about push delivery.** It passed on
  `task_id.is_some()`, which every other streaming test already proves, and
  would have stayed green with push entirely broken. It now requires the
  webhook to have actually received something — currently 6 events.

- **`examples/agent-team/src/tests/mod.rs` omitted the `transport` module**
  from its own module list, so the numbering jumped 41-50 → 61-90 and the only
  evidence that Tests 51-60 existed was the gap. `examples/README.md`'s
  "exercises every SDK feature with 81+ automated tests" and echo-agent's
  "complete request lifecycle" are corrected to what is measured.

- **`PROVENANCE.md` §2.1's commit counts were wrong**, measured on a shallow
  clone. Over the full 648 non-merge commits the DCO verdict is 126 pass / 477
  assistant-authored / 29 bot / 16 unsigned human — not 120 / 138 / 21 / 3, and
  the pass rate is 19%, not 43%. §2.1 also said §1's figure of 608 commits
  "could not be reproduced"; on a complete clone it reproduces exactly, and §1
  needed no change. The failure mode is worth knowing: a shallow clone drops
  the oldest commits, which here are precisely the non-compliant ones, so the
  pass count survives intact while every failure count shrinks. It reads as
  good news.

- **The 500-line file-length ratchet now covers `.sh` and `.py`**, not `.rs`
  alone. Widening turned up two over-limit scripts nobody had counted, which is
  the argument for a ratchet over a one-time split. Baseline 77 → 81 entries.

- **ADR 0006 no longer says developers must address all surviving mutants
  before merge.** The gate is `--in-diff`; the obligation is the lines a PR
  changes. `CONTRIBUTING.md` was corrected earlier and the ADR was not, leaving
  a governing document contradicting the enforced rule. Its stale 92% / 183
  figure is now the measured 94% / 125.

- **The verdict-bearing steps outside `ci.yml` are now proven able to fail.**
  `scripts/prove_gates_fail.sh` covers `ci.yml`'s eight gate jobs by injecting
  defects into tracked source and running cargo; that mechanism does not reach
  the other ten workflows, whose gates decide over *data* — a conformance
  report, a directory of mutation artifacts, a git range, a tag name.

  `scripts/prove_workflow_gates_fail.py` runs each step's real body from the
  real workflow file against synthetic healthy and defective inputs: 17 gates
  proven, 11 exempt with recorded reasons, 0 unproven, ~7s. It runs on every
  PR in the `fmt` job, and drift is a hard error both ways — a step that can
  fail with no registry entry, and a registry entry naming a step that no
  longer exists, both exit 2.

  It reproduces GitHub's shell mapping exactly (`bash -e` without a `shell:`
  key, `bash -eo pipefail` with `shell: bash`), because those two disagree
  about the defect it was written for, and it asserts each gate exits 0 on
  healthy input as well — a gate that fails unconditionally would otherwise
  score as proven while blocking every legitimate run.

- **The TCK drives all four bindings, and grades §5.1 equivalence.** It ran
  JSON-RPC and HTTP+JSON only; `--binding websocket` (§12) and
  `--binding grpc` (§10) complete the set, and `--equivalence` grades
  `BIND-EQUIV-001..004` — the four §5.1 `MUST`s that are statements about the
  *relation* between bindings and so cannot be graded one binding at a time.
  Upstream has carried them as `NOT TESTED` since April.

  The gRPC checks are separate assertions rather than a ProtoJSON adapter over
  the JSON ones. Converting protobuf responses and reusing the JSON assertions
  would be less code and would produce checks that cannot fail:
  `task_state_values` asserts the state is one of nine `TASK_STATE_*` strings,
  and over gRPC that string exists only after this crate's own enum-to-name
  mapping runs, so the assertion would be about the converter. `tck/sut` now
  serves all four bindings (`SUT_GRPC_HOST`, `SUT_WS_HOST`).

  `BIND-EQUIV-004` is graded structurally only, and says so in its output:
  the card declares security once at card level and no interface may override
  it, but proving each binding *enforces* those schemes identically needs a
  target configured to require credentials.

- **`SignatureHeader` and `signature_header()` in `a2a-protocol-types`** —
  decodes an `AgentCardSignature`'s JWS protected header, exposing `alg`,
  `kid` and `jku` so a caller can determine *which* key to retrieve before
  verifying. It performs no cryptographic verification, and the header is
  attacker-controlled until `verify_agent_card` succeeds against a trusted
  key — the docs say so at the call site. Key expiry and revocation
  (`CARD-SIGN-004`) remain the caller's responsibility, since neither is
  carried in the header; this provides the `jku`/`kid` needed to go and
  check them.

- **Unrecognised request parameters are now logged instead of vanishing.** The
  specification requires implementations to *ignore* unknown fields for
  forward compatibility (§11; the official TCK grades this as
  `DM-SERIAL-005`), so `ListTasks` with `{"contxtId": …}` must keep returning
  every task rather than erroring. It no longer does so invisibly: the
  JSON-RPC binding diffs each request's top-level keys against the ones the
  method understands and warns about the difference, naming them. Honouring
  §11 on the wire and telling the operator what was discarded are not in
  conflict.

  The accepted-key lists live on a new `AcceptedFields` trait and are verified
  against `a2a.proto` by `proto_field_alias.rs`, so a list that drifts short
  (spurious warnings) or long (silenced real ones) fails the build.

- **The TCK SUT takes a `SUT_PROFILE`.** `CORE-CAP-001/002/004` check that a
  server rejects push and streaming operations it never advertised, and the
  suite skips them against an agent that does advertise them — so those paths
  were unreachable from a single SUT, and the gate could never have caught a
  regression in `ensure_streaming_supported` / `ensure_push_supported`.
  `SUT_PROFILE=minimal` advertises nothing; CI runs the suite once per profile
  and gates both. `CORE-CAP-001` and `CORE-CAP-002` now pass rather than skip.

- **The TCK SUT serves gRPC.** Its agent card advertised only `JSONRPC` and
  `HTTP+JSON`, and the TCK builds one client per advertised interface — so the
  whole `GRPC-*` family and every core requirement's gRPC leg reported
  `SKIPPED` while this SDK shipped a gRPC binding. The suite went from
  `176 passed / 89 skipped` to **`244 passed / 21 skipped`, still 0 failed**,
  and MUST-level `SKIPPED` from 11 to 5. The binding passed everything on
  first exposure, so this closed no defects — it closed a hole in what the
  score was measuring. `SUT_GRPC_HOST` overrides the port.

### Changed

- **`AgentCard.url` is no longer emitted.** It is the v0.3 top-level URL; the
  v1.0 `AgentCard` has no `url` field, `supportedInterfaces` replaced it, and
  emitting it made this SDK's card fail the specification's own JSON schema
  (`'url' does not match any of the regexes: …`) — reported by `CARD-EXT-001`
  on both JSON bindings while gRPC passed, since protobuf cannot carry a field
  the schema does not define.

  The field is still **parsed**, so a card published by a v0.3 peer still
  loads; the reference implementation does the same, folding `url` into
  `supportedInterfaces`. To publish an agent's address, use
  `supported_interfaces`. Same policy as the `securitySchemes` change below:
  accept both vintages, emit only v1.0.

### Fixed

- **The official-TCK conformance gate could not fail.** The step
  `official-tck.yml` calls "THE GATE" ended `| tee /tmp/tck-gate.log`, and
  GitHub's default shell for a `run:` step with no `shell:` key is
  `bash -e {0}` — `-e` but not `-o pipefail`. The step's exit status was
  `tee`'s, which is 0 whatever the checker decided.

  Measured by running the step body verbatim: against an all-SKIPPED report
  the checker exits 1 and prints `UNDER-MEASURED — 0 MUST requirement(s)
  graded, floor is 88`; against a report carrying an injected MUST failure it
  exits 1 and prints `REGRESSION`. Both step bodies exited 0. With
  `set -o pipefail` both exit 1, a healthy report still exits 0, and the log
  the `tee` exists to write is still written.

  `--min-graded` was added so a run that measured nothing could not pass this
  gate; it could never fire. A guard behind a broken gate is not a guard. The
  minimal- and extension-profile suite steps carried the same masking, so
  their `|| suite_status=$?` never fired and their closing
  `exit "$suite_status"` was always 0 — requirement-level regressions were
  still caught there by the un-piped gate steps that follow, suite-level
  failures were not. Enabling pipefail does not turn these red: all three
  suites were run directly and exit 0.

- **A conformance check passed two bindings it never ran against.**
  `jsonrpc_envelope_format` returned `Ok(())` for `rest` and `websocket`
  rather than declaring itself inapplicable, so a `rest` run reported
  `22/22 passed` while 21 checks had actually run. Not-applicable is now a
  reported outcome, printed with its reason and excluded from the
  denominator, and a run that grades zero checks exits 2 — every check being
  skipped is a run that verified nothing, not a pass.

- **`scripts/preflight.sh` silently skipped any non-cargo gate.** It matched
  `run: cargo ` when collecting CI's gate steps, so the first gate invoking a
  script rather than cargo would have been dropped without a word. It now
  matches every `run:` step in the gate jobs and cross-checks both
  directions. 31 gates, up from 28.

- **The 500-line file limit was an unenforced checklist line.** CONTRIBUTING
  claimed 46 of 139 files exceeded it; the tree had 77 of 310.
  `scripts/check_file_lengths.sh` pins the current count so it cannot grow,
  and CI runs it.

- **`release.yml` accepted lightweight tags.** A release cut from one carries
  no tagger, date, or message, and cannot carry a signature. The workflow now
  requires an annotated tag and fails with the `git tag -a` invocation to fix
  it.

- **A `SubscribeToTask` stream ended when the executor exited, not when the
  task finished.** Spec §3.1.6: "The stream MUST terminate when the task
  reaches a terminal state." A task's event queue lives exactly as long as one
  executor invocation, so an agent that parked a task in `input_required`
  destroyed the queue at every turn boundary — closing the stream while the
  task was still running, having reported no terminal state at all.

  `InMemoryQueueReader` gained an optional reattach hook, consulted when its
  channel closes: terminal task → emit a `TaskStatusUpdateEvent` carrying that
  status and end; task gone → end; still running → wait for the next turn's
  queue and continue. The send path, executor lifecycle and queue manager are
  untouched.

  Two new `HandlerLimits` bound the wait: `subscribe_reattach_interval`
  (250 ms) and `subscribe_max_idle` (5 min), after which the stream ends and
  the client may resubscribe per §3.5.2.

  One in-repo test asserted the defect as a requirement and was rewritten.

  This closes the last baselined failure: the official TCK now reports
  **176 passed, 0 failed**, MUST compatibility **100%**, and
  `tck/conformance-baseline.json` is empty. That figure covers *tested*
  requirements only — 21 MUST requirements still report `NOT TESTED` because
  the SUT does not exercise them, and are a coverage gap rather than a pass.

- **A blocking `SendMessage` could never return a direct `Message`.** Spec
  §3.1.1 allows either a `Task` or a `Message`; `SendMessageResponse::Message`
  was never constructed anywhere in the server, so an agent that replied with
  a message got back a task stuck in `Submitted`. The response is now a
  `Message` when the executor produced a message and nothing task-shaped —
  narrower than the reference, which errors on mixed streams and would break
  agents that narrate progress. Closes `DM-MSG-001`.

### Changed

- **`AgentCard.securitySchemes` is now emitted in the v1.0 wire shape.**
  `SecurityScheme` is a protobuf `oneof` in `a2a.proto`, so its ProtoJSON
  encoding is a single-key object naming the arm
  (`{"apiKeySecurityScheme": {"location": "header", …}}`). This SDK emitted the
  v0.3 OpenAPI-style form instead (`{"type": "apiKey", "in": "header", …}`).

  A reference `a2a-sdk` client parsed that via its own legacy-compatibility
  shim, so this was not an interop break with the reference. But a peer feeding
  the card straight to `ParseDict(…, ignore_unknown_fields=True)` — the same
  option the reference resolver itself passes — got schemes with **empty
  contents**, and would conclude the agent supports no usable authentication.
  That is the silent-wrong-data failure mode again, pointed the other way
  across the wire. Verified by feeding this SDK's own card bytes to three
  reference parsers.

  **Both encodings are accepted** (the v1.0 form under either the `json_name`
  or the proto field-name spelling of the arm, and the v0.3 form), and a v0.3
  scheme normalises to the v1.0 form on re-emission. `ApiKeySecurityScheme`
  emits `location` — the proto field name — with `in` kept as an alias.

  This is a breaking change to a published wire format: the bytes on
  `/.well-known/agent-card.json` change. It only affects a consumer reading the
  card's raw JSON keys rather than parsing it with an A2A implementation, and
  deserialization is strictly more permissive than before. Five in-repo tests
  asserted the old encoding and passed confidently; they now assert the v1.0
  encoding plus v0.3 acceptance. See `docs/official-tck-findings.md` §7.

### Fixed

- **`SendMessageConfiguration.taskPushNotificationConfig` was parsed and
  dropped, so no webhook was ever registered or delivered.** The schema is
  explicit that this is how a client subscribes at send time — *"Task id should
  be empty when sending this configuration in a `SendMessage` request"* — and
  the reference implementation registers it before the executor starts. This
  SDK deserialised the field and never looked at it again:
  `ListTaskPushNotificationConfigs` came back empty and no notification fired.

  The config is now registered against the created task, with `taskId` filled
  in server-side, at the point the task is saved and before the executor is
  spawned, so the first status transition is already covered. Registration
  reuses the standalone `CreateTaskPushNotificationConfig` validation —
  capability check, task existence, SSRF screening, per-task and global quotas
  — rather than writing to the store directly, so the inline path cannot become
  an unguarded back door. Four counter-tests drive each of those guards to a
  failure through the inline path specifically.

  Against the official TCK this closes all six `PUSH-DELIVER-001/002/003` legs
  across both bindings, taking the run from 166/10 to **172/4** and MUST
  compatibility from 93.9% to **97.6%**. The baseline shrinks from 9 pairs
  across 5 requirements to 3 across 2.

  **Behaviour change:** a `SendMessage` carrying an inline push config against
  a server with no push support now fails with `PushNotSupported` instead of
  succeeding and ignoring the config. The reference skips silently here; this
  SDK does not, on the same reasoning as the alias fix below — a client that
  asked for notifications and will never receive any should be told.

- **Request parameters spelled with the protobuf field name were silently
  ignored, turning a filter into wrong data.** The A2A JSON model is generated
  from `a2a.proto`, and protobuf's canonical JSON mapping requires parsers to
  accept **both** the proto field name (`context_id`) and its `json_name`
  (`contextId`). This SDK accepted only the latter and ignored the former, so
  `ListTasks` with `{"context_id": "ctx-A"}` returned **every** task instead of
  the 3 in that context — no error, just the wrong answer. The official TCK
  sends the snake_case spelling for six fields; the reference implementation
  (`a2a-sdk` 1.1.2, whose types *are* protobuf messages) accepts both.

  All 73 multi-word fields across the 44 schema messages now accept both
  spellings and still emit only the `json_name`, per spec §5.5.

  The alias list is derived from `a2a.proto` rather than from the Rust field
  names — deriving it from the Rust names would prove only that the aliases
  match the Rust identifiers, and would go stale silently as the schema grows.
  `proto_field_alias.rs` parses the schema, computes camelCase with protobuf's
  own `ToJsonName` rather than serde's `rename_all`, and asserts per field that
  both spellings parse to the same value, that the sample used is
  distinguishable from the field being absent, that only the `json_name` is
  emitted, and that every schema field is covered or explicitly exempt. Five
  counter-tests drive each direction to a deliberate failure.

  Against the official TCK this closes 7 MUST-level (requirement, transport)
  pairs — `CORE-HIST-002`, `PUSH-CREATE-001/002`, `PUSH-DEL-001/002`,
  `PUSH-GET-001`, `PUSH-LIST-001` — taking the run from 158/12 to 166/10 and
  MUST compatibility from 85.4% to 93.9%. `tck/conformance-baseline.json`
  shrinks from 16 pairs across 12 requirements to 9 across 5.

  **Not fixed, and deliberately not fixable this way:** a parameter misspelled
  any *other* way (`contextID`, `contxtId`, `pagesize`) is still ignored.
  Rejecting unknown fields would close it, and was implemented and then
  reverted — the specification says implementations **SHOULD** ignore
  unrecognized fields for forward compatibility (§11), and the official TCK
  grades that as `DM-SERIAL-005`, which the change failed on both bindings.
  Pinned by `unrecognised_fields_are_ignored_not_rejected` so it is not
  re-attempted; see `docs/official-tck-findings.md` §3.3(b).

  Two deliberate divergences are recorded rather than papered over: a request
  carrying *both* spellings of one field is rejected here (`-32602 duplicate
  field`) where the reference takes the last key, and `AgentCard.securitySchemes`
  is still emitted in the v0.3 shape (§7 of the same document).

- **Mid-stream SSE errors escaped their JSON-RPC envelope, so no conformant
  client could read them.** Success frames on the JSON-RPC binding are wrapped
  in a `JsonRpcSuccessResponse` (§9.4.2), but error frames were emitted as a
  bare `A2aError` — a payload carrying neither `result` nor `error`. This
  SDK's own client reported

      Serialization("JSON-RPC 2.0 response carries neither `result` nor
      `error`; §5 requires exactly one")

  instead of the error the server was sending, which made the 0.7.0
  `streamLagged` truncation signal **unreadable over JSON-RPC**: a consumer
  that fell behind learned only that the frame was malformed, not that its
  view was truncated or that it should resubscribe. Error frames are now
  `JsonRpcErrorResponse` envelopes echoing the request id, with `error.data`
  preserved so the `streamLagged` marker survives. The REST binding keeps
  bare payloads per §11.7, asserted so the fix cannot leak across bindings.

  A second site had the same defect: the serialization-failure fallback
  emitted an ad-hoc `{"error":"serialization failed: …"}` string, which is
  neither a JSON-RPC error response nor an `A2aError`. Both sites now share
  one enveloping helper.

  Surfaced by `cargo bench -p a2a-benchmarks --bench backpressure`, which
  panicked at `stream_volume/502_events`. Confirmed pre-existing: the same
  panic reproduces on `b416c1a` (v0.7.0), so it is not a regression from the
  TCK work. Pinned by `streaming_error_envelope_tests.rs`, which forces a
  deterministic lag by overflowing a 2-slot ring before the reader is polled,
  rather than depending on benchmark timing.

### Changed

- **`backpressure` benchmark: the event queue is sized above the event
  count.** `stream_volume/502_events` ran against the default 256-slot
  broadcast ring, so the reader lagged and the stream ended in a
  `streamLagged` error — that configuration was measuring event loss as much
  as streaming cost. The historical "252→502 events: ~193µs/event" figure in
  the source comment was taken from such a run and has been removed as not
  comparable. Slow-consumer behaviour is still exercised deliberately in
  `bench_slow_consumer`.

### Fixed

- **The official-TCK workflow reported `Success` while 12 MUST-level checks
  failed.** Both suite steps carried `continue-on-error: true`, so the job
  went green with `Process completed with exit code 1` sitting in the
  annotations. That is the same class of defect this project criticises
  elsewhere — a published signal that does not reflect reality — and a green
  check nobody can trust is worse than a red one, because nobody reads a green
  check.

  Replaced with a differential gate. `tck/scripts/check_conformance.py`
  compares the suite's machine-readable `compatibility.json` against a
  checked-in baseline (`tck/conformance-baseline.json`) at
  (requirement, transport) granularity, and fails the job on **either**
  direction:

  - a MUST-level failure not in the baseline — a regression;
  - a baseline entry that now passes — a stale baseline.

  The second direction is what keeps the gate honest: a baseline allowed to
  rot is `continue-on-error` with extra steps. Transport granularity matters
  because `PUSH-CREATE-001` fails on `jsonrpc` and passes on `http_json`
  today, and a requirement-level baseline would miss it spreading. A missing
  or malformed report also fails, so "the suite never ran" cannot read as
  success.

  The gate was counter-tested against injected regressions, an injected fix,
  a failure spreading to a second transport, missing/malformed/wrong-shaped
  reports, and a SHOULD-level failure (which must not gate) — plus an
  end-to-end simulation of the job's two shell steps confirming the run
  step's `|| true` cannot mask the gate's exit code.

  CI now runs the full suite once rather than twice: the `--level must` run
  was verified to produce an identical set of gated failures.

- `docs/official-tck-findings.md` now states that **all 12 remaining failures
  are `MUST` level** (previously listed without their level, which understated
  them), records that reported MUST compatibility of 85.4% is computed over
  tested requirements only — 21 further MUST requirements report `NOT TESTED`
  — and documents the baseline and gate.

### Fixed (found by the official A2A TCK)

Adopted the A2A project's own conformance suite
([`a2aproject/a2a-tck`](https://github.com/a2aproject/a2a-tck)) alongside the
in-repo one, with a new `tck/sut` System Under Test implementing its
`messageId`-keyed behaviour contract. It immediately found two real defects:

- **Unsupported `Content-Type` returned the wrong JSON-RPC error code**
  (**breaking**, error code only) — the JSON-RPC binding rejected an
  unsupported media type with `ParseError` (-32700); spec §5.4 maps it to
  `ContentTypeNotSupportedError` (-32005). The body is never parsed on that
  path, so the old code both misreported the cause and withheld the §10.6
  `CONTENT_TYPE_NOT_SUPPORTED` reason, which is now attached. Three in-repo
  tests had asserted the incorrect code and passed.
- **The task state machine rejected conformant agents** — six MUST-level
  checks failed because `TaskState::can_transition_to` required
  `Submitted → Working` before any finish state, so an agent that answered in
  one step got `InvalidParams` from its own SDK. Spec §4.1.3 defines no
  transition matrix and requires no intermediate state, and the reference SDKs
  complete directly from `Submitted`. The table now enforces only that
  terminal states are final and that nothing re-enters `Submitted` /
  `Unspecified`; `Working → Rejected` is permitted too (§4.1.3 allows
  rejecting "later"). `state_validation_tests.rs` was rewritten to pin all 81
  matrix cells against an independently-computed predicate.

Conformance went from 87 to 158 passing checks against the same SUT.

The remaining failures are recorded in `docs/official-tck-findings.md` rather
than skipped. That document also carries a correction: an earlier revision
claimed a bug in the TCK's JSON-RPC client (snake_case params vs spec §5.5).
**That claim was wrong and is retracted.** The A2A JSON data model is
generated from protobuf, and ProtoJSON parsers accept both the camelCase
`json_name` and the original snake_case field name; the official Python SDK
was measured doing exactly that. §5.5 governs emission, not acceptance.

The real defect the TCK surfaced is this SDK's own: **unrecognised request
parameters are silently ignored** where the reference rejects them, so a
`ListTasks` filter misspelled by any means — snake_case, wrong case, or a
typo — returns every task instead of an error or a filtered set. The reported
`historyLength` overrun has the same single root cause (`historyLength` is
honoured; the ignored `history_length` is not). Scope and fix direction are in
the findings document; the fix is deliberately not rushed in here, since it
needs an alias list generated from the protobuf schema and a per-field test,
not patches for the six spellings the TCK happens to send.

### Added

- **Developer Certificate of Origin adopted.** The project now requires a
  `Signed-off-by:` trailer on every commit, certified under the
  [DCO 1.1](DCO) (verbatim text added at the repository root). A new
  `.github/workflows/dco.yml` gate fails any pull request containing a
  non-merge commit without a sign-off matching its git author, and
  additionally rejects commits whose author is a known AI-assistant service
  account — a sign-off is an assertion by a person, so a tool identity cannot
  make one.
- **`PROVENANCE.md`** — a provenance record covering (1) a full disclosure of
  this project's AI-assisted development, with reproducible commit-authorship
  figures; (2) a one-time blanket DCO certification by the maintainer covering
  every commit through `b416c1a`, made because rewriting history would
  invalidate all ten release tags and the SLSA attestations bound to the
  published v0.2.0–v0.7.0 crates; and (3) an inventory of third-party material
  in the tree (the spec's `a2a.proto`, vendored googleapis stubs, the ITK
  `instruction.proto`, and the a2a-inspector card ruleset) with its licensing.
- **`.github/PULL_REQUEST_TEMPLATE.md`**, leading with the sign-off
  requirement.
- **`docs/rust-sdk-assessment.md`** — a source-verified technical and
  governance comparison of this SDK against `a2aproject/a2a-rs`, prepared for
  A2A project / Linux Foundation review.

### Changed

- **AI-assisted commits are now authored by the human who directed them**,
  with the assistant credited via a `Co-Authored-By:` trailer, replacing the
  prior pattern of `Claude <noreply@anthropic.com>` in the git author field.
  `CONTRIBUTING.md` documents the convention; CI enforces it.
- `CONTRIBUTING.md`, `GOVERNANCE.md`, `README.md`, and
  `.github/workflows/README.md` updated for the DCO requirement.

### Added (governance, legal, and release policy)

- **Security and conduct reports now go to a reachable address.**
  `a2a-rust.dev` is not registered — DNS returns NXDOMAIN — so both
  `security@a2a-rust.dev` (`SECURITY.md`'s primary vulnerability channel) and
  `conduct@a2a-rust.dev` (`CODE_OF_CONDUCT.md`'s only reporting channel) were
  undeliverable, and anything sent to either would have bounced or been lost.
  Both now point at the maintainer address already published in this project's
  copyright headers, and `SECURITY.md` promotes GitHub Security Advisories to
  the preferred channel since it stays private without a PGP key. Each file
  states plainly that the `a2a-rust.dev` address does not work, so it is not
  reinstated on the assumption that it does.
- **`ROADMAP.md`** — what the repository has already committed to for 0.8
  (three deprecation removals), the gaps in the project's own verification,
  the supply-chain items still open (unsigned tags, no PGP key, the
  unregistered domain), and the measured 92/114 TCK position. Derived only
  from what is already in the tree; it asserts no dates.
- **`SECURITY.md` release-artifact verification table** — states what is and
  is not signed. All ten release tags are lightweight, so `git tag -v`
  verifies nothing; adopters needing a cryptographic link to this repository
  must use the SLSA build provenance attestations instead.
- **`CODE_OF_CONDUCT.md`** — Contributor Covenant 2.1, with the four-tier
  enforcement ladder (Correction / Warning / Temporary Ban / Permanent Ban)
  and a conduct-specific reporting address. Replaces the four-line clause in
  `GOVERNANCE.md`, which routed conduct reports to the *security* mailbox;
  that section is now a pointer. The document also states its own limitation:
  with a single maintainer there is no independent party inside the project to
  escalate a report *about* that maintainer to.
- **`NOTICE`** — canonical Apache-2.0-form notice carrying the project
  copyright and the third-party attributions previously only enumerated in
  `PROVENANCE.md` (the spec's `a2a.proto`, vendored googleapis stubs, the ITK
  `instruction.proto`, the a2a-inspector card ruleset).
- **`.github/ISSUE_TEMPLATE/`** — structured bug-report and feature-request
  forms, plus a `config.yml` that disables blank issues and routes security
  reports to GitHub Security Advisories rather than the public tracker.
- **`RELEASING.md` — "Path to 1.0.0"**: explicit criteria for what would
  justify a 1.0.0 release (no unresolved MUST-level TCK failures, no coverage
  regression below the measured floor, a clean full mutation sweep, no open
  P0/P1s, and a deliberate `pub` API surface review), plus a **post-1.0
  deprecation policy** — `#[deprecated]` for at least one minor version
  before removal, removal only in a major bump, with security fixes exempt.
- **`docs/upstream/`** — a bug report against `a2aproject/a2a-tck` with a
  standalone reproduction script, covering a harness defect this SDK triggers
  by behaving correctly. Filed 2026-08-07 as
  [a2a-tck#225](https://github.com/a2aproject/a2a-tck/issues/225); the file is
  retained as the record of what was submitted and the evidence behind it.

### Changed (public API documentation)

- **`otel::init_otlp_pipeline` now documents that it panics outside a Tokio
  runtime.** Behaviour is unchanged; the precondition was previously
  undocumented. Calling it outside a runtime raises `there is no reactor
  running, must be called from the context of a Tokio 1.x runtime` from
  inside `tonic`'s channel constructor rather than returning `Err` — and
  because `[profile.release]` sets `panic = "abort"`, that aborts the process
  in release builds. Also newly documented: the installed `MeterProvider` is
  process-global and last-write-wins, and `shutdown()` returns an error when
  metrics have been recorded and no collector is reachable (it attempts a
  final flush), which graceful-termination code should treat as "metrics may
  have been lost" rather than as fatal. Found by adding the first test to ever
  call the function.

## [0.7.0] - 2026-07-24

Interop, hardening, and edge-case fixes from an independent protocol audit,
plus a protobuf-native rewrite of the gRPC transport. Several public types
changed shape (0.x breaking — warrants a minor bump).

### Added (cross-SDK interop & adversarial testing)

- **Official-SDK interop is now proven, both directions.** The TCK runs
  against echo agents built on the official Python (`a2a-sdk`),
  JavaScript (`@a2a-js/sdk`), Go (`a2a-go/v2`), and Java (`a2a-java`) SDKs
  — 20/20 on both JSON-RPC and REST for every one — and the official
  Python SDK *client* drives our server end to end
  (`itk/interop/python_client_vs_rust.py`). New `itk/agents/*-sdk`
  agents; TCK gained `--skip` for documented reference-SDK divergences.
- **Upstream ITK current-mount.** `itk/` is now the a2aproject/a2a-itk
  "current" agent (`itk-current-agent`), implementing the multi-hop
  traversal instruction protocol on `a2a-protocol-{server,client}` across
  JSON-RPC, gRPC, and HTTP+JSON (plain, streaming, push, resubscribe). A
  deterministic in-repo self-test and a CI workflow that mounts this repo
  into the real ITK against the official Python baseline both cover it.
- **Fuzzing expanded 1 → 6 targets** (JSON, JSON-RPC envelope + params,
  SSE parser, protobuf↔serde differential round-trip, ISO-8601, JWKS),
  wired into CI (60s smoke per PR, 10-min nightly).
- **Hostile-peer harness**: our client vs malicious servers (oversized /
  slow-drip / truncated bodies, immediate close, valid-JSON-wrong-shape)
  — each must fail safely, no hang or panic.
- **`SPEC_COMPLIANCE.md`**: a §-by-§ traceability matrix (spec → impl →
  test evidence). Release CI gained `cargo-semver-checks` and CycloneDX
  SBOM generation + attestation alongside the existing SLSA provenance.

### Changed (cross-SDK interop)

- **Default `AgentExecutor::cancel` now cancels a working task** instead of
  refusing with `TaskNotCancelable` — the handler already triggers the
  cancellation token, so the default emits the terminal `Canceled` status
  (best-effort delivery). Every reference SDK requires working cancel out
  of the box; the pre-0.7 default made `WORKING` tasks uncancelable and
  mislabeled the refusal as the task's fault. The cancel handler treats a
  re-read of `Canceled` as success rather than a TOCTOU race.
- **Example agent cards advertise the spec-canonical `"HTTP+JSON"`**
  protocol binding instead of the legacy `"REST"` spelling (the official
  Python client cannot match `"REST"` when selecting a transport). The
  client still accepts both spellings when reading a peer's card.

### Added (spec-compliance closure pass)

- **gRPC errors carry `google.rpc.ErrorInfo`** (spec §10.6) — every
  A2A-specific error now attaches the machine-readable
  `reason`/`domain: a2a-protocol.org` detail to `status.details` via
  `tonic-types`, making the three bindings error-equivalent (§5.1). The
  gRPC client decodes it back to the exact `ErrorCode` instead of the lossy
  status-code inverse mapping, and the REST client now decodes AIP-193
  error bodies (§11.6) into structured `A2aError`s the same way.
- **`taskId`-only continuations** (spec §3.4.3) — `SendMessage` with a
  `taskId` and no `contextId` now infers the context from the referenced
  task instead of rejecting with `InvalidParams`; an unknown `taskId`
  returns `TaskNotFoundError` per §3.4.2.
- **`statusTimestampAfter` is enforced** (spec §3.1.4) — previously parsed
  and silently ignored by every store; now applied as a strictly-after
  filter in the in-memory, SQLite, Postgres, and tenant-aware stores, with
  malformed timestamps rejected as `InvalidParams` at the handler.
- **`ListTasks` sorts by status timestamp** (spec §3.1.4) — ordering now
  follows `status.timestamp` descending (write wall-clock only as the
  fallback for tasks without one), so a re-save that does not change the
  status — e.g. an artifact append — no longer spuriously bumps a task to
  the front. Status timestamps gained millisecond precision (§5.6.1) to
  keep the order deterministic. The in-memory store's page tokens changed
  format (`millis:seq`); SQL cursors are unchanged.
- **Required-extension negotiation** (spec §3.3.4) — agent-card extensions
  marked `required: true` are now enforced on every data-plane operation:
  clients that do not declare them in `A2A-Extensions` get
  `ExtensionSupportRequiredError` (previously never emitted). The HTTP
  bindings also echo the activated extension set (requested ∩ declared)
  in the response `A2A-Extensions` header, matching official-SDK behavior.
- **JSON-RPC SSE envelopes echo the request `id`** (spec §9.4.2) —
  streaming frames previously carried `"id": null`.
- **gRPC validates the `a2a-version` service parameter** (spec §3.6.2 /
  §10.2), matching the JSON-RPC/REST/WebSocket bindings; REST and the
  WebSocket handshake now reject unsupported versions with the real
  `VersionNotSupportedError` shape (AIP-193 body with `ErrorInfo`) instead
  of an anonymous 400. The client WebSocket transport now sends
  `A2A-Version` on the upgrade request (§3.6.1).
- **OIDC discovery test coverage** — `from_oidc_issuer`/`discover_jwks_uri`
  now have end-to-end tests (live issuer, missing `jwks_uri`, invalid
  JSON, HTTP error, unreachable issuer), plus a live-TLS JWKS end-to-end
  test and a new `JwtAuthInterceptor::from_jwks_url_with_tls_config` for
  identity providers behind private CAs.

### Changed (reference-SDK interop pass)

Running our TCK against an echo agent built on the **official Python
`a2a-sdk` (1.1.2)** — instead of hand-written stubs — surfaced three
interop deviations, all fixed for exact reference parity (TCK now passes
20/20 against the official SDK on both JSON-RPC and REST):

- **Missing `A2A-Version` request header is now rejected** (spec §3.6.2,
  **breaking**) — a request without the header (or with an empty value)
  MUST be interpreted as protocol 0.3, which this server does not
  implement, so all data-plane HTTP/WebSocket calls now fail with
  `VersionNotSupported` exactly like the reference SDK. Agent-card
  discovery (`/.well-known/agent-card.json`) stays versionless — clients
  fetch it before they know anything about the agent. Opt out via
  `DispatchConfig::accept_missing_version_header()` (HTTP) or
  `WebSocketDispatcher::accept_missing_version_header()` for manual
  testing / trusted deployments. The TCK itself now sends
  `A2A-Version: 1.0` on every request — previously its requests were,
  per spec, 0.3 requests, and the reference SDK correctly refused them.
- **v0.3-style method names and paths removed** (**breaking**) — the
  JSON-RPC and WebSocket dispatchers no longer accept `message/send`,
  `tasks/get`, `tasks/pushNotificationConfig/*`, etc. as aliases for the
  v1.0 PascalCase RPC names (`SendMessage`, `GetTask`, …), and the REST
  binding no longer accepts `/message/send` for `/message:send`. The
  reference SDK returns `MethodNotFound`/404 for these (its 0.3 support
  is a separate opt-in adapter with real 0.3 payload semantics — which
  the aliases never provided).
- **TCK tolerates non-JSON error bodies on non-A2A paths** — a framework
  plain-text 404 for an unrouted path is legitimate (the reference SDK
  returns one); the status code is the conformance signal.

### Changed (spec-compliance closure pass)

- **Extended agent card requires authentication by default** (spec §13.3,
  **breaking**) — when the agent card declares
  `capabilities.extendedAgentCard: true` but the interceptor chain contains
  no authenticating interceptor, `GetExtendedAgentCard` now refuses to
  serve the card instead of handing the "authenticated" card to anonymous
  callers. `ServerInterceptor` gained a `authenticates()` marker (default
  `false`; the built-in API-key/bearer/JWT interceptors return `true` —
  custom auth interceptors should override it), and
  `RequestHandlerBuilder::allow_unauthenticated_extended_card()` is the
  explicit opt-out for deployments that authenticate upstream.
- **Slow streaming consumers get an explicit lag error** (**breaking
  semantics**) — a consumer that falls behind the broadcast ring previously
  had the gap silently skipped; it now receives a marked stream error
  (`data.streamLagged`) and the stream closes, so the client knows its view
  is truncated and can resubscribe for a fresh snapshot (§3.5.2). Task
  persistence was never affected (it uses a dedicated lossless channel).
  Relatedly, `with_event_queue_write_timeout` / `with_write_timeout` are
  deprecated no-ops (queue writes never block) slated for removal in 0.8.
- **Resubscribe after a process restart serves snapshot-then-EOF** — a
  non-terminal task with no live event queue previously produced
  `Internal ("no active event queue")`; per §3.5.2 reconnection the stream
  now delivers the current `Task` snapshot and ends cleanly.
- **Wire form of the protocol version is `1.0`** (spec §3.6, **breaking
  constant**) — `A2A_VERSION` changed from `"1.0.0"` to `"1.0"` ("patch
  version numbers SHOULD NOT be used in requests, responses and Agent
  Cards"); the `A2A-Version` response header and the examples' agent cards
  follow.
- **HTTP responses emit `Content-Type: application/json`** (spec §9.1 and
  §11.1; previously `application/a2a+json`). Both media types remain
  accepted on ingress, and the registered `application/a2a+json` constant
  is still exported.
- **Push webhook auth scheme matching is case-insensitive** (RFC 9110) —
  a config spelling the scheme `Bearer`/`BASIC` previously fell through
  and silently sent **no** `Authorization` header; canonical
  capitalization is now emitted regardless of config spelling.
- **Push delivery no longer retries non-retryable 4xx** — 400/401/403/404
  and similar fail fast after one attempt; retry with backoff is reserved
  for 408, 429, 5xx, timeouts, and connection errors.
- **Weekly full mutation sweep re-enabled** in CI (the incremental gate
  only covers files a PR touches), and dedicated `websocket`/`grpc`
  feature test legs were added alongside a combined
  `auth-jwt + tls-rustls` leg.

### Changed

- **gRPC is now protobuf-native and wire-compatible with the official A2A
  SDKs** — the transport speaks the canonical `lf.a2a.v1.A2AService`
  (fully-typed messages generated from the specification's protobuf
  schema, kept byte-identical in-repo) instead of the pre-0.7 JSON-in-
  `bytes` tunnel on a non-standard service. A Go, Python, or Java SDK
  peer can now interoperate over gRPC. Details in ADR 0009.
  - `a2a-protocol-types` gains a `proto` feature exposing the generated
    message types (`a2a_protocol_types::proto`) and a bidirectional
    `TryFrom` conversion layer to the serde domain types (ProtoJSON
    semantics; property-tested through real encoded protobuf bytes).
  - `a2a-protocol-server`: `GrpcDispatcher` serves the canonical service.
    The deprecated JSON tunnel (`a2a.v1.A2aService`) can still be served
    *alongside it* for 0.6 clients via the new `grpc-legacy-json` feature
    (off by default, removal planned for 0.8); rolling-upgrade coexistence
    is covered by an e2e test. `into_service` now returns the canonical
    service type; the legacy service is available via `into_legacy_service`.
  - `a2a-protocol-client`: `GrpcTransport` speaks the canonical service;
    the tunnel client was removed. Conversion failures surface as
    non-retryable `ClientError::Transport` errors.
  - Wire compatibility is proven against the official A2A Python SDK:
    golden binary fixtures serialized by `a2a-sdk` are checked in under
    `tck/fixtures/grpc/`, validated in both directions (prost decodes the
    official bytes; the official SDK parses prost-encoded bytes) by the
    new `grpc-wire-compat` CI job.
- **`a2a-protocol-types`: push types accept spec-compliant JSON** —
  `AuthenticationInfo.credentials` and `TaskPushNotificationConfig.taskId`
  are now `Option<String>`, matching the canonical protocol schema (both
  were previously required, rejecting valid cross-SDK payloads at parse
  time — e.g. a push config nested in `SendMessageConfiguration` before the
  task exists). A standalone `CreateTaskPushNotificationConfig` without a
  `taskId` is now rejected by the server with a structured invalid-params
  error instead of a parse error; all push-config stores guard the missing
  routing key explicitly.
- **`a2a-protocol-types`: JSON-RPC request ids are three-state** —
  `JsonRpcRequest.id` is now the `JsonRpcRequestId` enum
  (`Absent`/`Null`/`Value`). An explicit `"id": null` request previously
  collapsed into a notification on round-trip; per JSON-RPC 2.0 it is a
  *call* and now round-trips faithfully. Server responses are unchanged.
- **`a2a-protocol-server`: concurrent streams capped at 1024 by default** —
  `max_concurrent_streams` previously defaulted to unlimited; every stream
  eagerly allocates channels and spawns tasks, an unauthenticated DoS
  vector. Deployments needing more must raise the cap explicitly
  (`usize::MAX` effectively disables it). `executor_timeout` deliberately
  keeps no default, now documented as a production recommendation.
- **`a2a-protocol-server`: `RateLimitInterceptor` hardened** — caller
  identity no longer trusts the client-controlled `X-Forwarded-For` header
  unless `RateLimitConfig::trusted_proxy_hops` is set (forged headers
  previously bypassed the limit entirely); the bucket map is bounded by
  `max_buckets` (default 10,000); `RateLimitInterceptor::new` is now
  fallible and rejects zero config values (`window_secs == 0` previously
  panicked with a divide-by-zero at request time).
- **`a2a-protocol-client`: buffered response bodies capped** — unary
  responses were collected with no size limit; they are now capped at
  32 MiB by default (configurable via
  `ClientBuilder::with_max_response_size`), enforced during the read.

### Fixed

- **`a2a-protocol-types`: ambiguous `Part` content rejected** — a part
  carrying more than one content member (`text`/`raw`/`url`/`data`) was
  silently coalesced to the first match; it now fails deserialization.
- **`a2a-protocol-types`: RFC 8785 canonicalization conformance (signing)**
  — object keys now sort by UTF-16 code units (§3.2.3) and doubles use
  ECMAScript `Number::toString` formatting (§3.2.2), fixing cross-SDK
  signature mismatches for cards containing supplementary-plane keys or
  non-integer numbers in `AgentExtension.params`.
- **WebSocket (opt-in)**: the server's 4 MiB message cap is now enforced at
  the protocol level via `WebSocketConfig` (previously tungstenite's 64 MiB
  default applied and oversized messages were fully buffered before the
  check); the client no longer leaks a pending-request map entry per
  timed-out request.

### Security

- **`a2a-protocol-server`: a configured `TenantResolver` is now enforced.**
  `with_tenant_resolver(...)` previously configured a resolver that was never
  consulted — the client-controlled `tenant` field alone selected the store
  partition, so any caller could read or write another tenant's tasks by
  naming it. The resolver is now authoritative (tenant derived from trusted
  request context); a client-supplied tenant that disagrees is rejected. With
  no resolver configured, behavior is unchanged (single-tenant / trusted
  caller).
- **`a2a-protocol-server`: push-notification config count is bounded on every
  store backend.** A per-task cap (`HandlerLimits::max_push_configs_per_task`,
  default 100) is enforced in the handler; previously only the in-memory store
  self-enforced, so the SQLite/Postgres stores let a client mint unbounded
  configs for one task (disk exhaustion, and delivery amplification since each
  event fans out to every config).
- **`a2a-protocol-server`: tenant-aware in-memory push-config store no longer
  allocates a partition on read.** `get`/`list`/`delete` for an unseen tenant
  were creating (and counting) a partition, an unauthenticated `max_tenants`
  exhaustion vector.
- **`a2a-protocol-server`: OpenTelemetry error metrics are labeled by a bounded
  discriminant, not the error message.** The `error` attribute was the rendered
  message (embedding client-controlled ids), a metric-cardinality-explosion
  vector; it is now `ServerError::metric_label()` (a fixed set).
- **`a2a-protocol-server`: rate-limit caller keys canonicalize IP forms.** An
  IPv4-mapped IPv6 address and its plain IPv4 form now share one bucket, so a
  client cannot obtain two budgets by presenting both.
- **`a2a-protocol-client`: JSON-RPC error `data` and unknown codes are no longer
  discarded.** All transports preserve `error.data`, and an implementation-
  defined code outside the closed `ErrorCode` set is retained (under the error
  `data`) instead of collapsing silently to `InternalError`.
- **`a2a-protocol-client` (gRPC/WebSocket): unparseable auth metadata now fails
  the request closed** instead of being silently dropped and sent
  unauthenticated. Header values are never echoed in the error.
- **`a2a-protocol-types`: webhook secrets are redacted in `Debug`.**
  `AuthenticationInfo.credentials` and `TaskPushNotificationConfig.token` no
  longer print their values via `{:?}` (presence is still shown);
  serialization is unchanged.
- **`a2a-protocol-types` / proto: bounded recursion.** `Struct`/`Value`
  conversion and RFC 8785 canonicalization reject nesting past a fixed depth
  instead of risking a stack overflow on a programmatically-built value.

### Fixed (additional hardening)

- **`a2a-protocol-server`: fire-and-forget (`returnImmediately`) sends now run
  to completion.** They spawned no background processor and no persistence
  channel, so the executor's events went nowhere: nothing was persisted, no
  push fired, and the task was stuck in `Submitted` forever. They now use the
  background processor exactly like streaming.
- **`a2a-protocol-server`: hitting `max_concurrent_streams` returns a clean
  overload error with no side effects.** The cap was detected only after the
  task and cancellation token had been committed, orphaning the task in
  `Submitted` and returning a misleading internal error; the queue is now
  leased (and the cap checked) before any side effect, surfacing the new
  `ServerError::Overloaded` (gRPC `RESOURCE_EXHAUSTED` / HTTP 503).
- **`a2a-protocol-server`: a second `message/send` to a task already being
  processed is rejected** instead of spawning a second executor and overwriting
  the first's cancellation token (which left the original work uncancelable and
  racing on store writes).
- **`a2a-protocol-server`: `CancelTask` no longer leaks an event queue.**
  Cancelling a task whose executor had already exited registered a queue via
  `get_or_create` that nothing removed — a permanent map + concurrency-slot
  leak keyed by task id.
- **`a2a-protocol-server`: per-artifact `parts` growth is bounded**
  (`HandlerLimits::max_parts_per_artifact`, default 10,000), so an unbounded
  stream of `append: true` artifact updates cannot grow one task record without
  limit.
- **`a2a-protocol-server`: REST query percent-decoding is UTF-8 correct** —
  multi-byte sequences (e.g. a percent-encoded non-ASCII tenant name) decode to
  the original string instead of per-byte Latin-1 garbage.
- **`a2a-protocol-server`: the bundled `HttpPushSender` rejects `https://`
  webhooks with a clear, actionable error** (it is HTTP-only) instead of an
  opaque connector error after every retry; HTTPS delivery is available by
  supplying a TLS-capable `PushSender`.
- **`a2a-protocol-server`: agent-card hot-reload watchers no longer block a
  runtime worker** on file I/O (reads run on the blocking pool).
- **`a2a-protocol-client` (gRPC): `RESOURCE_EXHAUSTED` maps to a retryable
  error**, and mid-stream `Unavailable`/`DeadlineExceeded` keep their retryable
  classification (they were non-retryable `Protocol` errors); the configured
  response-size cap now applies to the gRPC decode limit.
- **`a2a-protocol-client` (REST streaming): the non-2xx error body is read
  through the size-capped collector** (was unbounded), and a non-SSE `200`
  response surfaces as an error instead of dissolving into a silently empty
  stream.
- **`a2a-protocol-client` (WebSocket): terminal-state detection recognizes the
  canonical `TASK_STATE_*` wire strings**, fixing a pending-map + sender leak of
  one entry per completed stream against every spec-conformant server;
  `route_frame` no longer holds the pending-map mutex across a bounded stream
  send (a stalled consumer could wedge the whole transport).
- **`a2a-protocol-client` (SSE): a BOM split across reads is stripped**
  correctly (the first event was being lost), and the tail of an over-limit
  event is discarded to the next boundary instead of being re-parsed into a
  spurious frame.
- **`a2a-protocol-client`: the spec binding name `HTTP+JSON` resolves to the
  REST transport** (it was rejected as unknown), and the retry jitter uses
  `Duration::try_from_secs_f64` to avoid a panic on a near-`Duration::MAX`
  backoff config.

### Added (second hardening pass)

- **HTTPS is first-class.** `tls-rustls` is now a **default** feature of
  `a2a-protocol-client` and `a2a-protocol-sdk`, so the client reaches
  `https://` agents — the spec-standard transport — out of the box (opt out
  with `default-features = false`). A new `a2a-protocol-server` `tls-rustls`
  feature makes the bundled `HttpPushSender` deliver to `https://` webhooks;
  the SDK's `tls-rustls` enables it too. Without the feature, `https://` fails
  fast with an actionable error (client `build()` and the push sender) instead
  of a late, opaque connector error. `HttpPushSender::with_tls_config` accepts a
  custom rustls `ClientConfig` (internal/private-CA webhooks, or mutual TLS).
  A live-TLS end-to-end test drives the sender through a real handshake against
  a `tokio-rustls` server on loopback (runs in the `tls-rustls` and
  `all-features` CI jobs).
- **`a2a-protocol-server`: opt-in strict multi-tenancy**
  (`RequestHandlerBuilder::require_resolved_tenant`) — a configured
  `TenantResolver` that returns `None` rejects the request instead of falling
  back to the shared default (`""`) partition. Off by default to preserve the
  documented resolver contract.
- **`a2a-protocol-server`: a global push-config ceiling**
  (`HandlerLimits::max_total_push_configs`, default 100,000; per-tenant for
  tenant stores) via a new `PushConfigStore::count` method.

### Fixed (second hardening pass)

- **`a2a-protocol-client`: automatic retries no longer duplicate work.**
  Non-idempotent methods (`SendMessage`, `SendStreamingMessage`,
  `CreateTaskPushNotificationConfig`, and any unknown method) are retried only
  when the server rejected the request up front (`429`/`503`), never on an
  ambiguous timeout or connection error the server may already have processed.
- **`a2a-protocol-client`: `request_timeout` is a single per-call deadline.**
  It was applied once to the response headers and again to the body read,
  letting a slow server hold a call for up to 2× the configured budget.
- **`a2a-protocol-client`: `Retry-After` (delta-seconds) is honored** on
  `429`/`503` in preference to the computed backoff (clamped to `max_backoff`),
  so the client stops hammering a server that asked it to wait.
- **`a2a-protocol-client`: WebSocket streams get an establishment timeout.**
  The WS streaming path previously returned a stream with no timeout of any
  kind; a socket accepted but never answered would hang the consumer forever.
- **`a2a-protocol-types`: `JsonRpcResponse` deserialization enforces JSON-RPC
  2.0 §5** (exactly one of `result`/`error`). A malformed response carrying
  *both* was silently read as success with the error discarded; a mistyped
  `result` now surfaces the real type error instead of an opaque
  "no variant matched".
- **`a2a-protocol-server`: closed a lost-work race in the send path.** A
  continuation send that hit an already-registered event queue (reachable via a
  cancel-then-resend race or an aged-out cancellation token) spawned a second
  executor with no persistence channel — silently dropping the resent task's
  state transitions and push notifications while racing the original on store
  writes. Such a send is now rejected. The stale-token sweep no longer evicts a
  still-running task's token.
- **`a2a-protocol-server`: SQL push-config backends are bounded.** The global
  ceiling (above) closes a disk-growth vector where configs spread across
  unboundedly many task ids had no limit on SQL stores.
- **Docs:** corrected the book's push-notifications page (the bundled sender's
  HTTPS behavior, a removed non-existent "HTTPS-only enforcement" toggle, and a
  won't-compile `credentials` example) and the "all features off by default"
  note (now that `tls-rustls` is a default).

### Changed (third hardening pass)

- **WebSocket is a full-surface, authenticated transport** — the WebSocket
  binding now matches the JSON-RPC HTTP dispatcher's method surface and
  security posture:
  - `a2a-protocol-server`: the upgrade request's HTTP headers (plus the
    request path, under `":path"`) are captured during the handshake and
    passed to the handler for every request on the connection — tenant
    resolvers, strict multi-tenancy, and header-based auth now work over
    WebSocket exactly as over HTTP. Previously the dispatcher passed an
    **empty** header map, so every WebSocket client resolved to the default
    tenant partition and header-based authentication was impossible.
  - `a2a-protocol-server`: all push-notification-config methods
    (`CreateTaskPushNotificationConfig`, `GetTaskPushNotificationConfig`,
    `ListTaskPushNotificationConfigs`, `DeleteTaskPushNotificationConfig`)
    and `GetExtendedAgentCard` are now routed over WebSocket, and every
    method also accepts its v0.3 `method/verb` alias (`message/send`,
    `tasks/get`, …) for parity with the HTTP dispatcher. Previously only
    6 of the 11 A2A methods were reachable over WebSocket.
  - `a2a-protocol-server`: an upgrade request carrying an `A2A-Version`
    header with a major version other than 1 is rejected during the
    handshake (HTTP 400), mirroring the HTTP dispatchers.
  - `a2a-protocol-client`: new `WebSocketTransportConfig` +
    `WebSocketTransport::connect_with_config` configure the request
    timeout, upgrade headers, and the incoming message-size cap in one
    place. Incoming messages are now capped (default 32 MiB, the shared
    response-size ceiling of the HTTP/gRPC transports) at the WebSocket
    protocol level — previously tungstenite's 64 MiB default applied.

### Added (third hardening pass)

- **First-party authentication helpers (client + server)** — the SDK now
  *acquires* and *verifies* credentials, not just models the schemes and
  provides interceptor hooks (ADR 0010). Built on the existing `ring`/`hyper`
  stack — no OAuth-ecosystem dependencies.
  - `a2a-protocol-client`: a `TokenProvider` trait with `StaticTokenProvider`,
    a `BearerAuthInterceptor` that injects a fresh token before every request
    (so a rotating token stays current, including on retries), and
    `OAuth2ClientCredentials` — the RFC 6749 §4.4 client-credentials grant with
    token caching, proactive pre-expiry refresh, single-flight concurrent
    refresh, `Basic`/`Post` client-auth styles, and constructors that read the
    token endpoint from an agent card's OAuth2 flow or discover it from an OIDC
    issuer. Client secrets are redacted from `Debug` and never echoed in errors.
  - `a2a-protocol-server`: `ApiKeyAuthInterceptor` and
    `BearerTokenAuthInterceptor` (constant-time comparison, no feature flag)
    and, behind the new `auth-jwt` feature, `JwtAuthInterceptor` — verifies
    HS256/RS256/ES256, checks `exp`/`nbf`/`iss`/`aud`, and resolves keys from a
    static `Jwks`, a shared HS256 secret, or a remote JWKS endpoint (TTL-cached,
    refetched once on a key-id miss to follow rotation) with OIDC discovery.
    `alg: none` and unlisted algorithms are rejected, and HS256 is only ever
    checked against a configured secret — never a JWKS public key — so the
    RS256→HS256 confusion downgrade is structurally impossible. Rejections are
    generic (no oracle) and map to `InvalidRequest` (HTTP 400). JWT verification
    is cross-checked against independently-generated (Python) test vectors and
    a live interceptor→dispatcher→HTTP end-to-end test.
  - `a2a-protocol-sdk`: re-exports the client/server auth types from the
    prelude; the `auth-jwt` feature passes through to the server.
- **`A2A-Extensions` header wired end to end (spec §14.2.2)** — new
  `A2A_EXTENSIONS_HEADER` constant in `a2a-protocol-types`, and every
  server binding now parses the comma-separated extension URIs into
  `CallContext::extensions` for interceptors and resolvers (the accessor
  existed but nothing populated it — it always returned empty). Extension
  *data* continues to ride in-band in `Message::extensions`/metadata.

### Fixed (third hardening pass)

- **`a2a-protocol-server` (push): the notification token is sent as
  `X-A2A-Notification-Token`** — the header name the spec's push example
  uses and the official SDK's webhook receivers read. The bare
  `a2a-notification-token` name was this SDK's own pre-0.7 invention, so
  receivers written against the official convention never saw the token;
  it is still sent alongside the canonical name for migration and will be
  removed in 0.8. The default CORS allow-list now also includes
  `a2a-version` and `a2a-extensions` — protocol headers A2A clients send
  on every request, without which a browser client's preflight fails.
- **`a2a-protocol-server` (REST): the canonical `/{tenant}/...` bindings
  are now routed** — the spec proto's `google.api.http` additional
  bindings put the tenant as a bare first path segment
  (`/{tenant}/message:send`, `/{tenant}/tasks`, …), which is exactly what
  official-SDK REST clients send when configured with a tenant; only this
  SDK's own `/tenants/{tenant}/...` form was recognized, so canonical
  tenant-scoped requests 404'd. Both forms now work, with
  literal-beats-variable matching so a real route is never swallowed as a
  tenant. Verified live with the official Python SDK's `RestTransport`
  (tenant-scoped send/list/get all round-trip).

- **`a2a-protocol-types`: ProtoJSON empty-repeated omission no longer breaks
  cross-SDK parsing** — ProtoJSON printers (what every official A2A SDK
  uses on the JSON wire) omit empty repeated fields and empty maps, so
  absence means "empty". Twelve JSON-facing list/map fields required their
  key to be present and rejected real official-SDK traffic at parse time —
  found live by driving this SDK's JSON-RPC server with the official
  Python SDK's client, whose `configuration` object legitimately omits an
  empty `acceptedOutputModes` and was answered with
  "invalid params: missing field `acceptedOutputModes`". All repeated
  fields now deserialize absent-as-empty
  (`SendMessageConfiguration.acceptedOutputModes`, `StringList.list`,
  `SecurityRequirement.schemes`, `TaskListResponse.tasks`,
  `ListPushConfigsResponse.configs`, `AgentCard.{supportedInterfaces,
  defaultInputModes, defaultOutputModes, skills}`, `AgentSkill.tags`,
  `Message.parts`, `Artifact.parts`). Semantic must-be-non-empty
  requirements are unchanged and enforced where they belong: the server
  still rejects empty message parts with a structured invalid-params
  error, event processors still drop empty-parts artifacts, and
  `AgentCard::validate()` still requires at least one interface — errors
  that now name the real problem instead of a JSON "missing field" type
  error. Serialized output is unchanged. Verified end-to-end against the
  official Python SDK (`a2a-sdk` 1.1.2): agent-card resolution, unary
  send, SSE streaming, and gRPC (all 33 golden fixtures plus live
  unary/streaming/mid-flight-subscribe probes) all pass in both
  directions.
- **`a2a-protocol-server` (WebSocket): accept-loop and handshake
  resilience** — a transient `accept()` error (per-connection abort,
  fd-table exhaustion) no longer tears down the WebSocket server; the
  accept loop now follows the same retry-with-backoff policy as the HTTP
  serve path. A peer that opens a TCP connection but never completes the
  WebSocket handshake is disconnected after a bounded handshake timeout
  (default 10 s, configurable via `with_handshake_timeout`) instead of
  pinning a connection and file descriptor indefinitely. The server also
  completes the WebSocket close handshake on shutdown paths, disables
  Nagle's algorithm to match the HTTP path's latency profile, answers
  binary frames with an explicit error instead of silence, and correlates
  "server busy"/"message too large" rejections with the request `id` so
  clients fail fast instead of timing out on an unroutable null-id error.
- **`a2a-protocol-client` (WebSocket): dropped transports leaked their
  connection; disconnects hung in-flight requests** — dropping a
  `WebSocketTransport` now aborts its background reader/writer tasks and
  closes the socket (a tokio `JoinHandle` detaches on drop, so every
  dropped transport previously leaked a task and an open TCP connection
  until the server closed it). A server-initiated close, end-of-stream, or
  write failure now fails **all** in-flight requests promptly with a
  transport error and marks the connection dead so subsequent requests
  fail fast — previously a clean server close left pending requests
  hanging for their full request timeout, and new requests kept queuing
  against the dead socket.
- **CI: per-benchmark regression tolerance** — the benchmark gate accepts
  targeted `--override PATTERN=THRESHOLD` globs, and the known-noisy
  `from_str/16384` benchmark (tiny absolute runtime; tight-CI swings on
  identical code while its size neighbours stay stable) now carries a 75%
  tolerance instead of the whole gate being loosened. Overridden rows are
  labelled in the gate output, and a regression past the raised threshold
  still fails.
- **`a2a-protocol-client`: every streaming transport now bounds the wait for
  the first event** — the HTTP JSON-RPC, REST, and gRPC streaming paths
  previously bounded only stream *establishment* (response headers /
  stream open); a server that accepted the stream and then went silent
  hung the consumer forever. All three now apply the same
  first-event timeout the WebSocket transport already had (lifted after
  the first frame — long-running quiet tasks are unaffected because SSE
  keep-alives and the spec-required initial Task event count as frames).
- **`a2a-protocol-client`: streaming error responses keep `Retry-After`** —
  a rate-limited stream start (HTTP 429/503 with `Retry-After`) surfaced
  `retry_after: None` on the JSON-RPC and REST paths, so the retry layer
  used its own short jittered backoff instead of the server-directed
  delay the unary paths already honor.
- **`a2a-protocol-client`: SSE parser accepts bare-CR line terminators** —
  the WHATWG SSE grammar allows CRLF, LF, or CR line endings; the parser
  handled only LF/CRLF, so a CR-only server's entire stream accumulated
  into one "line" and was rejected as oversized. CR now terminates a
  line, with a CRLF pair split across reads counted as one terminator.
- **`a2a-protocol-server`: stale-token sweep can no longer evict a live
  replacement token** — between candidate collection (read lock) and
  removal (write lock), a cancel-then-resend race can insert a fresh live
  cancellation token for the same task id; the sweep removed it by id
  unconditionally, leaving the resent executor uncancelable for its whole
  run. Removal now re-validates each entry under the write lock and
  spares tokens that are neither cancelled nor aged.
- **`a2a-protocol-server`: background event processor survives a missing
  task row at startup** — if the just-saved task vanished before the
  processor's initial store read (capacity eviction under extreme churn,
  or a transient store fault), the processor exited early and dropped its
  persistence receiver: every subsequent state transition and push
  notification for the task was silently lost while the client's stream
  kept delivering. It now falls back to the send path's task snapshot,
  re-asserting the row on a confirmed miss.
- **`a2a-protocol-server`: path-traversal detection decodes to a fixpoint** —
  the REST guard decoded percent-encoding exactly twice, so
  triple-encoded `..` passed undetected (not reachable today — routing
  matches raw segments — but the detector no longer encodes that
  assumption; undecodable-after-8-passes input now fails closed).
- **Docs:** honest-limits pass — `TaskStoreConfig::max_capacity` documents
  the last-resort eviction of non-terminal tasks under overload,
  `HandlerLimits` sweep thresholds are documented as prune triggers
  rather than hard bounds, `PerTenantConfig` documents fairness under
  shared process-wide caps, the retry layer documents that interceptors
  run once per call (not per attempt), and push-config creation documents
  its sync-check-at-create / DNS-recheck-at-delivery split.

### Changed (fourth hardening pass)

- **`ListTasks` returns tasks most-recently-updated first (spec §3.1.4)** —
  every task store previously returned tasks in ascending `id` order (an
  arbitrary lexical order the spec does not permit), and paged with an
  `id`-based cursor. All five stores now order by last-update time,
  most-recent first, with a stable cursor:
  - `InMemoryTaskStore` keys a `BTreeMap<u64, TaskId>` update-order index by
    a monotonic per-write sequence, iterated in reverse — O(log n +
    page_size) pagination with no per-call sort, and a collision-free
    integer cursor. Every write (not just the first insert) re-positions the
    task to the front, so an in-place update correctly moves it.
  - The SQLite/PostgreSQL stores (and their tenant-scoped variants) order by
    `(updated_at DESC, id DESC)` with a composite row-value cursor
    `(updated_at, id) < (?, ?)`; because `id` is the primary key the pair is
    unique, so pagination never drops or repeats a row even when many tasks
    share a timestamp. SQLite now records `updated_at` at millisecond
    precision (fixed-width, so `TEXT` comparison matches chronological
    order); PostgreSQL serializes the cursor timestamp as a UTC-normalized
    microsecond string so paging is stable regardless of session time zone.
    New `(updated_at, id)` indexes back the ordering (added via SQLite
    migration v4 / PostgreSQL migration v3).
- **Capability validation enforced at the handler (spec §3.3.4)** — when an
  `AgentCard` is configured, the server now honors its advertised
  `capabilities`. `SendStreamingMessage` and `SubscribeToTask` return
  `UnsupportedOperationError` unless `capabilities.streaming` is `true`; the
  push-config operations (Create/Get/List/Delete) return
  `PushNotificationNotSupportedError` unless `capabilities.pushNotifications`
  is `true`. A card-less handler is unaffected (it publishes no capability
  contract), so existing card-less deployments keep working.

### Fixed (fourth hardening pass)

- **`GetTaskPushNotificationConfig` reports a missing config as
  `TaskNotFoundError`** (spec §3.1.8) instead of `InvalidParams` — over REST
  this changes the HTTP status from 400 to 404, matching the canonical error
  mapping and the other SDKs.
- **`CreateTaskPushNotificationConfig` validates that the target task exists**
  (spec §3.1.7), returning `TaskNotFoundError` instead of silently storing an
  unroutable, orphaned config for a task that was never created.
- **Cross-binding metadata portability** — the send path now rejects a
  client-supplied `metadata` value (on the request, the message, or any
  message part) that is present but not a JSON object, with a structured
  invalid-params error. A protobuf `google.protobuf.Struct` — the gRPC wire
  form of every `metadata` field — can only hold a JSON object, so a task
  accepted over JSON-RPC/REST with, e.g., `metadata: [1, 2, 3]` would fail to
  serialize the instant it was served over gRPC. Rejecting it at ingress
  keeps every accepted task representable across all A2A transports.
- **JWT `exp` is now fail-closed at the boundary** (`auth-jwt`) — the
  expiration check accepted a token at exactly `exp + leeway` (`now > exp`).
  RFC 7519 §4.1.4 requires a token to be rejected "on or after" `exp`, so the
  check is now `now >= exp + leeway`. `nbf` was already correct (valid at
  exactly `nbf`, per §4.1.5 "not before"). Tokens sitting exactly at their
  expiry instant are now rejected.
- **Mutation-test hardening of the changed surface** — closed every gap the
  incremental mutation-testing gate found in this pass's diff, so the new and
  touched code is covered by assertions that pin behavior, not just line
  coverage. This added deterministic tests for JWT time-boundary and JWKS
  parsing/redaction logic, a shared unit-tested pagination boundary helper
  used by all five stores, and DER length-encoding tests — and extracted a
  couple of pure helpers (`check_claims_at(now)`, `cache_is_fresh`) so the
  crypto/validation boundaries are testable without a wall clock.

## [0.6.0] - 2026-06-10

Released as a **minor** (not patch) bump: no public API signatures changed,
but observable behavior did — `Task.history` is now populated in responses,
streaming disconnects no longer fail running tasks, and `Working → Working`
status refreshes are accepted. Consumers relying on the old behaviors should
read the entries below.

### Changed

- **`Task.history` is retrievable, and send responses opt in to it** —
  `tasks/get` returns the populated history (subject to `historyLength`;
  previously the field was always absent). `message/send` responses and
  streaming snapshots omit history unless
  `SendMessageConfiguration.historyLength` requests it: echoing the
  just-sent message back doubled response payloads for large sends (+95%
  median at 1 MiB, caught by the benchmark regression gate).
- **A dropped `message/stream` connection no longer cancels work** — tasks
  continue to completion and clients reattach via `SubscribeToTask`.
  Anything depending on disconnect-kills-task semantics must cancel
  explicitly via `CancelTask`.

### Fixed

- **`a2a-protocol-server`: a client disconnecting from `message/stream` no
  longer fails the running task** — The event-queue writer treated a
  broadcast send with zero live receivers as an executor-fatal error, so the
  only SSE consumer dropping its connection (network blip, closed tab)
  marked the in-flight task `TASK_STATE_FAILED` even though every event had
  already reached the persistence channel. Zero receivers is now a non-error
  whenever a persistence channel exists — the task keeps running and clients
  can reattach via `tasks/resubscribe` (which is the reason that method
  exists). Sync mode is unchanged: there the sole receiver *is* the request.

- **`a2a-protocol-types`: `Working → Working` is now a valid state
  transition** — Repeated `Working` status updates, each carrying a new
  `status.message`, are how an agent narrates long-running work to streaming
  clients. The state machine rejected the self-transition, so any executor
  that emitted more than one progress note had its task marked
  `TASK_STATE_FAILED` by the background processor *while the SSE stream
  delivered the same events and a final `Completed` to the client* — a
  stream/store split-brain. Self-transitions remain invalid for every other
  state.

- **`a2a-protocol-client`: JSON-RPC error responses to `message/stream` are
  surfaced instead of dissolving into an empty stream** — A streaming
  request that fails up-front (e.g. continuing a task with the wrong
  `contextId`) returns HTTP 200 with a JSON-RPC error envelope. The client
  fed that body to the SSE parser, which found no frames and ended the
  stream with zero events and no error. The transport now checks the
  response content type: JSON-RPC error envelopes map to
  `ClientError::Protocol` with the original code, and other non-SSE bodies
  map to `ClientError::Transport`.

- **`a2a-protocol-server`: `Task.history` is now actually populated** —
  Nothing ever appended messages to task history: tasks were created with
  `history: None`, continuations never added the new message, and both event
  processors explicitly ignored agent `Message` events. `GetTask`'s
  `historyLength` parameter (fixed in 0.3.4 to "truncate history") truncated
  a permanently empty list, and multi-turn executors could not see prior
  turns via `RequestContext::stored_task`. Incoming user messages are now
  appended at send time, agent `Message` events are recorded by both the
  sync and background processors, and history is capped at 1,024 messages
  (oldest dropped first).

- **`a2a-protocol-server`: continuations no longer wipe accumulated task
  state** — Sending a follow-up message to an existing non-terminal task
  saved a freshly constructed task over the stored one, destroying its
  accumulated artifacts, metadata, and history. Continuations now carry all
  three forward; only the status returns to `Submitted` for the new turn.

- **`a2a-protocol-server`: JSON-RPC legacy alias `tasks/resubscribe`** —
  The dispatcher accepted the v1.0 method `SubscribeToTask` and the alias
  `tasks/subscribe`, but not `tasks/resubscribe` — the actual method name
  from the v0.2.x spec that older clients send. All three now route to
  resubscription.

All of the above were found by driving the new `incident-response` example's
multi-turn, multi-agent flow end-to-end against a live local model — including
a full probe of resubscribe-after-disconnect (works: a reattached client
receives the remaining events and terminal state) and real webhook push
delivery (works: deliveries carry the `a2a-notification-token` header and
continue while no SSE consumer is connected).

### Added

- **`incident-response` example** — A three-agent incident-response team
  (triage orchestrator + deterministic log-search agent + LLM runbook agent)
  demonstrating `INPUT_REQUIRED` multi-turn continuation, agent-to-agent
  delegation, streaming progress narration, artifacts, cooperative
  cancellation, and honest failure states. Runs fully local (llama-server /
  Ollama) or with hosted providers, and passes the TCK 20/20.
- **TCK: `a2a_media_type_accepted` conformance test** — Servers must accept
  the registered A2A media type `application/a2a+json` that production
  clients send, not just plain `application/json`. Added after the Rust
  client's requests were rejected by the JS ITK agent, whose JSON body
  parser was content-type-strict (fixed too); the cross-language CI matrix
  now enforces this for every agent.

### Security

- **`rustls-webpki` upgraded to 0.103.12** — Fixes RUSTSEC-2026-0098
  ([GHSA-965h-392x-2mh5](https://github.com/rustls/webpki/security/advisories/GHSA-965h-392x-2mh5)):
  name constraints for URI names were incorrectly accepted during X.509 path
  validation, which could allow a CA with URI `NameConstraints` to be bypassed
  for certificates it should have been restricted from issuing. The bug
  affected all rustls-webpki releases in the 0.103.x line through 0.103.11 and
  reaches `a2a-protocol-client` transitively via `rustls` → `hyper-rustls`
  / `tokio-rustls` whenever the `tls-rustls` feature is enabled.

  Exploitability in practice requires a trust chain whose CA uses URI name
  constraints — uncommon in the public Web PKI (Mozilla/`webpki-roots`), but
  possible for consumers wiring a private or enterprise CA bundle into the
  client. Out of caution this is shipped as a dedicated patch release so
  downstreams can pick it up without waiting for unrelated changes.

  No code changes are required by consumers; `cargo update -p rustls-webpki`
  on existing lockfiles is sufficient for anyone not yet moving to 0.5.1.

## [0.5.0] - 2026-04-02

### Breaking Changes

- **`TaskStore::save()` and `TaskStore::insert_if_absent()` now accept `&Task`
  instead of owned `Task`** — This eliminates forced `.clone()` at every call
  site. Store implementations that need ownership (e.g., `InMemoryTaskStore`)
  clone internally; database-backed stores (`SqliteTaskStore`,
  `PostgresTaskStore`) borrow fields directly and never clone.

  **Migration guide:**
  ```rust
  // Before (0.4.x):
  store.save(task.clone()).await?;
  store.insert_if_absent(task).await?;

  // After (0.5.0):
  store.save(&task).await?;
  store.insert_if_absent(&task).await?;
  ```

  Custom `TaskStore` implementations must update their method signatures:
  ```rust
  // Before:
  fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

  // After:
  fn save<'a>(&'a self, task: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
  ```

- **Version bump: 0.4.1 → 0.5.0** — All four crates (`a2a-protocol-types`,
  `a2a-protocol-client`, `a2a-protocol-server`, `a2a-protocol-sdk`) are bumped
  to 0.5.0 to signal the breaking `TaskStore` trait change.

### Performance

- **Broadcast channel capacity increased from 64 to 256 events** — Pushes
  the per-event cost inflection from ~52 to ~252 events, reducing broadcast
  buffer pressure for high-volume streaming tasks.
- **`serde_helpers` module** (`a2a-protocol-types`) — `SerBuffer` provides
  thread-local reusable serialization buffers (2.3× less overhead on small
  payloads); `deser_from_str`/`deser_from_slice` enable borrowed
  deserialization (~15-25% fewer allocations).
- **SSE frame building uses thread-local reusable buffer** — Amortized 0
  allocations per event vs previous 1 allocation per event.
- **267 benchmarks, zero panics, zero errors** — Cleanest benchmark run in
  project history. All 13 benchmark suites (transport, protocol, lifecycle,
  concurrency, cross-language, realistic, error paths, backpressure, data
  volume, memory, enterprise, production, advanced) pass with zero failures.
- **Streaming bimodal distribution fully resolved** — Zero streaming benchmarks
  appear in the high-outlier list. `stream_drain` confidence interval tightened
  from [1.79ms, 2.11ms] (18% range) to [1.59ms, 1.67ms] (5% range).
- **Agent burst sub-linear scaling confirmed** — Per-agent cost drops from
  714µs/agent (10 agents) to 310µs/agent (100 agents). SDK handles high-fanout
  agent coordination without degradation.
- **Subscribe fan-out O(1) up to 5 subscribers** — 1 subscriber = 2.90ms,
  5 subscribers = 2.89ms. Broadcast channel delivers in a single pass.
- **Pagination context index 2x speedup** — Filtered walk at 1K tasks: 309µs
  vs unfiltered 592µs. BTreeSet context index eliminates half the scan work.
- **Tenant resolvers effectively free** — 88–173ns per request (~0.008% of a
  typical 1.6ms round-trip).
- **SSE streaming bimodal distribution eliminated** — Root-caused the ~24%
  high severe outlier rate in all streaming benchmarks to cross-thread task
  scheduling: on a 4-core system, `tokio::spawn` has a 3/4 probability of
  placing the SSE builder task on a different worker thread, causing a ~500µs
  cache-miss + work-stealing penalty. Three production fixes applied:
  1. Replaced `tokio::time::interval` with `tokio::time::sleep` + reset
     pattern in `build_sse_response` — eliminates persistent timer wheel
     registration during active streaming
  2. Added `tokio::task::yield_now()` before read loops in SSE builder
     (server) and body reader tasks (client JSON-RPC + REST)
  3. Transport streaming benchmarks now use `worker_threads(1)` runtime
     and streaming-specific warmup, reducing outliers from 24 high severe
     to 4 high mild and tightening confidence intervals by 3×

### Fixed

- **Benchmark server `AddrInUse` on CI** — Benchmark servers now set
  `SO_REUSEADDR` + `SO_REUSEPORT` via `socket2` and use a graceful shutdown
  handle (`watch::Sender<bool>`) so that rapid server cycling during cold-start
  benchmarks does not fail with `Address already in use` on CI runners where
  `TIME_WAIT` recycling is slower.
- **Criterion timeout warnings eliminated (round 2)** — Bumped `measurement_time`
  for 5 additional benchmark groups based on CI analysis: `transport/payload_scaling`
  (8s→10s), `concurrent/sends` (18s→30s), `realistic/payload_complexity` (10s→15s),
  `realistic/connection` (10s→15s), `enterprise/client_interceptors` (8s→10s).
  All 267 benchmarks now complete within their budget on CI runners.
- **Push config benchmark per-task limit** — `production/push_config/set_roundtrip`
  and `delete_roundtrip` now upsert a pre-created config instead of creating new
  configs each iteration, preventing `push config limit exceeded` panics during
  criterion warmup.

### Benchmarks

- **Transport payload scaling extended to 1MB** — Added 100KB and 1MB payload
  sizes to `transport_throughput.rs` for large-payload regression detection.
- **New `protocol/payload_scaling` isolation benchmarks** — Pure serde cost
  from 64B to 1MB in `protocol_overhead.rs`; compares `to_vec` vs `SerBuffer`
  and `from_slice` vs `from_str` for serde regression detection.
- **Cache-busting step for `data_volume/get` at 100K** — 4MB allocation to
  flush CPU caches between populate and measure, eliminating the cache warming
  artifact.
- **Documentation comments added** — Connection reuse best practices, cold
  start vs steady state explanation, concurrent store anomaly notes added to
  benchmark files.

### Changed

- **Benchmark documentation expanded** — Added 8 new "Known Measurement
  Limitations" entries to `benches/README.md` and the auto-generated GH Book
  benchmarks page: data_volume/save wide CIs, dispatch routing inverted results,
  cold start vs steady state, subscribe fan-out O(1) scaling, agent burst
  sub-linear scaling, tenant resolver overhead, pagination context index speedup.
  These complement the existing entries for streaming bimodal distribution,
  get()/100K cache anomaly, stream volume per-event cost inflection, and slow
  consumer timer calibration.
- **Stream volume scaling documentation** — Added detailed per-event cost
  analysis comments to `backpressure.rs` explaining the broadcast channel
  capacity-driven inflection at 252+ events.

### Performance

- **`a2a-protocol-server`: `InMemoryTaskStore::list()` O(n log n) → O(log n + page_size)** —
  Added `BTreeSet<TaskId>` sorted index and `HashMap<String, BTreeSet<TaskId>>`
  context_id secondary index. Eliminates the per-call sort that caused 20-70×
  regressions at 10K+ tasks. Uses `BTreeSet::range()` for O(log n) cursor
  positioning.
- **`a2a-protocol-server`: SSE per-event allocation reduced** — New
  `build_sse_message_frame()` serializes JSON directly into the SSE frame
  buffer via `serde_json::to_writer`, reducing per-event allocations from 2 to 1.
- **`a2a-protocol-types`: Part deserialization ~80 fewer allocations per Task** —
  Replaced `#[serde(flatten)]` on `Part.content` with a hand-rolled `Deserialize`
  implementation that reads all fields in a single pass without intermediate
  `serde_json::Value` buffering.

### Added

- **`advanced_scenarios` benchmark suite** — Tenant resolver overhead (header,
  bearer, path segment extraction); agent card hot-reload (read, update, complex
  card swap); `/.well-known/agent.json` discovery endpoint latency; subscribe
  fan-out (1–10 concurrent subscribers); streaming artifact accumulation cost
  (`task.clone()` at 0–500 artifact depth); pagination full walk (100–1K tasks,
  unfiltered + context-filtered); extended agent card round-trip.
- **`production_scenarios` benchmark suite** — SubscribeToTask reconnection,
  cold start vs steady-state, concurrent cancel+subscribe race, 7-step E2E
  orchestration, push config CRUD round-trip, parallel agent burst (10-100
  agents), dispatch routing isolation.
- **Timer calibration benchmark** — Measures actual `tokio::time::sleep()`
  duration to isolate CI timer jitter from real SDK overhead.
- **`NoopPushSender`** for benchmarks that require push notification support
  without performing actual HTTP webhook delivery.
- **`start_jsonrpc_server_with_push()`** helper for benchmark servers with push
  notification capabilities enabled.

### Fixed

- **`MultiEventExecutor` invalid state transitions** — Was emitting
  `Working → Working` status events in a loop, violating the A2A spec state
  machine. Now emits `Working` once, then N artifact events, then `Completed`.
- **`production_scenarios` push config benchmark** — Was using a server without
  push notification support, causing `PushNotificationNotSupported` errors.
- **`production_scenarios` dispatch routing benchmark** — Pre-allocate params
  outside the measurement loop for `direct_handler_invoke` to isolate handler
  dispatch cost from fixture allocation cost, producing a fairer comparison
  against the HTTP round-trip path.
- **`InMemoryTaskStore::insert()` unnecessary index operations** — Update path
  now skips BTreeSet and context index operations when the task already exists
  with the same context_id, eliminating variance from occasional BTreeSet node
  splits and reducing update cost from ~2.5µs to ~700ns.
- **Criterion `measurement_time` warnings** — Added `measurement_time` to 23+
  benchmark groups across 8 files, eliminating all 15 warnings and preventing
  23 borderline cases from triggering on CI runners.

## [0.4.1] - 2026-03-31

### Fixed

- **`a2a-protocol-client`: REST streaming deserialization failure** — The REST
  binding sends bare `StreamResponse` JSON in SSE frames (per A2A spec Section
  11.7), but the client always tried to unwrap a JSON-RPC envelope, causing
  `"data did not match any variant of untagged enum JsonRpcResponse"` errors.
  `EventStream` now tracks the transport binding and parses bare responses for
  REST streams.

## [0.4.0] - 2026-03-31

### Breaking Changes

This release implements full A2A v1.0.0 wire format compliance. The following
changes are **breaking** — existing clients and servers using the old wire format
will need to update.

- **Part wire format migrated to v1.0 flat oneof** — Parts no longer use
  `{"type": "text", "text": "..."}` discriminated format. The new format uses
  JSON member name as discriminator: `{"text": "..."}`, `{"raw": "base64..."}`,
  `{"url": "https://..."}`, `{"data": {...}}`. Top-level `filename` and
  `mediaType` fields replace the nested `file` object. The `PartContent` enum
  now has `Text`, `Raw`, `Url`, and `Data` variants (previously `Text`, `File`,
  `Data`). The old `FileContent` struct is retained for backward-compatible
  constructors only.

- **Enum serialization uses ProtoJSON SCREAMING_SNAKE_CASE** — `TaskState` now
  serializes as `"TASK_STATE_COMPLETED"`, `"TASK_STATE_INPUT_REQUIRED"`, etc.
  (previously `"completed"`, `"input-required"`). `MessageRole` now serializes
  as `"ROLE_USER"`, `"ROLE_AGENT"` (previously `"user"`, `"agent"`). Legacy
  lowercase values are still accepted on deserialization via serde aliases.

- **`SendMessageResponse` uses externally tagged format** — Now serializes as
  `{"task": {...}}` or `{"message": {...}}` per proto `oneof payload` semantics
  (previously untagged — just the inner object).

- **Agent Card well-known path changed** — Discovery path is now
  `/.well-known/agent-card.json` (previously `/.well-known/agent.json`),
  matching the spec Section 8.2 and IANA registration (Section 14.3).

- **`OAuthFlows` is now an enum (oneof)** — Previously a struct with five
  optional fields, now an enum with one variant per flow type, matching the
  proto `oneof flow` definition. Only one OAuth flow can be specified at a time.

- **Error responses use AIP-193 format** — REST errors now follow
  `{"error": {"code": N, "status": "...", "message": "...", "details": [...]}}`.
  JSON-RPC errors include `google.rpc.ErrorInfo` in the `data` array with
  `@type`, `reason` (UPPER_SNAKE_CASE), and `domain` ("a2a-protocol.org").

### Fixed

- **HTTP error status codes corrected for all 9 A2A error types** — Per Section
  5.4: `ContentTypeNotSupported` → 415, `InvalidAgentResponse` → 502,
  `UnsupportedOperation` → 400, `ExtendedAgentCardNotConfigured` → 400,
  `ExtensionSupportRequired` → 400, `VersionNotSupported` → 400.

- **gRPC error status codes corrected for all 9 A2A error types** — Per Section
  5.4: `UnsupportedOperation`/`VersionNotSupported` → `UNIMPLEMENTED`,
  `ContentTypeNotSupported` → `INVALID_ARGUMENT`,
  `ExtendedAgentCardNotConfigured`/`ExtensionSupportRequired` →
  `FAILED_PRECONDITION`.

- **Blocking SendMessage now returns on interrupted states** — Per Section 3.2.2,
  `return_immediately=false` operations now correctly return when the task
  reaches `INPUT_REQUIRED` or `AUTH_REQUIRED`, not just terminal states.

- **`ListTasks` `includeArtifacts` parameter now applied** — Per Section 3.1.4,
  when `includeArtifacts` is false (the default), artifacts are omitted entirely
  from task responses.

### Added

- **`ErrorCode::a2a_reason()`** — Returns the UPPER_SNAKE_CASE reason string
  for `google.rpc.ErrorInfo`.
- **`ErrorCode::http_status()`** — Returns the correct HTTP status code per
  Section 5.4.
- **`ErrorCode::grpc_status()`** — Returns the correct gRPC status string per
  Section 5.4.
- **`A2aError::error_info_data()`** — Builds the `google.rpc.ErrorInfo` data
  array for error responses.
- **`TaskState::is_interrupted()`** — Returns true for `InputRequired` and
  `AuthRequired` states.
- **Missing error constructors** — `push_not_supported()`,
  `content_type_not_supported()`, `extension_support_required()`,
  `version_not_supported()`.

## [0.3.4] — Unpublished

> A standalone 0.3.4 release was never tagged or published to crates.io;
> the changes below first shipped as part of [0.4.0] - 2026-03-31.

### Fixed

- **`a2a-protocol-server`: SendMessage now rejects messages to tasks in terminal
  state** — Per A2A spec CORE-SEND-002, tasks in Completed, Failed, Canceled, or
  Rejected state cannot accept further messages. Previously, messages sent to
  terminal tasks were silently accepted and forwarded to the executor. Now returns
  `UnsupportedOperation` error. (Cross-SDK learning from a2a-java#741)

- **`a2a-protocol-server`: SendMessage with unknown taskId now returns
  TaskNotFound** — Per A2A spec section 3.4.2, when a client includes a `taskId`
  in a Message, it must reference an existing task. Previously, a client-provided
  `taskId` that didn't exist would create a new task with that ID. Now correctly
  returns `TaskNotFound` error. (Cross-SDK learning from a2a-java#766)

- **`a2a-protocol-server`: GetTask and ListTasks now apply `historyLength`
  parameter** — The `history_length` parameter was accepted in query/params but
  never actually used to truncate the message history in responses.
  `historyLength=0` now correctly returns no history, and positive values return
  only the N most recent messages. (Cross-SDK learning from a2a-python#573)

- **`a2a-protocol-server`: SubscribeToTask on terminal task now returns
  `UnsupportedOperation`** — Per A2A spec section 3.1.6, subscribing to a task
  in a terminal state should return `UnsupportedOperation`, not a generic
  internal error. (Cross-SDK learning from a2a-java#767)

### Added

- **`a2a-protocol-types`: `Artifact::validate()` method** — Validates that an
  artifact's `parts` list is non-empty per A2A spec requirements. Server-side
  event processing now validates artifacts before persisting them.
  (Cross-SDK learning from a2a-python#670)

- **`a2a-protocol-types`: `Part::text_content()` accessor** — Returns the text
  content of a text part, or `None` for non-text parts.

- **`a2a-protocol-server`: `ServerError::UnsupportedOperation` variant** — New
  error variant that maps to `ErrorCode::UnsupportedOperation` (-32004) for
  operations that are not valid for the current task state.

- **`a2a-protocol-server`: `SendMessageResult` now implements `Debug`** — Added
  `#[derive(Debug)]` to improve error messages in tests and logging.

- **`a2a-protocol-server`: SubscribeToTask emits Task snapshot as first event** —
  Per A2A spec, the first event in a `SubscribeToTask` stream must be a Task
  object representing the current state, preventing clients from missing state on
  reconnection. Added `EventQueueManager::subscribe_with_snapshot()`.
  (Cross-SDK learning from a2a-go#231, a2a-js#323)

- **`a2a-protocol-client`: `ClientBuilder::from_card()` preserves tenant** —
  The `tenant` field from `AgentInterface` was silently dropped when constructing
  a client from an `AgentCard`. Now preserved in `ClientConfig::tenant` and
  automatically applied to `SendMessage` requests. Added `with_tenant()` builder
  method for explicit configuration. (Cross-SDK learning from a2a-java#772)

- **`a2a-protocol-client`: `ClientConfig::tenant` field** — New optional field
  for default tenant in multi-tenancy scenarios.

- **`a2a-protocol-server`: A2A-Version header validation on incoming requests** —
  Both JSON-RPC and REST dispatchers now validate the `A2A-Version` header if
  present. Requests with incompatible major versions (not 1.x) are rejected with
  `VersionNotSupported` (-32009). (Cross-SDK learning from a2a-python#865)

- **`a2a-protocol-client`: JSON-RPC response ID validation** — The client now
  verifies that the response `id` matches the request `id` in JSON-RPC
  responses. Previously, any response was accepted regardless of ID, which could
  cause silent data corruption in pipelined scenarios.
  (Cross-SDK learning from a2a-js#318)

- **`a2a-protocol-server`: Artifact append now merges parts AND metadata** —
  When `TaskArtifactUpdateEvent` has `append=true`, the server now correctly
  merges parts into the existing artifact and deep-merges metadata (new keys
  override existing). Previously, appended artifacts were pushed as separate
  entries, losing the merge semantics. Both sync and background event processors
  are fixed. (Cross-SDK learning from a2a-python#735, a2a-java#615)

- **`a2a-protocol-types`: `TaskListResponse` fields are now required per spec** —
  `next_page_token` (`String`), `page_size` (`u32`), and `total_size` (`u32`)
  are now always present on the wire (not `Option`), matching the proto
  definition. Empty `next_page_token` means no more pages. All task store
  implementations now populate these fields.

- **`a2a-protocol-server`: `SendStreamingMessage` emits Task snapshot as first
  event** — Per A2A spec section 3.1.2, the first event in any streaming response
  MUST be a `Task` object. Previously only `SubscribeToTask` did this.

- **`a2a-protocol-server`: `GetExtendedAgentCard` capability check** — Per spec
  section 3.1.11, returns `UnsupportedOperationError` when
  `capabilities.extended_agent_card` is false/absent, and
  `ExtendedAgentCardNotConfiguredError` when capability is declared but no card
  is configured. Previously returned a generic internal error for both cases.

## [0.3.3] - 2026-03-30

### Fixed

- **`a2a-protocol-server`: `find_task_by_context` now prefers non-terminal tasks** —
  When multiple tasks shared the same `context_id` (e.g. after a task reached a
  terminal state and a new one was created), the lookup used `page_size=1` and
  returned whichever task the store ordered first, which could be the stale
  terminal task. Now fetches up to 10 candidates and returns the first
  non-terminal (active) task, falling back to the first terminal task only when
  no active task exists.

- **`a2a-protocol-server`: `context_locks` map no longer grows without bound** —
  The per-context mutex map (used to serialize concurrent `SendMessage` requests
  for the same `context_id`) never cleaned up stale entries, causing unbounded
  memory growth under sustained traffic with diverse context IDs. Stale locks
  (where no task holds a reference) are now pruned when the map exceeds the
  configurable `max_context_locks` limit (default 10,000).

- **`a2a-protocol-server`: `PayloadTooLarge` now returns correct JSON-RPC error
  code** — Previously mapped to `InternalError` (-32603), now correctly returns
  `InvalidRequest` (-32600) since an oversized payload is a client error.

- **`a2a-protocol-server`: params-level `context_id` now validated** — The
  `context_id` field at the `MessageSendParams` level (which takes precedence
  over `message.context_id`) was not checked by `validate_id()`, allowing
  empty/whitespace-only or excessively long values to bypass validation.

- **`a2a-protocol-server`: `eviction_interval=0` no longer panics** — Setting
  `TaskStoreConfig::eviction_interval` to 0 caused a panic in
  `u64::is_multiple_of(0)`. Now treated as "disable periodic eviction" (only
  capacity-based eviction triggers).

- **`a2a-protocol-server`: push config `list()` now returns deterministic order** —
  The in-memory push config store iterated over a `HashMap`, producing
  non-deterministic ordering. Results are now sorted by `(task_id, config_id)`.

- **`a2a-protocol-server`: cancel task TOCTOU race narrowed** — `on_cancel_task`
  now re-reads the task immediately before writing `Canceled` state, preventing
  it from overwriting a concurrent terminal transition (e.g. `Completed`) that
  occurred between the initial check and the save.

- **`a2a-protocol-server`: `page_size` clamped at handler level** — `ListTasks`
  now clamps `page_size` to 1000 at the handler layer before passing to the
  store, preventing oversized allocations from untrusted client input.

- **`a2a-protocol-server`: tenant store no longer allocates on read** — Read-only
  operations (`get`, `list`, `count`, `delete`) on the tenant-aware in-memory
  store no longer create a new tenant partition, closing a DoS vector where
  read requests with unknown tenant IDs could exhaust `max_tenants`.

- **`a2a-protocol-server`: `from_pool()` schema now matches `with_migrations()`** —
  The `SqliteTaskStore::from_pool()` and `TenantSqliteTaskStore::from_pool()`
  constructors now create the `created_at` column and composite
  `(context_id, state)` index, matching the schema produced by
  `with_migrations()`.

- **`a2a-protocol-server`: JSON-RPC serialization errors no longer produce `null`
  results** — `success_response` and `success_response_bytes` now return proper
  JSON-RPC error responses instead of silently producing `null` result values
  when `serde_json::to_value` fails. The `internal_serialization_error` fallback
  now returns HTTP 200 per JSON-RPC spec.

- **`a2a-protocol-types`: `MessageRole` serializes as lowercase** — Now serializes
  as `"user"` / `"agent"` per the A2A v1.0 JSON wire format, instead of
  proto-style `"ROLE_USER"` / `"ROLE_AGENT"`. The proto-style values remain as
  deserialization aliases for backward compatibility.

- **Unused example dependencies removed** — Removed `rig-core` from `rig-agent`
  and `bytes` from `echo-agent` examples.

## [0.3.2] - 2026-03-30

### Fixed

- **`a2a-protocol-server`: task_id not reused for non-terminal continuations** —
  `on_send_message` unconditionally generated a new `task_id` even when the
  client sent a `task_id` matching an existing non-terminal task (e.g.
  `input-required`). This violated A2A spec §3.4.3 (multi-turn conversation
  patterns) and caused non-deterministic `invalid params` errors on subsequent
  messages due to duplicate tasks per `context_id`. The handler now reuses the
  client-provided `task_id` when it matches the stored task. (#66)

## [0.3.1] - 2026-03-21

### Security

- **`rustls-webpki` upgraded to 0.103.10** — Fixes RUSTSEC-2026-0049
  ([GHSA-pwjx-qhcg-rvj4](https://github.com/rustls/webpki/security/advisories/GHSA-pwjx-qhcg-rvj4)):
  when a certificate had more than one `distributionPoint`, only the first was
  matched against each CRL's `IssuingDistributionPoint`, causing subsequent
  distribution points to be silently ignored and valid CRLs to be skipped.

### Fixed (Benchmarks)

- **`data_volume/save` eviction interference** — The `bench_save_at_scale` and
  `bench_store_with_history` benchmarks accumulated tasks across all criterion
  warmup and measurement iterations (sharing one store instance), triggering
  O(n log n) eviction scans every 64 writes once the store exceeded 10K tasks.
  Disabled eviction with `TaskStoreConfig { max_capacity: None, task_ttl: None }`
  to measure pure insert performance. Results: ~580µs → ~1.5µs (400× improvement).
- **`protocol_type_serde` throughput misreporting** — Throughput was set once for
  `AgentCard` but applied to all subsequent `Task` and `Message` benchmarks,
  causing incorrect bytes/sec reporting. Now sets correct `Throughput::Bytes`
  before each type's benchmark.
- **`realistic_workloads` fixture allocation in hot loop** — `mixed_parts_message()`
  and `nested_metadata()` were constructed inside `b.iter()` closures, measuring
  fixture creation cost instead of the send operation. Moved to pre-construction
  outside the hot loop.
- **Concurrent benchmark params allocation in hot loop** — `send_params()` with
  `format!` was called inside `b.iter()` closures in `concurrent_agents`,
  `cross_language`, and `backpressure` benchmarks. Pre-allocated params `Vec`
  outside the hot loop to avoid measuring allocation overhead.
- **`fixtures::large_metadata_message` unnecessary String clone** — Changed from
  cloning a `String` then wrapping in `Value::String` per entry to cloning the
  pre-built `Value::String` directly, eliminating one allocation per metadata entry.

### Improved (Performance)

- **`TCP_NODELAY` on all sockets** — Enabled `TCP_NODELAY` on server accept
  sockets and all client `HttpConnector` instances (JSON-RPC, REST, TLS,
  discovery). Eliminates ~40ms Nagle/delayed-ACK latency that caused constant
  overhead on SSE streaming regardless of event count.
- **`InMemoryTaskStore` switched to `BTreeMap`** — Replaced `HashMap` with
  `BTreeMap<TaskId, TaskEntry>` (added `Ord` to `TaskId`). List queries now
  use `BTreeMap::range()` for O(page_size) cursor seek instead of O(n) full
  scan + O(m log m) sort + O(m) clone. Benchmarked improvements:
  1K tasks 346µs → 20µs (17×), 10K tasks 4.2ms → 27µs (153×),
  100K tasks 4.5ms → 27µs (164×).
- **Batch request clone removal** — JSON-RPC batch dispatch now takes ownership
  of the parsed `Value::Array` instead of cloning each item, eliminating one
  heap allocation per batch element.
- **Benchmark server `TCP_NODELAY`** — The in-process benchmark server was
  missing `TCP_NODELAY`, causing all streaming benchmarks to report ~44ms
  (Nagle delay) instead of actual SDK latency (~1.5ms). Benchmark streaming
  results now accurately reflect SDK overhead.

### Fixed (CI)

- **`memory_overhead` benchmark crash** — The benchmark encoded deterministic
  allocation counts as `Duration::from_nanos()`, producing identical samples
  that caused criterion's statistical analysis to panic on NaN. Now measures
  real wall-clock time and verifies allocation counts via assertions.

## [0.3.0] - 2026-03-19

### Fixed (CI / Release Pipeline)

- **Release workflow missing `protoc`** — The release workflow uses
  `--all-features` which enables the `grpc` feature, requiring `protoc` for
  proto compilation via `tonic-build`. Added `arduino/setup-protoc` (same
  action used in `ci.yml`) to all four release jobs that build crates: CI
  matrix, package, publish-dry-run, and publish.
- **Benchmark `NoopExecutor` invalid state transition** — The `NoopExecutor`
  used by the `minimal_overhead` benchmark attempted a direct
  `Submitted → Completed` transition, which the state machine rejects. Added
  the required intermediate `Working` state update.

### Improved (Performance)

- **`JsonRpcVersion` deserialization** — Replaced `String::deserialize` with a
  zero-allocation `visit_str` visitor, eliminating a heap allocation on every
  JSON-RPC envelope (2× per request/response cycle).
- **`SendMessageResponse` deserialization** — Removed unnecessary
  `Value::clone()` in the no-`role` branch. When `"role"` is absent the value
  must be a `Task`, so the fallback path was dead code with a wasted clone.
- **Metadata size validation** — Replaced `serde_json::to_string` (allocates a
  throwaway `String`) with a zero-allocation byte-counting writer via
  `serde_json::to_writer` for the metadata size check on every `SendMessage`.

### Fixed (Dogfooding — Pass 16)

- **`ListPushConfigs` response format mismatch** — Both REST and JSON-RPC
  dispatchers now correctly wrap push config list results in
  `ListPushConfigsResponse { configs, next_page_token }` instead of serializing
  a bare `Vec`. Previously, every `list_push_configs` call via REST or JSON-RPC
  failed with a deserialization error. The Axum adapter and gRPC service were
  already correct.
- **Push config URL validation ignores `allow_private_urls`** — The handler now
  consults the push sender's `allows_private_urls()` method before rejecting
  loopback/private webhook URLs at config creation time. Previously,
  `allow_private_urls()` on `HttpPushSender` only affected delivery-time
  validation, not config-creation validation, causing all private-URL configs
  to be rejected even in testing environments.
- **Background processor silent exit on store failure** — The streaming
  background event processor now logs distinct error messages when the task
  store read fails or returns `None`, instead of silently returning.

### Added (Dogfooding — Pass 16)

- **`PushSender::allows_private_urls()`** — New trait method (default: `false`)
  that lets the handler query whether the push sender allows private/loopback
  webhook URLs. `HttpPushSender` implements it based on its
  `allow_private_urls` field.

### Fixed (Dogfooding — Pass 12)

- **`truncate_body` UTF-8 panic** — Response body truncation for error messages
  now uses char-boundary-safe slicing instead of byte-offset slicing. Previously,
  non-ASCII error responses (common with international error messages) could panic
  when the truncation point fell inside a multi-byte UTF-8 character.
- **SSE parser line buffer OOM** — The SSE parser now caps `line_buf` growth at
  2× `max_event_size` to prevent a malicious server from causing OOM by sending a
  single very long line without newlines.
- **`get_extended_agent_card` ignoring interceptor params** — The
  `GetExtendedAgentCard` method now forwards interceptor-modified params instead
  of discarding them and sending an empty object.
- **REST path parameter injection** — Path parameters (task IDs, config IDs) are
  now percent-encoded before interpolation into REST URLs, preventing path
  traversal via IDs containing `/` or `..`.
- **Silent-pass tests** — `test_list_tasks_context_filter` now correctly fails on
  wrong task count; `test_stale_page_token` validates error messages.

### Known Limitations

The following issues were identified during deep analysis at the time of this
release. **Every one of them has since been fixed** — the historical text is
kept for the record, with resolution notes:

- **Broadcast channel lag** — *(Resolved: the background event processor now
  consumes a dedicated lossless mpsc persistence channel that is unaffected
  by SSE backpressure, and a lagging streaming consumer receives an explicit
  marked stream error instead of silent event loss — see 0.7.0.)* If an SSE
  consumer falls behind, the broadcast channel drops events for both the SSE
  reader and the background event processor. State transitions missed by the
  background processor are not persisted to the task store.
- **SSRF DNS rebinding** — *(Resolved: `HttpPushSender` resolves DNS and pins
  the validated address for the actual connection.)* `HttpPushSender`
  validates webhook URLs against private IP patterns but does not resolve
  DNS.
- **WebSocket message size** — *(Resolved: the WebSocket dispatcher enforces
  a configurable message/frame size cap at the protocol level.)* The
  WebSocket dispatcher does not enforce a message size limit.
- **SQL push config stores** — *(Resolved: the SQLite and Postgres push
  config stores enforce per-task and global bounds.)* Unlike
  `InMemoryPushConfigStore`, the SQLite and Postgres push config stores do
  not enforce per-task or global config limits.

### Fixed (v0.3.0 Hardening — Pass 11)

- **Retry jitter** — backoff now applies full jitter (0.5–1.0× randomization)
  using `std::hash::RandomState`, preventing thundering-herd retry storms when
  multiple clients experience the same transient failure. No `rand` dependency.
- **gRPC timeout retryability** — `tonic::Code::DeadlineExceeded` and `Cancelled`
  now map to `ClientError::Timeout` (retryable) instead of `ClientError::Protocol`
  (non-retryable). `Unavailable` maps to `HttpClient` (retryable). Fixes silent
  failure-to-retry when switching from REST to gRPC transport.
- **SSRF validation at config creation** — push webhook URLs are validated for
  private/loopback addresses when the config is created, not just at delivery
  time. Closes the window where malicious URLs could be stored.
- **Push delivery amplification cap** — total push delivery time per event is
  capped at 30 seconds in both sync and background processors, preventing DoS
  via 100 slow webhook endpoints (previously unbounded: up to 25 minutes).
- **`connection_timeout` validation** — `ClientBuilder::build()` now rejects
  `Duration::ZERO` for `connection_timeout`, matching existing validation for
  `request_timeout` and `stream_connect_timeout`.
- **Streaming interceptor lifecycle** — `stream_message()` and
  `subscribe_to_task()` now call `run_after()` with a synthetic 200 response
  after stream establishment. Previously only non-streaming methods called the
  after-hook, leaving interceptors without cleanup/logging opportunities.
- **gRPC per-request timeouts** — `execute_unary` and `execute_streaming` are
  now wrapped in `tokio::time::timeout()` for per-request enforcement, matching
  REST/JSON-RPC behavior. Also sets `tonic::Request::set_timeout()`.

### Changed (v0.3.0 Hardening — Pass 11)

- **Breaking:** `ClientBuilder::from_card()` now returns `ClientResult<Self>`
  instead of `Self`. Agent cards with no `supported_interfaces` return
  `ClientError::InvalidEndpoint`, matching server-side validation.
- **Breaking:** `CallContext` fields are now private. Use accessor methods:
  `method()`, `caller_identity()`, `extensions()`, `request_id()`,
  `http_headers()`. Prevents interceptors from mutating security-critical
  context mid-request.
- **Breaking:** `HandlerLimits` zero values (`max_id_length=0`,
  `max_metadata_size=0`, `push_delivery_timeout=0`) are now rejected at
  `RequestHandlerBuilder::build()` time instead of failing at runtime.
- `with_task_store_config()` now triggers `debug_assert!` if called after
  `with_task_store()`, catching the silent-ignore footgun in development.

### Fixed (v0.3.0 Hardening — Pass 10.5)

- **Client timeout enforcement** — `connection_timeout` is now applied to the
  underlying `HttpConnector` via `set_connect_timeout()`. Previously configured
  but never enforced, causing TCP connections to hang for the OS default (~2 min)
  when servers were unreachable. Response body collection is now wrapped in
  `tokio::time::timeout()` to prevent slow-body hangs.
- **Multi-tenant data leak** — `find_task_by_context()` now uses
  `TenantContext::current()` instead of hardcoded `tenant: None`, preventing
  cross-tenant context lookups in multi-tenant deployments.
- **Bug #38 race condition** — background event processor now subscribes to the
  broadcast channel BEFORE the executor is spawned, eliminating the window where
  fast-completing executors (<1ms) finish before the subscription is active.
- **SQLite production readiness** — all 4 SQLite stores now configure WAL
  journal mode, `busy_timeout=5000ms`, `synchronous=NORMAL`, and
  `foreign_keys=ON` via `SqliteConnectOptions::pragma()`. Default pool size
  increased from 4 to 8.
- **Background event processor error handling** — in-memory task state is now
  reverted when `task_store.save()` fails, preventing phantom state divergence
  between memory and persistence.
- **Sync/streaming behavioral consistency** — invalid state transitions in
  streaming mode now mark the task as `Failed` (matching sync-mode behavior)
  instead of being silently ignored.
- **Unbounded artifact accumulation** — added `max_artifacts_per_task` (default:
  1000) to `HandlerLimits`, enforced in both sync and streaming paths.
- **Cancellation token race** — cancellation token is now inserted BEFORE
  `task_store.save()`, eliminating the window where `CancelTask` silently fails.
- **Connection pool idle timeout** — configured 90-second `pool_idle_timeout`
  on all hyper clients to prevent idle connection accumulation.

### Added (v0.3.0 Hardening)

- `HandlerLimits::max_artifacts_per_task` — configurable limit (default 1000)
  preventing O(n²) serialization cost and unbounded memory growth.
- `HandlerLimits::with_max_artifacts_per_task()` builder method.
- `CallContext::method()`, `caller_identity()`, `extensions()`, `request_id()`,
  `http_headers()` read-only accessor methods.

### Added (Framework Integration)

- **Axum framework integration** (`axum` feature) — `A2aRouter` builds an idiomatic
  `axum::Router` that wraps the existing `RequestHandler`. All 11 A2A v1.0 REST
  methods are mapped, including SSE streaming. The router is composable with other
  Axum routes, middleware, and layers. Zero business logic duplication — delegates
  entirely to the existing handler. Feature-gated behind `axum` in both
  `a2a-protocol-server` and `a2a-protocol-sdk`. 9 integration tests.

### Added (Testing)

- **TCK wire format conformance tests** — 44 tests in
  `crates/a2a-types/tests/tck_wire_format.rs` validating wire format compatibility
  against the A2A v1.0 specification. Covers ProtoJSON SCREAMING_SNAKE_CASE for
  `TaskState` and `MessageRole`, `SecurityRequirement`/`StringList` wrapper format,
  `Part` type discriminator, all `SecurityScheme` variants, cross-SDK interop
  fixtures (Python, JS, Go payloads), JSON-RPC 2.0 envelope, error codes, and
  full round-trip serialization of complex objects.
- **Mutation testing** — adopted `cargo-mutants` as a required quality gate with
  zero surviving mutants across all library crates. Configuration in `mutants.toml`.
- **Mutation testing CI** — on-demand via `workflow_dispatch` in
  `.github/workflows/mutants.yml`. Surviving mutants fail the build.
  Nightly schedule and PR-gate triggers are currently disabled to save CI time.
- **ADR 0006** — documents the rationale for mutation testing as a required quality
  gate, including alternatives considered and consequences.
- **60+ new tests** to kill surviving mutants across all crates, covering:
  state machine transitions, serde round-trips, builder patterns, hash functions,
  HTTP date formatting, rate limiter arithmetic, Debug impls, Arc delegation,
  OTel instrument recording, cancellation tokens, and more.
- **Wave 2 inline unit tests** — added `#[cfg(test)]` modules directly to 9 critical
  `a2a-protocol-server` source files covering the full request pipeline:
  `handler/messaging` (22 tests: ID validation, empty parts, metadata size limits,
  happy path, `return_immediately`), `handler/event_processing` (16 tests: state
  transitions, artifact updates, push delivery, `collect_events`),
  `handler/push_config` (8 tests: push CRUD), `handler/lifecycle` (21 tests:
  get/list/cancel/resubscribe/agent card), `handler/mod` (11 tests: builder
  accessors, Debug), `dispatch/rest` (40 tests: path parsing, response helpers,
  error mapping), `dispatch/jsonrpc` (13 tests: header extraction, param parsing,
  batch handling), `dispatch/grpc` (12 tests: config builders, encode/decode,
  error-to-status mapping), `dispatch/websocket` (5 tests: param parsing, error
  display). Total workspace test count: **~1,630 passing tests** (~1,850 with all feature flags).

### Added (PostgreSQL Support)

- **PostgreSQL-backed stores** (`postgres` feature) — `PostgresTaskStore` and
  `PostgresPushConfigStore` provide persistent store implementations using `sqlx`
  with the PostgreSQL driver. Multi-tenant variants `TenantAwarePostgresTaskStore`
  and `TenantAwarePostgresPushConfigStore` partition by `tenant_id` column.
  `PgMigration` and `PgMigrationRunner` provide forward-only schema versioning.
  Feature-gated behind `postgres` in both `a2a-protocol-server` and
  `a2a-protocol-sdk`.

### Added (Beyond-Spec Enhancements)

- **OpenTelemetry metrics integration** (`otel` feature) — `OtelMetrics` implements the
  `Metrics` trait with native OTLP export via `opentelemetry-otlp`. Instruments: request
  counts, response counts, error counts (with error type label), request latency (seconds),
  queue depth, and HTTP connection pool stats (active, idle, created, closed). Use
  `init_otlp_pipeline()` to bootstrap the global meter provider.
- **Connection pool metrics** — `ConnectionPoolStats` struct and
  `Metrics::on_connection_pool_stats()` callback for monitoring active/idle connections,
  total connections created, and connections closed.
- **Hot-reload agent cards** — `HotReloadAgentCardHandler` wraps agent cards behind
  `Arc<RwLock<_>>` for runtime updates without restarts. Three reload strategies:
  `reload_from_file()` (on-demand), `spawn_poll_watcher()` (periodic polling,
  cross-platform), `spawn_signal_watcher()` (Unix SIGHUP).
- **Store migration tooling** (`sqlite` feature) — `Migration` and `MigrationRunner`
  provide forward-only schema versioning for `SqliteTaskStore`. Built-in migrations:
  v1 (initial schema with indexes), v2 (add `created_at` column), v3 (composite index
  on `context_id, state`). Tracks applied versions in a `schema_versions` table.
- **Per-tenant configuration** — `PerTenantConfig` and `TenantLimits` allow operators
  to set per-tenant overrides for max concurrent tasks, executor timeout, event queue
  capacity, max stored tasks, and rate limits, with fallback to defaults.
- **`TenantResolver` trait** — abstracts tenant identity extraction from requests.
  Built-in implementations: `HeaderTenantResolver` (default `x-tenant-id`),
  `BearerTokenTenantResolver` (with optional token-to-tenant mapping),
  `PathSegmentTenantResolver` (URL path segment by index).
- **Agent card signing E2E test** — `test_agent_card_signing` in the agent-team suite
  generates an ES256 key pair, signs a card, verifies the signature, and tests tamper
  detection (`#[cfg(feature = "signing")]`).

### Fixed (Pass 10 — Exhaustive Audit)

- **Bug #40: Event queue serialization error silently swallowed** —
  `InMemoryQueueWriter::write()` used `unwrap_or(0)` when measuring serialized
  event size via `CountingWriter`. If `serde_json::to_writer` failed during size
  measurement, the error was silently masked and the event was sent through the
  channel without validation. Now propagates the serialization error as
  `A2aError::internal("event serialization failed: ...")`.
- **Bug #41: Capacity eviction fails when insufficient terminal tasks** —
  `InMemoryTaskStore` capacity eviction only removed terminal (completed/failed)
  tasks. When the store exceeded `max_capacity` with mostly non-terminal tasks,
  eviction could not remove enough entries, leaving the store permanently over
  capacity. Now falls back to evicting the oldest non-terminal tasks as a last
  resort to guarantee the hard capacity limit is enforced.
- **Bug #42: Lagged event count not exposed in reader warning** — The broadcast
  channel `Lagged(n)` error discarded the count of dropped events (`_n`). Now
  includes the actual count in the `trace_warn!` message for production
  observability (`"event queue reader lagged, {n} events skipped"`).

### Added (Pass 10)

- **1 new unit test** — `capacity_eviction_falls_back_to_non_terminal_when_needed`
  verifies that the fallback eviction path correctly removes non-terminal tasks
  when there are insufficient terminal tasks to bring the store under capacity.
- **94 passing E2E tests** with all features enabled (websocket, grpc, axum,
  sqlite, signing, otel). All tests exercised in a single dogfood run.
- **Total workspace tests: 1,750+** passing across all crates.

### Fixed (Pass 8 — Deep Dogfood)

- **Bug #32: Timeout errors misclassified as Transport (CRITICAL)** — REST and
  JSON-RPC transports mapped `tokio::time::timeout` errors to
  `ClientError::Transport` instead of `ClientError::Timeout`. Since `Transport`
  is non-retryable, timeouts never triggered retry logic. Fixed in both
  `rest.rs` and `jsonrpc.rs`.
- **Bug #33: SSE parser O(n) dequeue** — `SseParser::next_frame()` used
  `Vec::remove(0)` which shifts all remaining elements. Replaced internal
  `Vec<Result<SseFrame, SseParseError>>` with `VecDeque` for O(1) `pop_front`.
- **Bug #34: SSE parser silent UTF-8 data loss** — Malformed UTF-8 lines were
  silently discarded, causing data loss when multi-byte sequences split across
  TCP chunks. Now uses `String::from_utf8_lossy()` to preserve data with
  replacement characters instead of dropping entire lines.
- **Bug #35: Double-encoded path traversal bypass** — `contains_path_traversal()`
  only decoded one level of percent-encoding, allowing `%252E%252E` to bypass
  the check. Now applies two decoding passes to catch double-encoded sequences.
- **Bug #36: gRPC stream errors lose error context** — `grpc_stream_reader_task`
  mapped gRPC stream errors to generic `ClientError::Transport` instead of using
  `grpc_code_to_error_code()`, losing protocol error codes. Fixed to use
  `ClientError::Protocol(A2aError)` with proper code mapping.

### Added

- **6 new regression tests** — timeout retryability (Bug #32), SSE VecDeque
  dequeue correctness (Bug #33), SSE lossy UTF-8 (Bug #34), double/single/raw
  path traversal (Bug #35), exhaustive retryable classification.
- **3 new E2E tests (76-78)** — timeout retryable verification, concurrent
  cancel stress test (10 parallel), stale page token graceful handling.
- **Total E2E tests: 82** (98 with optional gRPC+WebSocket+Axum+SQLite+signing+OTel).

### Fixed (Pass 9 — Scale Probing)

- **Bug #37: SSE parser unbounded error queue** — `SseParser` internal frame
  queue could grow without bound from malicious streams. Added `max_queued_frames`
  limit (default 4096) with `with_max_queued_frames()` builder method.
- **Bug #39: Retry backoff float overflow** — `cap_backoff()` could panic on
  `f64::INFINITY` or `NaN` from extreme multiplier values. Now checks for
  non-finite results before `Duration` conversion.
- **Bug #38: Background event processor race** (documented) — In streaming mode,
  the background processor subscribes after the executor starts, so fast executors
  may complete before the subscription is active. Documented as known limitation.

### Added (Pass 9)

- **10 new deep dogfood E2E tests (81-90)** — state transition ordering, executor
  error propagation, streaming completeness, oversized metadata rejection, artifact
  content correctness, GetTask history, rapid sequential throughput, cancel terminal
  task, agent card semantic validation, GetTask-after-stream sync.
- **6 new Axum + SQLite E2E tests (93-98)** — Axum send/stream/card discovery,
  SQLite task store lifecycle, SQLite push config CRUD, combined Axum+SQLite stack.

### Fixed (Pass 7 — Deep Dogfood)

- **Graceful shutdown executor hang** — `shutdown()` and `shutdown_with_timeout()`
  now bound `executor.on_shutdown()` with a timeout to prevent indefinite hangs
  if an executor blocks during cleanup.
- **Push notification body clone per retry** — `body_bytes.clone()` inside the
  retry loop now uses `Bytes` (reference-counted) instead of `Vec<u8>`, reducing
  allocations from O(n × retries) to O(n) for push delivery.
- **Webhook URL missing scheme validation** — `validate_webhook_url()` now
  explicitly requires `http://` or `https://` schemes, rejecting `ftp://`,
  `file://`, and schemeless URLs that previously bypassed SSRF validation.
- **Push config unbounded global growth (DoS)** — `InMemoryPushConfigStore`
  now enforces a configurable global limit (default 100,000) in addition to the
  existing per-task limit. Prevents memory exhaustion from attackers creating
  millions of tasks with configs.
- **gRPC error code mapping incomplete** — added mappings for `Unauthenticated`,
  `PermissionDenied`, `ResourceExhausted`, `DeadlineExceeded`, `Cancelled`, and
  `Unavailable` tonic status codes to A2A error codes.
- **BuildMonitor cancel race** — `cancel()` now checks if the task is already
  cancelled before emitting a `Canceled` status, preventing invalid terminal
  state transitions.
- **CodeAnalyzer missing cancellation re-check** — added cancellation check
  between artifact emissions to allow faster abort during analysis.
- **Webhook/JSON-RPC/REST accept loop break on error** — accept loops in all
  agent-team server functions now `continue` on transient accept errors instead
  of terminating the entire server.
- **Coordinator silent client build failure** — now logs warnings with agent
  name and URL when client construction fails, aiding debugging.

### Added

- **`InMemoryPushConfigStore::with_max_total_configs()`** — configures the
  global push config limit (default 100,000) to prevent DoS via unbounded
  config creation.
- **8 new SSRF validation tests** — webhook URL scheme rejection (ftp, file,
  schemeless), CGNAT range, unspecified IPv4, IPv6 unique-local, IPv6 link-local.
- **4 new E2E tests (72-75)** — push config global limit enforcement, webhook
  URL scheme validation, combined status+context_id ListTasks filter, and
  metrics callback verification.
- **Total E2E tests: 68** (73 with optional gRPC+WebSocket transports).

### Fixed (Pass 6)

- **gRPC agent-team placeholder URL** — the gRPC `CodeAnalyzer` still used
  `"http://placeholder"` in its agent card (same Bug #12 pattern). Fixed by
  adding `GrpcDispatcher::serve_with_listener()` and using the pre-bind pattern.
- **REST transport query string encoding** — `build_query_string()` did not
  percent-encode parameter values. Values containing `&`, `=`, or spaces
  would corrupt query strings. Added RFC 3986 percent-encoding.
- **WebSocket stream termination detection** — replaced fragile
  `text.contains("stream_complete")` with proper JSON-RPC frame deserialization
  that checks for terminal task states. Prevents false positives from payloads
  containing the word "stream_complete".
- **Background event processor silent data loss** — 5 `let _ = task_store.save(...)`
  call sites in the streaming background processor silently dropped store errors.
  Now logs failures via `trace_error!`.
- **Metadata size validation bypass** — `unwrap_or(0)` allowed unserializable
  metadata to bypass size limits. Now rejects with `InvalidParams` error.
- **`InMemoryCredentialsStore` lock poisoning** — changed from silent `.ok()?`
  to `.expect()` (fail-fast). Poisoned locks now surface immediately instead of
  masking failures with silent `None` returns.
- **Rate limiter TOCTOU race on window advance** — replaced non-atomic
  load-check-store sequence with a `compare_exchange` (CAS) loop. Two concurrent
  threads can no longer both reset the counter to 1 on window boundary, which
  previously allowed 2N requests through per window.
- **Rate limiter unbounded bucket growth** — added amortized stale-bucket cleanup
  (every 256 `check()` calls). Buckets from departed callers are now evicted when
  their window is more than one window old.
- **Clippy `is_multiple_of` lint** — replaced manual `count % N == 0` with
  `count.is_multiple_of(N)` throughout.

### Added

- **`GrpcDispatcher::serve_with_listener()`** — accepts a pre-bound
  `TcpListener` for the gRPC server, enabling the same pre-bind pattern used
  by HTTP dispatchers. Ensures agent cards contain correct URLs.
- **`encode_query_value()`** — internal URL encoding for REST transport query
  string parameters (RFC 3986 §2.3 unreserved character set).
- **`is_stream_terminal()`** — WebSocket transport now uses structured JSON
  parsing for stream completion detection, with 6 new unit tests.
- **Protocol version compatibility warning** — `ClientBuilder::from_card()` now
  emits a `tracing::warn!` when the agent card's major protocol version differs
  from the client's supported version (currently `1.x`).
- **21 new unit tests** — comprehensive coverage for credentials store
  (multi-session, multi-scheme, overwrite, debug security), auth interceptor
  (basic/custom schemes), client builder validation (zero timeouts, unknown
  bindings, empty interfaces, config propagation), server builder validation
  (zero executor timeout, empty agent card interfaces, full option chain),
  rate limiter concurrency (200 concurrent requests), and stale-bucket cleanup.

### Changed

- **WebSocket transport** (`websocket` feature flag) — `WebSocketDispatcher` for
  server-side WebSocket support via `tokio-tungstenite`. JSON-RPC 2.0 messages
  are exchanged as WebSocket text frames. Streaming methods send multiple frames
  followed by a `stream_complete` response. Client-side `WebSocketTransport`
  provides persistent connection reuse.
- **Multi-tenancy** — `TenantAwareInMemoryTaskStore` and
  `TenantAwareInMemoryPushConfigStore` provide full tenant isolation using
  `tokio::task_local!` via `TenantContext::scope()`. Each tenant gets an
  independent store instance. SQLite variants (`TenantAwareSqliteTaskStore`,
  `TenantAwareSqlitePushConfigStore`) partition by `tenant_id` column.
- **TLS/mTLS integration tests** — 7 tests covering client certificate
  validation, SNI hostname verification, unknown CA rejection, and mutual TLS
  with valid/invalid/rogue client certificates. Uses `rcgen` for test-time
  certificate generation and `tokio-rustls` for TLS server.
- **Memory and load stress tests** — 5 tests for sustained concurrent load
  (200 concurrent requests, 500 requests over 10 waves), task store eviction
  under load, concurrent multi-tenant isolation (10 tenants × 50 tasks), and
  rapid connect/disconnect cycles.
- **Agent-team dogfood tests 51-55** — WebSocket send message, WebSocket
  streaming, tenant isolation, tenant ID independence, and tenant count tracking.
  Total agent-team E2E tests: 55 (66 with coverage gap tests, 69 with gRPC).
- `tls::build_https_client_with_config()` made public for custom TLS scenarios.
- `serve()` and `serve_with_addr()` server startup helpers — reduces the ~25 lines
  of hyper boilerplate per agent to a single function call. Both `JsonRpcDispatcher`
  and `RestDispatcher` implement the new `Dispatcher` trait.
- `RetryPolicy` and `ClientBuilder::with_retry_policy()` — configurable automatic
  retry with exponential backoff for transient client errors (connection errors,
  timeouts, HTTP 429/502/503/504). Ships as a transparent `RetryTransport` wrapper.
- `ClientError::is_retryable()` — classifies errors as transient or permanent.
- `EventEmitter` upstreamed to `executor_helpers` module — reduces event emission
  from 7-line struct literals to one-liners (`emit.status(TaskState::Working).await?`).
  Previously lived only in the agent-team example.
- `CallContext::request_id` — first-class request/trace ID field, automatically
  populated from the `X-Request-ID` HTTP header when present.
- `Metrics::on_latency(method, duration)` callback — the #1 production metric.
  All handler methods now measure and report request latency.
- Blanket `impl Metrics for Arc<T>` — eliminates the `MetricsForward` wrapper
  pattern when sharing metrics across handlers.
- `CallContext::http_headers` field — interceptors can now inspect
  `Authorization`, `X-Request-Id`, and other HTTP headers for auth decisions.
- `HandlerLimits::push_delivery_timeout` — configurable per-webhook timeout
  (default 5s) prevents one slow webhook from blocking all subsequent deliveries.
- Background event processor for streaming mode — push notifications and task
  store updates now fire for every event regardless of consumer mode.
- `SqliteTaskStore` and `SqlitePushConfigStore` — persistent store reference
  implementations behind the `sqlite` feature flag, using `sqlx` for async
  SQLite access. Includes schema auto-creation, cursor-based pagination,
  and upsert support.
- `boxed_future` helper function and `agent_executor!` macro in
  `executor_helpers` module — reduces `AgentExecutor` boilerplate from
  5 lines to 1 line per method.
- Doc examples for `TaskStore` and `AgentExecutor` traits — `# Example`
  sections in rustdoc for crates.io users.
- Explicit `sqlite` feature gate in CI — clippy and test steps for the
  `sqlite` feature flag alongside existing feature-specific gates.
- `JsonRpcDispatcher` now serves agent cards at `GET /.well-known/agent.json`,
  matching the existing `RestDispatcher` behavior.
- `EventQueueManager::subscribe()` creates additional readers for an active
  task's event stream, enabling `SubscribeToTask` (resubscribe) when another
  SSE stream is already active.
- Agent-team example refactored from monolithic 2800-line `main.rs` into
  best-practice modular structure (25 files) with 50 E2E
  tests across 5 categories (basic, lifecycle, edge cases, stress, dogfood).
- Client `send_message()` and `stream_message()` now merge client-level config
  (`return_immediately`, `history_length`, `accepted_output_modes`) into
  request parameters automatically. Per-request values take precedence.
- Dogfooding documentation restructured into modular book sub-pages: bugs
  found, test coverage matrix, and open issues roadmap.
- `EventEmitter` helper in agent-team example — caches `task_id` +
  `context_id` from `RequestContext`, reducing 9-line event struct literals
  to 1-line calls. Proof-of-concept for upstream `executor_helpers` addition.
- 10 new dogfood regression tests (tests 41-50) covering agent card URL
  correctness, push config listing via JSON-RPC, event classification,
  resubscribe, multiple artifacts, concurrent streams, context filtering,
  file parts, and history length.
- `RateLimitInterceptor` and `RateLimitConfig` — built-in fixed-window
  per-caller rate limiting as a `ServerInterceptor`. Caller keys are derived
  from `CallContext::caller_identity`, `X-Forwarded-For`, or `"anonymous"`.
- `TaskStore::count()` method — returns the total number of stored tasks.
  Useful for metrics and capacity monitoring. Has a default implementation
  returning `0` for backward compatibility. Implemented for both
  `InMemoryTaskStore` and `SqliteTaskStore`.

### Improved

- `InMemoryQueueWriter::write()` no longer allocates a full `String` to
  measure serialized event size. Uses a zero-allocation `CountingWriter`
  that counts bytes via `serde_json::to_writer()` instead of `to_string()`.
- `InMemoryTaskStore::save()` no longer holds the write lock during O(n)
  eviction sweeps. Eviction is decoupled from the insert and runs in a
  separate lock acquisition, reducing write lock contention under high
  concurrency.

### Changed

- **Breaking:** `PartContent` now uses `#[serde(tag = "type")]` with variant
  renames (`"text"`, `"file"`, `"data"`) per A2A spec. The old `Raw` and `Url`
  variants were merged into `File` with a new `FileContent` struct. Wire format
  now requires `{"type": "text", "text": "..."}` instead of `{"text": "..."}`.
  Backward-compatible `Part::raw()` and `Part::url()` constructors are provided.
- **Breaking:** `RequestHandler` stores changed from `Box<dyn TaskStore>` /
  `Box<dyn PushConfigStore>` / `Box<dyn PushSender>` to `Arc<dyn ...>` for
  cloneability into background tasks. `RequestHandlerBuilder` methods updated
  accordingly; `with_task_store_arc()` added for sharing store instances.
- **Breaking:** All `RequestHandler::on_*` methods now accept an additional
  `headers: Option<&HashMap<String, String>>` parameter for HTTP header
  forwarding to interceptors. Pass `None` if headers are not available.
- `handler.rs` (1,357 lines) split into 8 top-level modules under
  `handler/`: `mod.rs`, `limits.rs`, `helpers.rs`, `messaging.rs`,
  `lifecycle/` (5 sub-modules), `push_config.rs`, `event_processing/`
  (2 sub-modules), `shutdown.rs`. No public API changes.
- **Breaking:** `EventQueueManager` internals redesigned from `mpsc` to
  `tokio::sync::broadcast` channels. This enables multiple concurrent
  subscribers per task. Slow readers receive `Lagged` notifications instead
  of blocking the writer. The public `EventQueueWriter` / `EventQueueReader`
  traits are unchanged.

### Fixed

- `SubscribeToTask` (resubscribe) now works when another SSE reader is already
  active for the same task. Previously, `mpsc` channels allowed only a single
  reader, so resubscription returned "no active event queue for task".
- `ClientBuilder::with_return_immediately(true)` now actually propagates to
  the server. Previously, the flag was stored in `ClientConfig` but never
  injected into `MessageSendParams.configuration`, so the server always
  waited for task completion.
- JSON-RPC `ListTaskPushNotificationConfigs` now correctly parses
  `ListPushConfigsParams` instead of `TaskIdParams`. The field name mismatch
  (`id` vs `task_id`) caused silent deserialization failure — push config
  listing via JSON-RPC was completely broken while REST worked.
- Agent-team example agent cards now contain correct URLs via pre-bind
  listener pattern. Previously, cards were built with `"http://placeholder"`
  before the server bound to a port.
- Agent-team webhook event classifier now checks correct field names
  (`statusUpdate`/`artifactUpdate` instead of `status`/`artifact`).

## [0.2.0] - 2026-03-15

### Added

- Initial implementation of the A2A (Agent-to-Agent) v1.0.0 protocol specification.
- Core protocol type definitions and serialization.
- HTTP server with streaming (SSE) and JSON-RPC 2.0 dual transport.
- Client library for interacting with A2A-compatible agents.
- `SECURITY.md` with coordinated disclosure policy.
- `GOVERNANCE.md` with project governance and contribution guidelines.
- Health check endpoints (`GET /health`, `GET /ready`) for liveness/readiness probes.
- Request body size limits (4 MiB) to prevent DoS via oversized payloads.
- Content-Type validation on both JSON-RPC and REST dispatchers.
- Path traversal protection on the REST dispatcher.
- `TaskStoreConfig` with configurable TTL and capacity for `InMemoryTaskStore`.
- `RequestHandlerBuilder::with_task_store_config()` for store configuration.
- `ServerError::PayloadTooLarge` variant for body size limit violations.
- Executor timeout support via `RequestHandlerBuilder::with_executor_timeout()` to kill hung executors.
- Per-request HTTP timeout for `HttpPushSender` (default 30s) via `HttpPushSender::with_timeout()`.
- `TaskState::can_transition_to()` for handler-level state machine validation.
- Cursor-based pagination for `ListTasks` via `TaskStoreConfig`.
- URL percent-decoding for REST dispatcher path parameters.
- BOM (byte order mark) handling in JSON request bodies.
- Comprehensive hardening, dispatch, handler, push sender, and client test suites (1,769 tests).
- `#[non_exhaustive]` on 9 protocol types (7 enums, 2 structs) for forward-compatible evolution.
- SSRF protection for push notification webhook URLs (rejects private/loopback addresses).
- HTTP header injection prevention for push notification credentials.
- SSE parser memory limits (16 MiB default) to prevent OOM from malicious streams.
- Streaming task cancellation via `AbortHandle` on `Drop`.
- CORS support via `CorsConfig` for browser-based A2A clients.
- Graceful shutdown via `RequestHandler::shutdown()`.
- Path traversal protection against percent-encoded bypass (`%2E%2E`).
- Query string length limits (4 KiB) for DoS protection.
- Cancellation token map size bounds with automatic stale token cleanup.
- Amortized task store eviction (every 64 writes instead of every write).
- `ClientError::Timeout` variant for distinct timeout errors.
- Separate `stream_connect_timeout` configuration for SSE connections.
- Server benchmarks for task store and event queue operations.
- Carg