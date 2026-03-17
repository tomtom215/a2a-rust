# Dogfooding Bugs: Passes 5–6

Hardening-phase bugs discovered during concurrency auditing and architecture review.

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
