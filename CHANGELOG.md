<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Performance

- **SSE streaming bimodal distribution mitigation** — Added
  `tokio::task::yield_now()` before the SSE read loop (server-side) and body
  reader task (client-side JSON-RPC and REST) to align first polls with fresh
  executor slots, reducing timer wheel collisions. Set
  `MissedTickBehavior::Skip` on the keep-alive interval. Added HTTP connection
  warmup to transport streaming benchmarks. These changes reduce the bimodal
  pattern in isolated paths (lifecycle/e2e: 24%→1% outliers) while the full
  transport pipeline retains the pattern as a documented measurement artifact.

### Fixed

- **Benchmark server `AddrInUse` on CI** — Benchmark servers now set
  `SO_REUSEADDR` + `SO_REUSEPORT` via `socket2` and use a graceful shutdown
  handle (`watch::Sender<bool>`) so that rapid server cycling during cold-start
  benchmarks does not fail with `Address already in use` on CI runners where
  `TIME_WAIT` recycling is slower.
- **Criterion timeout warnings eliminated** — Increased `measurement_time` for
  3 remaining benchmark groups: `lifecycle/e2e` (8s→20s),
  `concurrent/sends` (10s→18s), `backpressure/slow_consumer` (15s→20s with
  10 samples). All 140 benchmarks now complete within their budget.
- **Push config benchmark per-task limit** — `production/push_config/set_roundtrip`
  and `delete_roundtrip` now upsert a pre-created config instead of creating new
  configs each iteration, preventing `push config limit exceeded` panics during
  criterion warmup.

### Changed

- **Benchmark documentation** — Added "Known Measurement Limitations" section
  to `benches/README.md` and the auto-generated GH Book benchmarks page
  documenting streaming bimodal distribution, get()/100K cache anomaly, stream
  volume per-event cost inflection, and slow consumer timer calibration.
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

## [0.3.4] - 2026-03-31

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

The following issues were identified during deep analysis and are documented for
future hardening. They do not affect correctness for typical usage but may
manifest at extreme scale or under adversarial conditions:

- **Broadcast channel lag** — If an SSE consumer falls behind, the broadcast
  channel drops events for both the SSE reader and the background event
  processor. State transitions missed by the background processor are not
  persisted to the task store. Mitigation: size the event queue capacity
  (`with_event_queue_capacity`) for your workload.
- **SSRF DNS rebinding** — `HttpPushSender` validates webhook URLs against
  private IP patterns but does not resolve DNS. A public domain resolving to a
  private IP bypasses the check. Mitigation: deploy a network-level egress
  filter.
- **WebSocket message size** — The WebSocket dispatcher does not enforce a
  message size limit (unlike JSON-RPC/REST which cap at 4 MiB). Mitigation:
  use a reverse proxy with WebSocket frame size limits.
- **SQL push config stores** — Unlike `InMemoryPushConfigStore`, the SQLite and
  Postgres push config stores do not enforce per-task or global config limits.
  Mitigation: implement application-level limits or periodic cleanup.

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
