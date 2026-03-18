<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
  display). Total workspace test count: **1,750+ passing tests**.

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
  best-practice modular structure (23 files) with 50 E2E
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
- Comprehensive hardening, dispatch, handler, push sender, and client test suites (1,750+ tests).
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
- Cargo-fuzz target for JSON deserialization of all major protocol types.
- `docs/implementation/plan.md` documenting planned beyond-spec extensions (request IDs,
  metrics, rate limiting, WebSocket, multi-tenancy, persistent store).
- Pitfalls catalog (`book/src/reference/pitfalls.md`) with entries for serde,
  hyper, SSE, push notifications, async/tokio, workspace, and testing gotchas.

### Changed

- Eliminated unnecessary `serde_json::Value` clones in 8 client methods by
  moving the value into `ClientResponse` and extracting it after interceptors run.

- **Breaking:** `AgentExecutor` trait is now object-safe — methods return
  `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>` instead of
  `impl Future`. This eliminates the generic parameter `E: AgentExecutor` from
  `RequestHandler`, `RequestHandlerBuilder`, `JsonRpcDispatcher`, and
  `RestDispatcher`, enabling dynamic dispatch via `Arc<dyn AgentExecutor>`.
- `InMemoryTaskStore` now performs TTL-based eviction of terminal tasks (default
  1 hour) and enforces a maximum capacity (default 10,000 tasks).

### Fixed

- Invalid state transitions (e.g. Submitted → Completed) are now rejected with `InvalidStateTransition` error.
- Push notification delivery now properly times out instead of hanging indefinitely.

### Removed

- (Nothing removed — this is the initial release.)
