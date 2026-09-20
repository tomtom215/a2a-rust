# Changelog

All notable changes to a2a-rust are documented in the project's [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

## Versioning

a2a-rust follows [Semantic Versioning](https://semver.org/):

- **Major** (1.0.0) — Breaking API changes
- **Minor** (0.2.0) — New features; before `1.0` a minor may also carry
  breaking changes, batched and labelled as [STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
  requires — see [Upgrading Between Minor Versions](./upgrading.md)
- **Patch** (0.2.1) — Bug fixes, backward compatible

All four workspace crates share the same version number and are released together.

## Release Process

Releases are triggered by pushing a version tag:

```bash
git tag -a v0.3.0 -m "Release v0.3.0"
git push origin v0.3.0
```

The tag must be **annotated** (`-a`). A lightweight tag records no tagger and
no date, so the tag alone attests nothing about who cut the release; the
release workflow runs `git cat-file -t` and stops if it does not print `tag`.
Creating a release through the GitHub web UI produces a lightweight tag.

The [release workflow](https://github.com/tomtom215/a2a-rust/blob/main/.github/workflows/release.yml) automatically:

1. Validates the tag against crate versions, CHANGELOG.md, CITATION.cff, and SECURITY.md
2. Runs the full CI suite and security audit
3. Packages all crates with SLSA build provenance
4. Creates a GitHub release with notes extracted from CHANGELOG.md
5. Publishes crates to crates.io in dependency order (behind a manually
   approved `crates-io` environment)

### Publish Order

```text
a2a-protocol-types → a2a-protocol-server → a2a-protocol-client → a2a-protocol-sdk
```

Topological order over **all** dependency edges: server precedes client
because the client has a versioned dev-dependency on the server, which
`cargo publish` resolves against the crates.io index.

## v0.13.0 (2026-09-20)

A minor release that makes the event log the record, and spends it on stream
resumption. Carries three deliberate breaking changes, batched and labelled as
[STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
requires. Full detail in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

- **An append-only event log, beside the snapshot.** A task's state is a fold,
  and until now the fold was the only thing kept — so a wrong fold had nothing
  to be checked against, which is how
  [#130](https://github.com/tomtom215/a2a-rust/issues/130) went unobserved.
  `TaskStore` gains `supports_event_log`, `append_event`, `last_event_seq` and
  `read_events`, implemented by every store this crate ships. The snapshot
  stays authoritative for reads; this is not event sourcing.
- **SSE frames carry an `id:`, and `Last-Event-ID` resumes from it.** A client
  that was disconnected sends back the position of the last frame it saw and
  receives exactly what it missed, rather than a snapshot it has to diff. The
  position is assigned once, where the event fans out to both the log and the
  stream, so the `id:` on the wire and the `seq` in the store are the same
  number by construction.
- **An executor can see who called it.** `RequestContext` gains a
  `call_context`, carrying caller identity, tenant, activated extensions and
  inbound headers across the `tokio::spawn` that task-locals do not survive.
- **Idempotency keys on `message/send`, as an SDK extension.** A send whose
  connection drops is ambiguous — the task may exist or it may not — and
  `message/send` creates server-side state, so a client is right to refuse the
  retry. A key the server deduplicates on makes that retry safe: the same
  message replays to the first task in whatever state it reached, and a
  *different* message on the same key is refused rather than silently answered.
  Keys are tenant-scoped, every bundled store honours them, and the card
  advertises support only when the store actually has it. This is **not part of
  A2A v1.0** — it ships as `https://a2a-rust.com/extensions/idempotency/v1`, so
  a server that enables it stays conformant. See
  [Idempotent Sends](../client/idempotency.md).
- **W3C trace context crosses an A2A hop**, so a delegation chain is one trace
  rather than several unrelated span trees.
- **A failed task says why**, as a `FailureClass` a caller can `match` on
  instead of parsing English out of a message.
- **A conformance harness for `AgentExecutor`**, behind the `conformance`
  feature: thirteen checks an implementation can run against itself.
- **Breaking:** eight items. `RequestContext`, `IdempotencyClaim` and
  `KeyError` are `#[non_exhaustive]`; `EventQueueReader::read` yields a
  `StreamEvent` carrying the log position; `FailureClass::ALL` is a slice
  rather than a fixed-size array; `PurgeReport::journal_orphans_deleted` is
  now `orphan_rows_deleted`; `build()` refuses a signed agent card it would
  otherwise have to edit; a keyed `message/send` is retried only against a
  peer that advertises the extension; and `message.id` is validated at
  ingress. See
  [Upgrading Between Minor Versions](./upgrading.md), which has a migration
  for each.

## v0.12.1 (2026-09-17)

A patch release: one specification correction, one security advisory, and
documentation fixes. No API changes. Full detail in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

- **A new task on an existing context no longer inherits the previous task's
  artifacts, history and metadata**
  ([#130](https://github.com/tomtom215/a2a-rust/issues/130)). When a client
  sent a second message on a context without a `taskId`, the server correctly
  minted a new task id and then built that task from the previous task's
  state, so every round returned the whole context's accumulated artifacts,
  growing by one each turn. `a2a.proto` scopes all three fields to the task.
  State now carries forward only when the ids match — the `input-required`
  continuation (spec §3.4.3) the carry-forward was written for in v0.6.0,
  where wiping accumulated state would be the bug. Affects 0.11.0 and 0.12.0.
- **`rustls` advanced to 0.23.45**
  ([RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285)):
  rustls accepted TLS 1.3 handshake messages sent at the wrong encryption
  level when they followed a key-changing message in the same record,
  contrary to RFC 8446 §5.1. The handshake transcript stays authenticated, so
  this is a conformance failure rather than a forgery or confidentiality
  break. The SLIMRPC binding cannot take the upgrade — an upstream
  `aws-lc-rs` pin forecloses it — and carries a documented, dated waiver
  instead; the four published crates use the `ring` backend and are on
  0.23.45.
- **Documentation corrections**: three wrong claims about the `rig-agent`
  example, a TCK figure carried in seven places that measurement
  contradicted, and six table rows rendering short of their declared columns.

## v0.12.0 (2026-09-10)

The deliberate breaking minor before the two clean minors
[STABILITY.md](https://github.com/tomtom215/a2a-rust/blob/main/STABILITY.md)
requires; the migration for every item is in
[Upgrading Between Minor Versions](./upgrading.md). Highlights — see
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
for the full list:

- **Fifteen public configuration structs are `#[non_exhaustive]`** with a
  `with_*` setter for every field, so a field can be added later without a
  break; `HandlerLimits` gains `push_delivery_budget` and
  `executor_drain_timeout`, the client's `WebSocketTransportConfig` gains
  `max_pending_requests`.
- **Bounds that were missing** — a drain that never closes, the push
  delivery budget, DNS lookups in the push sender, in-flight WebSocket
  requests, served gRPC connections (`with_max_connections`), and the
  SLIMRPC binding's slow-consumer stall.
- **Every dependency refreshed**, and the weekly mutation sweep's survivors
  addressed: the three proved equivalent carry `#[mutants::skip]`.
- **An executor's error text reaches a blocking caller** on the Failed
  task's status message; **the blocking path reports every push outcome**;
  **one deadline per token request and per stream start** in the client.
- **Adoption**: `tools/a2a-cli` (unpublished), a Rust worker in
  `multi-lang-team`, `examples/resilient-agent`, the upgrade guide, and
  `AgentCard::with_streaming`, `CardFetchOptions`,
  `ClientBuilder::chosen_interface`.
- **Gates**: reachability, timeout nesting, inert knobs, the reverse
  API-reference check, the weekly codecov-ignore check; DCO on every push
  to `main`; cargo-semver-checks over all four crates with all features;
  the toolchain action pinned by SHA.

## v0.8.0 to v0.11.0

Not summarised here; each release's section in
[CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
is the record, and the breaking items are in
[Upgrading Between Minor Versions](./upgrading.md). (This page carried a
"v0.7.0 upcoming" section until 2026-09-10, four releases after 0.7.0
shipped — the decay the sitemap and API-reference gates exist to catch, and
which nothing checks for this page.)

## v0.6.0 (2026-06-10)

### Fixed

- **SSE disconnects no longer fail running tasks** — clients reattach via
  `SubscribeToTask`; the persistence channel already had every event.
- **`Working → Working` is a valid transition** — repeated Working status
  updates (progress narration) no longer mark the task Failed in the store
  while the stream shows success.
- **`Task.history` is now populated** — user messages at send time, agent
  `Message` events from both processors, capped at 1,024 (oldest dropped);
  `historyLength` truncation is now observable.
- **Continuations preserve accumulated state** — follow-up messages no
  longer wipe a task's artifacts, metadata, and history.
- **Client surfaces streaming errors** — JSON-RPC error envelopes on
  `message/stream` map to `ClientError::Protocol` instead of an empty stream.
- **Legacy `tasks/resubscribe` alias** accepted alongside `SubscribeToTask`
  and `tasks/subscribe`.

### Added

- **`incident-response` example** — three-agent team demonstrating
  multi-turn `INPUT_REQUIRED`, delegation, streaming progress, artifacts,
  and cooperative cancellation; runs fully local.
- **TCK test 20: `a2a_media_type_accepted`** — servers must accept the
  registered `application/a2a+json` media type that real clients send.
- **PostgreSQL integration suite** — live-database tests for all five
  Postgres store files, run in CI against a `postgres:16` service.

### Security

- **`rustls-webpki` upgraded to 0.103.12** — Fixes
  [RUSTSEC-2026-0098](https://rustsec.org/advisories/RUSTSEC-2026-0098.html)
  ([GHSA-965h-392x-2mh5](https://github.com/rustls/webpki/security/advisories/GHSA-965h-392x-2mh5)):
  URI name constraints were incorrectly accepted during X.509 path validation.
  Reaches `a2a-protocol-client` transitively via `rustls` when the
  `tls-rustls` feature is enabled. No API changes — drop-in update.

## v0.5.0 (2026-04-02)

### Breaking Changes

- **`TaskStore::save()` and `insert_if_absent()` now take `&Task` instead of
  owned `Task`** — Eliminates forced clones at every save call site. Custom
  `TaskStore` implementations must update their method signatures. See
  [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md)
  for migration guide.
- **Version bump: 0.4.1 → 0.5.0** across all four crates.

### Performance

- **Broadcast channel capacity 64 → 256** — Pushes per-event cost inflection from ~52 to ~252 events.
- **`serde_helpers` module** — `SerBuffer` (thread-local buffer reuse, 2.3× less small-payload overhead) and `deser_from_str`/`deser_from_slice` (borrowed deserialization, ~15-25% fewer allocs).
- **SSE frame building: thread-local reusable buffer** — Amortized 0 allocations per event vs previous 1.
- **`InMemoryTaskStore::list()` — O(n log n) → O(log n + page_size)** — Added `BTreeSet<TaskId>` sorted index and `HashMap<String, BTreeSet<TaskId>>` context index. Eliminates the per-call sort that caused 20-70× regressions at 10K+ tasks.
- **`InMemoryTaskStore::insert()` — Update fast path** — Skips BTreeSet and context index operations when updating an existing task with unchanged context_id. Reduces save() from ~2.5µs to ~700ns for the common update case.
- **SSE per-event serialization — 2 allocations → 1** — `build_sse_message_frame()` serializes JSON directly into the SSE frame buffer via `serde_json::to_writer`, skipping the intermediate `serde_json::to_string()` allocation.
- **`Part` deserialization — ~80 fewer allocations per Task** — Replaced `#[serde(flatten)]` with a hand-rolled `Deserialize` implementation that reads all fields in a single pass without intermediate `serde_json::Value` buffering.

### Benchmarks

- **Transport payload scaling extended to 1MB** — 100KB and 1MB payloads in `transport_throughput.rs`.
- **New: `protocol/payload_scaling` isolation benchmarks** — Pure serde cost from 64B to 1MB; `to_vec` vs `SerBuffer`, `from_slice` vs `from_str`.
- **Cache-busting for `data_volume/get` at 100K** — 4MB allocation to flush CPU caches between populate and measure.
- **Documentation comments** — Connection reuse, cold start vs steady state, concurrent store anomaly.
- **New: `advanced_scenarios` suite** — Tenant resolver overhead (header, bearer, path), agent card hot-reload and discovery endpoint, subscribe fan-out (1-10 concurrent subscribers), streaming artifact accumulation cost (task.clone() at 0-500 depth), pagination full walk (100-1K tasks), extended agent card round-trip.
- **New: `production_scenarios` suite** — SubscribeToTask reconnection, cold start vs steady-state, concurrent cancel+subscribe race, 7-step E2E orchestration, push config CRUD round-trip, parallel agent burst (10-100 agents), dispatch routing isolation.
- **Fixed: `MultiEventExecutor`** — Was emitting invalid `Working → Working` state transitions; now emits `Working` once, then N artifacts, then `Completed`.
- **Fixed: `InMemoryTaskStore::insert()`** — Optimized update path skips redundant BTreeSet/context index operations, reducing save() variance from [1.5µs, 4.2µs] to ~700ns.
- **Fixed: Criterion measurement_time warnings** — Added measurement_time to 23+ groups across 8 files.
- **Improved: `data_volume` get benchmark** — Uses 64 pseudo-random keys instead of single midpoint to avoid HashMap bucket anomalies.
- **Improved: `backpressure` stream volume** — Added 252 and 502 event counts to push per-event signal above CI noise floor; added timer calibration benchmarks.

## v0.4.1 (2026-03-31)

### Bug Fixes

- **REST streaming deserialization failure** — `EventStream` now correctly parses bare `StreamResponse` JSON for REST SSE streams instead of expecting a JSON-RPC envelope wrapper. Fixes `"data did not match any variant of untagged enum JsonRpcResponse"` errors when streaming via the REST binding.

## v0.4.0 (2026-03-31)

### Breaking Changes

- **Part wire format → v1.0 flat oneof** — `{"type":"text","text":"..."}` → `{"text":"..."}`. File parts: `{"raw":"base64","filename":"f.png","mediaType":"image/png"}` or `{"url":"https://..."}`. `PartContent` enum variants: `Text`, `Raw`, `Url`, `Data`.
- **Enum serialization → SCREAMING_SNAKE_CASE** — `TaskState`: `"completed"` → `"TASK_STATE_COMPLETED"`. `MessageRole`: `"user"` → `"ROLE_USER"`. Legacy lowercase accepted on deserialization.
- **`SendMessageResponse` → externally tagged** — `{"task":{...}}` or `{"message":{...}}` (was untagged).
- **Agent Card path** — `/.well-known/agent-card.json` (previously `/.well-known/agent.json`).
- **`OAuthFlows` → enum (oneof)** — One flow per scheme, matching proto.
- **Error responses → AIP-193 format** — `{"error":{"code":N,"status":"...","message":"...","details":[...]}}` with `google.rpc.ErrorInfo`.

### Bug Fixes

- HTTP/gRPC error status code mappings corrected for all 9 A2A error types
- Blocking SendMessage returns on interrupted states (`INPUT_REQUIRED`/`AUTH_REQUIRED`)
- `ListTasks` `includeArtifacts` parameter now applied (default false omits artifacts)

### Added

- `ErrorCode::a2a_reason()`, `http_status()`, `grpc_status()`, `error_info_data()`
- `TaskState::is_interrupted()`
- Error constructors: `push_not_supported()`, `content_type_not_supported()`, `extension_support_required()`, `version_not_supported()`

## v0.3.4 (unpublished)

A standalone 0.3.4 release was never tagged or published; these changes
first shipped as part of v0.4.0 (2026-03-31).

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
