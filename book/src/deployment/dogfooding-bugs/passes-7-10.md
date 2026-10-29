<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Passes 7 to 10

Part of [Bugs Found & Fixed](../dogfooding-bugs.md). Fix descriptions are as first applied.

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

### Bug 38: Background Event Processor Misses Fast Executor Events (Fixed)

**Severity:** High | **Component:** Server `RequestHandler` background event processor

In streaming mode, `spawn_background_event_processor` originally subscribed to the broadcast channel *after* `yield_now()`. If the executor completed before the subscription was active, the background processor missed all events, so the task store might never record the terminal state for fast executors in streaming mode.

The root cause was architectural: `tokio::sync::broadcast::subscribe()` only delivers events sent *after* the subscription. Events already in the channel are lost.

**Why tests missed it:** Most test executors include artificial delays. The bug only manifests with very fast executors (e.g., pure computation without I/O).

**Status:** **Fixed** (the "H5" redesign). The background processor no longer consumes the lossy broadcast channel at all: the queue lease creates a dedicated bounded **mpsc persistence channel** *before* the executor is spawned, and the processor drains it — including a post-executor drain pass — so a pure-compute executor that finishes instantly cannot outrun the subscription. Every state transition reaches the store exactly once; the broadcast channel now only fans out to streaming consumers. Covered by `event_processing_tests/background_processor.rs` (including `streaming_mode_background_drains_after_executor_done`).

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
