# Dogfooding: Bugs Found & Fixed

Ten dogfooding passes across `v0.1.0` and `v0.2.0` uncovered **43 bugs** that 1,750+ unit tests, integration tests, property tests, and fuzz tests did not catch. 42 have been fixed; 1 is documented as a known limitation.

## Summary

| Pass | Focus | Bugs | Severity |
|------|-------|------|----------|
| [Pass 1](#pass-1-initial-dogfood-3-bugs) | Initial dogfood | 3 | 2 Medium, 1 Low |
| [Pass 2](#pass-2-hardening-audit-6-bugs) | Hardening audit | 6 | 1 High, 2 Medium, 3 Low |
| [Pass 3](#pass-3-stress-testing-1-bug) | Stress testing | 1 | 1 High |
| [Pass 4](#pass-4-sdk-regression-testing-3-bugs) | SDK regressions | 3 | 2 Critical, 1 Medium |
| [Pass 5](#pass-5-hardening--concurrency-audit-4-bugs) | Concurrency | 4 | 2 High, 1 Medium, 1 Low |
| [Pass 6](#pass-6-architecture-audit-5-bugs) | Architecture | 5 | 1 Critical, 1 High, 3 Medium |
| [Pass 7](#pass-7-deep-dogfood-9-bugs) | Deep dogfood | 9 | 1 Critical, 2 High, 4 Medium, 2 Low |
| [Pass 8](#pass-8-deep-dogfood-5-bugs) | Performance & security | 5 | 1 Critical, 3 Medium, 1 Low |
| [Pass 9](#pass-9-scale-probing-4-bugs) | Scale probing | 4 | 1 High, 2 Medium, 1 Low |
| [Pass 10](#pass-10-exhaustive-audit-3-bugs) | Exhaustive audit | 3 | 1 High, 1 Medium, 1 Low |

### By Severity

| Severity | Count | Examples |
|----------|-------|---------|
| **Critical** | 5 | Timeout retry broken (#32), push config DoS (#26), placeholder URLs (#11, #12, #18) |
| **High** | 9 | Concurrent SSE (#9), return_immediately ignored (#10), TOCTOU race (#15), SSRF bypass (#25), credential poisoning (#14), query encoding (#19), gRPC stream errors (#23), event ordering (#21), serialization error swallowed (#41) |
| **Medium** | 19 | REST field stripping (#1), path traversal (#35), stale pagination (#30), capacity eviction fails (#42), test coverage gaps (#40) |
| **Low** | 10 | Metrics hooks (#2, #6, #7), gRPC error context (#36), lagged event count hidden (#43) |

### Configuration Hardening

Extracted all hardcoded constants into configurable structs during passes 2-7:

| Struct | Fields | Where |
|---|---|---|
| `DispatchConfig` | `max_request_body_size`, `body_read_timeout`, `max_query_string_length` | Both dispatchers |
| `PushRetryPolicy` | `max_attempts`, `backoff` | `HttpPushSender` |
| `HandlerLimits` | `max_id_length`, `max_metadata_size`, `max_cancellation_tokens`, `max_token_age` | `RequestHandler` |

---

## Pass 1: Initial Dogfood (3 bugs)

### Bug 1: REST Transport Strips Required Fields from Push Config Body

**Severity:** Medium | **Component:** Client REST transport + Server dispatch

The client's REST transport extracts path parameters from the serialized JSON body to interpolate URL templates. For `CreateTaskPushNotificationConfig`, the route is `/tasks/{taskId}/pushNotificationConfigs`, so the transport extracts `taskId` from the body and *removes it*. But the server handler requires `taskId` in the body.

**Why tests missed it:** Unit tests test client transport and server dispatch in isolation. The bug only appears when they interact.

**Fix:** Server-side `handle_set_push_config` injects `taskId` from the URL path back into the body before parsing.

### Bug 2: `on_response` Metrics Hook Never Called

**Severity:** Low | **Component:** `RequestHandler`

`Metrics::on_response` showed 0 responses after 17 successful requests. The hook was defined but never called in any handler method.

**Fix:** Added `on_request()`/`on_response()` calls to all 10 handler methods.

### Bug 3: Protocol Binding Mismatch Produces Confusing Errors

**Severity:** Low | **Component:** Client JSON-RPC transport

When a JSON-RPC client called a REST-only server, the error was an opaque parsing failure rather than "wrong protocol binding."

**Fix:** (1) New `ClientError::ProtocolBindingMismatch` variant, (2) JSON-RPC transport detects non-JSON-RPC responses, (3) HealthMonitor uses agent card discovery to select correct transport.

---

## Pass 2: Hardening Audit (6 bugs)

### Bug 4: `list_push_configs` REST Response Format Mismatch

**Severity:** Medium | **Component:** Both dispatchers

Dispatchers serialized results as raw `Vec<TaskPushNotificationConfig>`, but the client expected `ListPushConfigsResponse { configs, next_page_token }`.

**Fix:** Both dispatchers wrap results in `ListPushConfigsResponse`.

### Bug 5: Push Notification Test Task ID Mismatch

**Severity:** Medium | **Component:** Test design

Push config was registered on Task A, but a subsequent `send_message` created Task B. No config existed for Task B.

**Fix:** Restructured as push config CRUD lifecycle test.

### Bug 6: `on_error` Metrics Hook Never Fired

**Severity:** Low | **Component:** `RequestHandler`

All handler error paths used `?` to propagate without invoking `on_error`.

**Fix:** All 10 handler methods restructured with async block + match on `on_response`/`on_error`.

### Bug 7: `on_queue_depth_change` Metrics Hook Never Fired

**Severity:** Low | **Component:** `EventQueueManager`

`EventQueueManager` had no access to the `Metrics` object.

**Fix:** Added `Arc<dyn Metrics>` to `EventQueueManager`, wired from builder.

### Bug 8: `JsonRpcDispatcher` Does Not Serve Agent Cards

**Severity:** Medium | **Component:** `JsonRpcDispatcher`

`resolve_agent_card()` failed for JSON-RPC agents because only `RestDispatcher` served `/.well-known/agent.json`.

**Fix:** Added `StaticAgentCardHandler` to `JsonRpcDispatcher`.

### Bug 9: `SubscribeToTask` Fails with Concurrent SSE Streams

**Severity:** High | **Component:** `EventQueueManager`

`mpsc` channels allow only a single reader. Once `stream_message` took the reader, `subscribe_to_task` failed with "no active event queue."

**Fix:** Redesigned from `mpsc` to `tokio::sync::broadcast` channels. `subscribe()` creates additional readers from the same sender.

---

## Pass 3: Stress Testing (1 bug)

### Bug 10: Client Ignores `return_immediately` Config

**Severity:** High | **Component:** Client `send_message()`

`ClientBuilder::with_return_immediately(true)` stored the flag in `ClientConfig` but `send_message()` never injected it into `MessageSendParams.configuration`. The server never saw the flag, so tasks always ran to completion.

**Why tests missed it:** The server-side `return_immediately` logic was correct. The bug was in client-to-server config propagation — a seam that only E2E testing exercises.

**Fix:** Added `apply_client_config()` that merges client-level `return_immediately`, `history_length`, and `accepted_output_modes` into params before sending. Per-request values take precedence over client defaults.

---

## Pass 4: SDK Regression Testing (3 bugs)

### Bug 11: JSON-RPC `ListTaskPushNotificationConfigs` Param Type Mismatch

**Severity:** Critical | **Component:** `JsonRpcDispatcher`

The JSON-RPC dispatcher parsed `ListTaskPushNotificationConfigs` params as `TaskIdParams` (field `id`), but the client sends `ListPushConfigsParams` (field `task_id`). This caused silent deserialization failure — push config listing via JSON-RPC was completely broken. REST worked because it uses path-based routing.

**Why tests missed it:** Previous push config tests used REST transport or tested create/get/delete but not list. The JSON-RPC list path was never exercised end-to-end.

**Fix:** Changed `parse_params::<TaskIdParams>` to `parse_params::<ListPushConfigsParams>` and `p.id` to `p.task_id` in `jsonrpc.rs`.

### Bug 12: Agent Card URLs Set to "http://placeholder"

**Severity:** Critical | **Component:** Agent-team example

Agent cards were constructed with `code_analyzer_card("http://placeholder")` *before* the server bound to a port. The actual address was only known after `TcpListener::bind()`. This meant `/.well-known/agent.json` served a card with a URL that didn't match the actual server address.

**Why tests missed it:** Tests used URLs from `TestContext` (the real bound addresses), not from the agent card. Only `resolve_agent_card()` tests would have caught this, and those didn't exist.

**Fix:** Introduced `bind_listener()` that pre-binds TCP listeners to get addresses *before* handler construction. Cards are now built with correct URLs.

### Bug 13: Push Notification Event Classification Broken

**Severity:** Medium | **Component:** Agent-team webhook receiver

The webhook receiver classified events by checking `value.get("status")` and `value.get("artifact")`, but `StreamResponse` serializes as `{"statusUpdate": {...}}` / `{"artifactUpdate": {...}}` (camelCase variant names). All push events were classified as "Unknown".

**Fix:** Check `statusUpdate`/`artifactUpdate`/`task` instead.

---

## Pass 5: Hardening & Concurrency Audit (4 bugs)

### Bug 14: Lock Poisoning Silently Masked in `InMemoryCredentialsStore`

**Severity:** High | **Component:** `InMemoryCredentialsStore`

All three `CredentialsStore` methods (`get`, `set`, `remove`) used `.ok()?` or `if let Ok(...)` to silently ignore `RwLock` poisoning. If a thread panicked while holding the lock, subsequent calls would return `None` (for `get`) or silently skip the operation (for `set`/`remove`), masking the underlying bug.

**Why tests missed it:** Tests don't exercise lock poisoning because `#[test]` functions that panic abort the test, not the lock.

**Fix:** Changed all three methods to `.expect("credentials store lock poisoned")` for fail-fast behavior. Added documentation explaining the poisoning semantics.

### Bug 15: Rate Limiter TOCTOU Race on Window Advance

**Severity:** High | **Component:** `RateLimitInterceptor`

When two concurrent requests arrived at a window boundary, both could see the old `window_start`, and both would store `count = 1` for the new window. This let 2N requests through per window instead of N.

The race: Thread A loads `window_start` (old), Thread B loads `window_start` (old), Thread A stores new `window_start` + count=1, Thread B stores new `window_start` + count=1 (clobbering A's count).

**Why tests missed it:** The single-threaded test executor doesn't interleave atomic operations. The race only manifests under real concurrent load.

**Fix:** Replaced the non-atomic load-check-store sequence with a `compare_exchange` (CAS) loop. Only one thread wins the CAS to reset the window; others retry and see the updated window on the next iteration.

### Bug 16: Rate Limiter Unbounded Bucket Growth

**Severity:** Medium | **Component:** `RateLimitInterceptor`

The `buckets` HashMap grew without bound. Each unique caller key created a `CallerBucket` that was never removed, even after the caller departed. In a service with many transient callers (e.g., serverless functions), this would leak memory indefinitely.

**Why tests missed it:** Tests use a small fixed set of callers. The leak only manifests with high caller churn over time.

**Fix:** Added amortized stale-bucket cleanup (every 256 `check()` calls). Buckets whose `window_start` is more than one window old are evicted.

### Bug 17: No Protocol Version Compatibility Warning

**Severity:** Low | **Component:** `ClientBuilder`

`ClientBuilder::from_card()` silently accepted any `protocol_version` from the agent card, including incompatible versions (e.g., `"2.0.0"` when the client supports `"1.x"`). Users would only discover the mismatch through obscure deserialization errors.

**Fix:** Added protocol version major-version check in `from_card()`. When the agent's major version differs from the client's supported version, a `tracing::warn!` is emitted.

---

## Pass 6: Architecture Audit (5 bugs)

### Bug 18: gRPC Agent Card Still Uses Placeholder URL

**Severity:** Critical | **Component:** Agent-team example (gRPC path)

The gRPC `CodeAnalyzer` agent was still constructed with `grpc_analyzer_card("http://placeholder")` — the exact same Bug #12 pattern that was fixed for HTTP agents in Pass 4. The gRPC path was missed because it was behind a `#[cfg(feature = "grpc")]` gate.

**Why tests missed it:** gRPC tests used the address from `serve_grpc()` return value, not from the agent card. Agent card discovery tests only ran for HTTP agents.

**Fix:** Added `GrpcDispatcher::serve_with_listener()` to accept a pre-bound `TcpListener`. The agent-team example now pre-binds for gRPC the same way it does for HTTP, ensuring the agent card URL is correct from the start.

### Bug 19: REST Transport Query Strings Not URL-Encoded

**Severity:** High | **Component:** Client REST transport

`build_query_string()` concatenated parameter values directly into query strings without percent-encoding. Values containing `&`, `=`, spaces, or other special characters would corrupt the query string, causing server-side parsing failures or parameter injection.

**Why tests missed it:** All test parameters used simple alphanumeric values that don't need encoding.

**Fix:** Added `encode_query_value()` implementing RFC 3986 percent-encoding for all non-unreserved characters. Both keys and values are now encoded.

### Bug 20: WebSocket Stream Termination Uses Fragile String Matching

**Severity:** Medium | **Component:** Client WebSocket transport

The WebSocket stream reader detected stream completion by checking `text.contains("stream_complete")`. This would false-positive on any payload containing that substring (e.g., a task whose output mentions "stream_complete") and would miss terminal status updates that don't contain that exact string.

**Fix:** Replaced with `is_stream_terminal()` that deserializes the JSON-RPC frame and checks for terminal task states (`completed`, `failed`, `canceled`, `rejected`) or the `stream_complete` sentinel in the result object.

### Bug 21: Background Event Processor Silently Drops Store Write Failures

**Severity:** High | **Component:** Server `RequestHandler` background processor

In streaming mode, the background event processor called `let _ = task_store.save(...).await;` at 5 call sites, silently discarding any store errors. If the store failed (disk full, permission denied), task state would diverge: the event queue showed completion but the persistent store didn't record it.

**Why tests missed it:** In-memory stores don't fail. The bug only manifests with persistent backends under storage pressure.

**Fix:** All 5 sites now use `if let Err(_e) = task_store.save(...).await { trace_error!(...) }` to surface failures via structured logging.

### Bug 22: Metadata Size Validation Bypass via Serialization Failure

**Severity:** Medium | **Component:** Server `RequestHandler` messaging

Metadata size was measured with `serde_json::to_string(meta).map(|s| s.len()).unwrap_or(0)`. If serialization failed, the size defaulted to 0, bypassing the size limit entirely.

**Fix:** Changed `unwrap_or(0)` to `map_err(|_| ServerError::InvalidParams("metadata is not serializable"))`, rejecting unserializable metadata outright.

---

## Pass 7: Deep Dogfood (9 bugs)

### Bug 23: Graceful Shutdown Hangs on Executor Cleanup

**Severity:** High | **Component:** `RequestHandler`

`shutdown()` called `executor.on_shutdown().await` with no timeout. If an executor's cleanup routine blocked indefinitely, the entire shutdown process would hang.

**Fix:** Both `shutdown()` and `shutdown_with_timeout()` now bound the executor cleanup call with a timeout (10 seconds for `shutdown()`, the provided timeout for `shutdown_with_timeout()`).

### Bug 24: Push Notification Body Clone Per Retry Attempt

**Severity:** Medium | **Component:** `HttpPushSender`

`body_bytes.clone()` inside the retry loop allocated a full copy of the serialized event body for every retry attempt. For large events with 3 retries, this caused 3 unnecessary heap allocations.

**Fix:** Changed `body_bytes` from `Vec<u8>` to `Bytes` (reference-counted). Clone is now an atomic reference count increment instead of a memory copy.

### Bug 25: Webhook URL Missing Scheme Validation

**Severity:** High | **Component:** `HttpPushSender`

`validate_webhook_url()` checked for private IPs and hostnames but did not validate the URL scheme. URLs like `ftp://evil.com/hook` or `file:///etc/passwd` bypassed all SSRF validation.

**Fix:** Added explicit scheme check requiring `http://` or `https://`. Unknown schemes and schemeless URLs are now rejected with a descriptive error.

### Bug 26: Push Config Store Unbounded Global Growth (DoS)

**Severity:** Critical | **Component:** `InMemoryPushConfigStore`

The per-task config limit (100) prevented excessive configs per task, but there was no global limit. An attacker could create millions of tasks with 100 configs each, exhausting memory.

**Fix:** Added `max_total_configs` field (default 100,000) with `with_max_total_configs()` builder. The global check runs before the per-task check in `set()`.

### Bug 27: gRPC Error Code Mapping Incomplete

**Severity:** Medium | **Component:** Client gRPC transport

Only 4 tonic status codes were mapped to A2A error codes. `Unauthenticated`, `PermissionDenied`, `ResourceExhausted`, `DeadlineExceeded`, `Cancelled`, and `Unavailable` all silently mapped to `InternalError`, losing semantic information.

**Fix:** Added explicit mappings for 6 additional gRPC status codes.

### Bug 28: BuildMonitor Cancel Race Condition

**Severity:** Medium | **Component:** Agent-team example

`BuildMonitorExecutor::cancel()` unconditionally emitted `TaskState::Canceled` without checking if the task had already reached a terminal state. If the executor completed between the handler checking and the cancel arriving, this caused an invalid `Completed → Canceled` transition.

**Fix:** Added `ctx.cancellation_token.is_cancelled()` guard before emitting cancel status.

### Bug 29: CodeAnalyzer Missing Cancellation Re-Check

**Severity:** Low | **Component:** Agent-team example

`CodeAnalyzerExecutor` only checked cancellation once (before artifact emission). Multiple artifact emissions happened without re-checking, meaning cancellation between artifacts was delayed.

**Fix:** Added cancellation re-check between the two artifact emission phases.

### Bug 30: Accept Loop Breaks Kill Servers

**Severity:** Medium | **Component:** Agent-team infrastructure

All three server startup functions (`serve_jsonrpc`, `serve_rest`, `start_webhook_server`) used `Err(_) => break` in their accept loops. A single transient accept error (e.g., EMFILE) would permanently kill the server.

**Fix:** Changed to `Err(_) => continue` for JSON-RPC and REST servers, and `eprintln!` + `continue` for the webhook receiver.

### Bug 31: Coordinator Silent Client Build Failure

**Severity:** Low | **Component:** Agent-team example

When `ClientBuilder::build()` failed for an agent URL, the error was silently discarded (`if let Ok(client) = ...`). If all agents failed, the coordinator would run with an empty client map, producing confusing "Unknown command" errors.

**Fix:** Changed to `match` with `Err(e) => eprintln!(...)` to log the failing agent name, URL, and error.

---

## Pass 8: Deep Dogfood (5 bugs)

### Bug 32: Timeout Errors Misclassified as Transport (CRITICAL)

**Severity:** Critical | **Component:** Client REST + JSON-RPC transports

Both `RestTransport::execute_request()` and `JsonRpcTransport::execute_request()` mapped `tokio::time::timeout` errors to `ClientError::Transport`. Since `Transport` is explicitly marked **non-retryable** in `is_retryable()`, timeouts never triggered retry logic — defeating the entire retry system for the most common transient failure mode.

**Why tests missed it:** Unit tests checked that timeouts produced errors, but never checked the error *variant*. The retry integration tests used simulated errors, not real timeouts.

**Fix:** Changed both transports to use `ClientError::Timeout("request timed out")`. Added exhaustive retryability classification tests.

### Bug 33: SSE Parser O(n) Dequeue Performance

**Severity:** Medium | **Component:** Client SSE parser

`SseParser::next_frame()` used `Vec::remove(0)` which is O(n) because it shifts all remaining elements. With high-throughput streaming (hundreds of events per second), this creates quadratic overhead.

**Why tests missed it:** Unit tests parse small event counts (1-3 events). The performance issue only manifests with large event queues.

**Fix:** Replaced `Vec<Result<SseFrame, SseParseError>>` with `VecDeque` for O(1) `pop_front()`.

### Bug 34: SSE Parser Silent UTF-8 Data Loss

**Severity:** Medium | **Component:** Client SSE parser

Malformed UTF-8 lines were silently discarded (`return` on `from_utf8` failure). When a multi-byte UTF-8 character spans a TCP chunk boundary, the trailing bytes can appear invalid. The entire line would be dropped, causing silent data loss.

**Why tests missed it:** All test inputs use ASCII. The bug only manifests with non-ASCII content delivered across TCP fragment boundaries.

**Fix:** Changed to `String::from_utf8_lossy()` which replaces invalid bytes with U+FFFD instead of dropping the entire line.

### Bug 35: Double-Encoded Path Traversal Bypass

**Severity:** Medium | **Component:** Server REST dispatcher

`contains_path_traversal()` only decoded one level of percent-encoding. An attacker could use `%252E%252E` (which decodes to `%2E%2E`, then to `..`) to bypass the check.

**Why tests missed it:** No test used double-encoded inputs. The existing test only checked raw `..` sequences.

**Fix:** Added a second decoding pass. Added tests for raw, single-encoded, and double-encoded path traversal.

### Bug 36: gRPC Stream Errors Lose Error Context

**Severity:** Low | **Component:** Client gRPC transport

`grpc_stream_reader_task` mapped gRPC stream errors to generic `ClientError::Transport(format!("gRPC stream error: {}", status.message()))` instead of using the existing `grpc_code_to_error_code()` function. This lost the structured error code information (NotFound, InvalidArgument, etc.).

**Why tests missed it:** gRPC streaming tests check for successful completion, not error paths within the stream.

**Fix:** Changed to use `ClientError::Protocol(A2aError::new(grpc_code_to_error_code(...), ...))` for proper error classification.

---

## Pass 9: Scale Probing (4 bugs)

### Bug 37: SSE Parser Unbounded Error Queue (OOM Risk)

**Severity:** Medium | **Component:** Client SSE parser

`SseParser` queued errors into an unbounded `VecDeque`. A malicious or corrupted SSE stream producing many oversized events could fill the queue with error entries, causing OOM on the client side.

**Why tests missed it:** Tests consume frames immediately after feeding. The bug only manifests when a consumer falls behind a producer generating many errors.

**Fix:** Added `max_queued_frames` limit (default 4096) with `with_max_queued_frames()` builder. When the limit is reached, the oldest frame/error is dropped via `pop_front()` before pushing the new one.

### Bug 38: Background Event Processor Misses Fast Executor Events (Known Limitation)

**Severity:** High | **Component:** Server `RequestHandler` background event processor

In streaming mode, `spawn_background_event_processor` subscribes to the broadcast channel *after* `yield_now()`. If the executor completes before the subscription is active, the background processor misses all events. This means the task store may not be updated to the terminal state for fast executors in streaming mode.

The root cause is architectural: `tokio::sync::broadcast::subscribe()` only delivers events sent *after* the subscription. Events already in the channel are lost.

**Why tests missed it:** Most test executors include artificial delays. The bug only manifests with very fast executors (e.g., pure computation without I/O).

**Status:** Documented as known limitation. The SSE consumer (which has the reader from queue creation) sees all events correctly. Only the background store-update path is affected. A proper fix requires either: (1) subscribing before spawning the executor, or (2) replaying missed events from the channel's buffer.

### Bug 39: Retry Backoff Float Overflow Can Panic

**Severity:** Low | **Component:** Client retry policy

`cap_backoff()` computed `Duration::from_secs_f64(current.as_secs_f64() * multiplier)` without checking for infinity or NaN. With extreme multiplier values or near-`Duration::MAX` durations, the multiplication could produce `f64::INFINITY`, causing `from_secs_f64` to panic.

**Why tests missed it:** Default multiplier is 2.0 and backoffs are small. The overflow only occurs with adversarial configurations.

**Fix:** Added `is_finite()` and negativity checks before the `Duration` conversion. Non-finite results clamp to `max_backoff`.

### Bug 40: Agent-Team Test Suite Missing Coverage for 10 SDK Features

**Severity:** Medium | **Component:** Agent-team example

The agent-team E2E test suite (previously 71 tests) had gaps in:
- State transition validation (no backwards-transition check)
- Executor error → Failed state propagation (not verified in streaming)
- Streaming event completeness (only checked first/last, not sequence)
- Oversized metadata rejection (never tested E2E)
- Artifact content correctness (only checked existence, not content)
- GetTask after streaming (background processor sync not verified)
- Rapid sequential throughput (no sustained load test)
- Cancel terminal-state tasks (only cancel of active tasks tested)
- Agent card semantic validation (only checked non-empty, not structure)
- GetTask history content (API success checked, not response content)

**Fix:** Added 10 new deep dogfood tests (tests 81-90) covering all gaps. Test suite now has 81 base tests (94 with all optional features).

---

## Pass 10: Exhaustive Audit (3 bugs)

### Bug 41: Event Queue Serialization Error Silently Swallowed

**Severity:** High | **Component:** Server `InMemoryQueueWriter`

`InMemoryQueueWriter::write()` used `unwrap_or(0)` when measuring serialized event size via `CountingWriter`. If `serde_json::to_writer` failed during size measurement, the error was silently masked — the serialized size was reported as 0, the size check passed, and the potentially unserializable event was sent through the broadcast channel.

**Why tests missed it:** All test events are valid `StreamResponse` variants that serialize successfully. The failure path was unreachable with well-formed types, but could be triggered by future enum variants or custom serialization implementations.

**Fix:** Replaced `unwrap_or(0)` with `map_err(|e| A2aError::internal(...))` followed by `?` operator, propagating the serialization error to the caller.

### Bug 42: Capacity Eviction Fails When Insufficient Terminal Tasks

**Severity:** Medium | **Component:** Server `InMemoryTaskStore` eviction

`InMemoryTaskStore` capacity eviction only removed terminal (Completed/Failed/Canceled) tasks when the store exceeded `max_capacity`. If the store was over capacity but all tasks were non-terminal (Working/Submitted), the eviction loop found zero terminal tasks to remove and silently returned — leaving the store permanently over capacity.

**Why tests missed it:** Existing eviction tests always included at least one terminal task. The edge case of an all-non-terminal store was never exercised.

**Fix:** Added a fallback eviction path that removes the oldest non-terminal tasks when there aren't enough terminal tasks to bring the store under capacity. Added a new unit test `capacity_eviction_falls_back_to_non_terminal_when_needed`.

### Bug 43: Lagged Event Count Not Exposed in Warning

**Severity:** Low | **Component:** Server `InMemoryQueueReader`

The broadcast channel `Lagged(n)` error provides the exact count of dropped events, but the reader used `_n` (underscore prefix) which discarded the value. The `trace_warn!` message said "skipping missed events" without saying how many.

**Why tests missed it:** Observability quality isn't tested by functional tests. The behavior was correct (events were skipped), but the diagnostic information was incomplete.

**Fix:** Changed `_n` to `n` and included it in the warning: `"event queue reader lagged, {n} events skipped"`.
