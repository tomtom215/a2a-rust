# Dogfooding Bugs: Passes 7–8

Deep dogfood bugs discovered during security review, performance analysis, and error handling audits.

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
