# Changelog

All notable changes to a2a-rust are documented in the project's [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

## Versioning

a2a-rust follows [Semantic Versioning](https://semver.org/):

- **Major** (1.0.0) — Breaking API changes
- **Minor** (0.2.0) — New features, backward compatible
- **Patch** (0.2.1) — Bug fixes, backward compatible

All four workspace crates share the same version number and are released together.

## Release Process

Releases are triggered by pushing a version tag:

```bash
git tag v0.3.0
git push origin v0.3.0
```

The [release workflow](https://github.com/tomtom215/a2a-rust/blob/main/.github/workflows/release.yml) automatically:

1. Validates all crate versions match the tag
2. Runs the full CI suite
3. Publishes crates to crates.io in dependency order
4. Creates a GitHub release with notes

### Publish Order

```
a2a-protocol-types → a2a-protocol-client + a2a-protocol-server → a2a-protocol-sdk
```

This ensures each crate's dependencies are available before it publishes.

## v0.3.4 (2026-03-31)

### Bug Fixes

- **SendMessage rejects terminal tasks** — Messages to Completed/Failed/Canceled/Rejected tasks now return `UnsupportedOperation` per spec CORE-SEND-002
- **SendMessage validates unknown taskId** — Client-provided `taskId` that doesn't reference an existing task now returns `TaskNotFound` per spec section 3.4.2
- **`historyLength` parameter applied** — `GetTask` and `ListTasks` now truncate message history to the requested length; `historyLength=0` returns no history
- **SubscribeToTask terminal task error** — Subscribing to a terminal task now returns `UnsupportedOperation` instead of a generic internal error

### Added

- **`Artifact::validate()` method** — Validates non-empty `parts` per A2A spec
- **`Part::text_content()` accessor** — Extracts text from a text part
- **`ServerError::UnsupportedOperation` variant** — Maps to `ErrorCode::UnsupportedOperation` (-32004)
- **SubscribeToTask emits Task snapshot as first event** — Prevents clients from missing state on reconnection
- **`ClientBuilder::from_card()` preserves tenant** — Tenant from `AgentInterface` is preserved in `ClientConfig::tenant`
- **`ClientBuilder::with_tenant()` method** — Explicit tenant configuration for multi-tenancy
- **`ClientConfig::tenant` field** — Default tenant for all requests
- **`TaskListResponse` required fields** — `next_page_token`, `page_size`, `total_size` always present per proto spec
- **`SendStreamingMessage` first event** — Task snapshot emitted as first SSE event (like SubscribeToTask)
- **`GetExtendedAgentCard` capability check** — Returns correct errors per spec section 3.1.11

## v0.3.3 (2026-03-30)

### Bug Fixes

- **`find_task_by_context` prefers non-terminal tasks** — Stale terminal tasks no longer shadow active tasks for the same `context_id`
- **`context_locks` memory leak** — Stale per-context mutexes are now pruned when the map exceeds `max_context_locks`
- **`PayloadTooLarge` error code** — Returns `InvalidRequest` (-32600) instead of `InternalError` (-32603)
- **Params-level `context_id` validation** — Now validated via `validate_id()` like message-level `context_id`
- **`eviction_interval=0` panic** — No longer panics; treated as "disable periodic eviction"
- **Push config deterministic ordering** — `list()` results sorted by `(task_id, config_id)`
- **Cancel task TOCTOU race narrowed** — Re-reads task before saving `Canceled` to avoid overwriting concurrent completion
- **`page_size` clamped at handler** — Prevents oversized allocations from untrusted input
- **Tenant store read-path DoS** — Read operations no longer allocate tenant partitions
- **`from_pool()` schema parity** — Now matches `with_migrations()` schema (adds `created_at`, composite index)
- **JSON-RPC serialization error handling** — Returns proper errors instead of `null` results; uses HTTP 200 per spec
- **`MessageRole` wire format** — Serializes as lowercase `"user"`/`"agent"` per A2A spec
- **Unused example deps removed** — `rig-core`, `bytes`

## v0.3.2 (2026-03-30)

### Bug Fixes

- **Task ID not reused for non-terminal continuations** — `on_send_message` now reuses the client-provided `task_id` when it matches a stored non-terminal task (#66)

## v0.3.0 (2026-03-19)

### Performance

- **`TCP_NODELAY` on all sockets** — Eliminates ~40ms Nagle/delayed-ACK latency on SSE streaming and JSON-RPC responses
- **`InMemoryTaskStore` BTreeMap migration** — List queries now O(page_size) instead of O(n): 17–164× faster at 1K–100K tasks
- **Batch clone removal** — JSON-RPC batch dispatch no longer clones each request item
- **`memory_overhead` benchmark fix** — CI no longer crashes on zero-variance allocation counts
- **Benchmark server `TCP_NODELAY`** — Streaming benchmarks now report actual SDK latency (~1.5ms) instead of Nagle-inflated ~44ms

- **Axum framework integration** (`axum` feature) — `A2aRouter` for idiomatic
  Axum servers. All 11 REST methods, composable with other Axum routes/middleware.
- **TCK wire format conformance tests** — 44 tests validating wire format
  compatibility against the A2A v1.0 specification.
- **Mutation testing** — zero surviving mutants across all library crates.
- **Comprehensive inline unit tests** — 1,769 tests across all crates covering
  the full request pipeline, dispatchers, push delivery, streaming, and more.

### Beyond-Spec Enhancements

- **OpenTelemetry metrics** (`otel` feature) — `OtelMetrics` with native OTLP export
- **Connection pool metrics** — `ConnectionPoolStats` and `on_connection_pool_stats` callback
- **Hot-reload agent cards** — `HotReloadAgentCardHandler` with file polling and SIGHUP
- **Store migration tooling** (`sqlite` feature) — `MigrationRunner` with V1–V3 built-in migrations
- **Per-tenant configuration** — `PerTenantConfig` and `TenantLimits` for differentiated service levels
- **`TenantResolver` trait** — `HeaderTenantResolver`, `BearerTokenTenantResolver`, `PathSegmentTenantResolver`
- **Agent card signing E2E** — test 79 in agent-team suite (`signing` feature)

### Bug Fixes (Passes 7–10)

- Event queue serialization error swallowing fixed with proper error propagation
- Capacity eviction now falls back to non-terminal tasks when terminal tasks are insufficient
- Lagged event count now exposed in reader warnings for observability
- Timeout errors now correctly classified as retryable (`ClientError::Timeout`)
- SSE parser O(n) dequeue replaced with `VecDeque` for O(1) `pop_front`
- Double-encoded path traversal bypass fixed with two-pass percent-decoding
- gRPC stream errors now preserve protocol error codes
- Rate limiter TOCTOU race fixed with CAS loop
- Push config store now enforces global limits (DoS prevention)

### Concurrency, Security & Robustness Fixes (Session 2026-03-19, Pass 15)

**Streaming reliability:**
- **H5: Separate persistence channel** — The background event processor now uses a dedicated mpsc persistence channel instead of subscribing to the broadcast channel after executor start. This eliminates the race where fast executors could emit events before the subscription was active, causing the task store to miss updates.

**Transport concurrency fixes:**
- **C1: gRPC Mutex removed** — gRPC transport no longer serializes concurrent requests through a Mutex. The tonic `Channel` (internally multiplexed, cheap to clone) is now cloned per request, enabling full concurrent throughput.
- **C2: WebSocket transport redesigned** — Replaced reader Mutex with a dedicated background reader task and `HashMap<RequestId, PendingRequest>` message routing, eliminating the deadlock where holding the reader lock blocked all other requests.
- **C3: WebSocket auth headers** — Extra headers (including auth interceptor headers) are now applied to the WebSocket upgrade HTTP request via the tungstenite `IntoClientRequest` trait.

**Security hardening:**
- **H6: SSRF DNS rebinding prevention** — Added `validate_webhook_url_with_dns()` that resolves DNS before IP validation, preventing DNS rebinding attacks where a hostname resolves to a public IP during validation but a private IP during the actual request.
- **H8: Agent card body size limit** — Added a 2 MiB body size limit on agent card fetch responses to prevent OOM from malicious or misconfigured card endpoints.

**Performance:**
- **H7: Retry transport optimization** — Params are now serialized to bytes once before the retry loop, then deserialized for each attempt, avoiding deep-clone of the `serde_json::Value` tree on every retry.
- **L4: Shutdown polling improvement** — Reduced shutdown polling interval from 50ms to 10ms with deadline-aware sleep for faster graceful shutdown.

**Error handling & validation:**
- **M3: `find_task_by_context` error propagation** — Changed from silently swallowing errors to propagating them via `?`.
- **M13: TaskId/ContextId TryFrom** — Added `TryFrom<String>` and `TryFrom<&str>` impls that reject empty and whitespace-only strings.
- **M14: CachingCardResolver error handling** — `new()` and `with_path()` now return `ClientResult<Self>` instead of silently producing empty URLs on invalid input.
- **M16: REST tenant passthrough** — Fixed REST dispatch handlers to pass the extracted tenant from the URL path through to all handler methods.
- **M17: FileContent validation** — Added `validate()` method that checks at least one of `bytes`/`uri` is set.
- **M19: Push URL validation** — Added `validate()` method on `TaskPushNotificationConfig` for URL format validation.
- **L14: Timestamp validation** — Added `has_valid_timestamp()` method to `TaskStatus` for RFC 3339 timestamp validation.

**Resource limits:**
- **M9: WebSocket concurrency limit** — Added per-connection `Semaphore(64)` to limit concurrent spawned tasks per WebSocket connection, preventing resource exhaustion from a single client.
- **M10: WebSocket message size limit** — Added 4 MiB message size check for incoming WebSocket frames.
- **M8: JSON-RPC batch size limit** — Confirmed existing `max_batch_size` in `DispatchConfig`.

**Database safety:**
- **L7: SQLite parameterized queries** — Changed `LIMIT` from `format!` string interpolation to parameterized queries, preventing potential SQL injection.

### v0.2.0 (2026-03-15)

Initial implementation of A2A v1.0.0 with all 11 protocol methods, dual transport (JSON-RPC + REST), SSE streaming, push notifications, agent card discovery, HTTP caching, enterprise hardening, and 600+ tests.

For the complete version history, see [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).
