# Pitfalls & Lessons Learned

A catalog of non-obvious problems encountered during development. Each entry documents a real issue and its solution.

## Serde Pitfalls

### Untagged enums hide inner errors

`#[serde(untagged)]` on `JsonRpcResponse<T>` and `SendMessageResponse` swallows the real deserialization error and replaces it with a generic "data did not match any variant" message. (Note: `StreamResponse` uses `#[serde(rename_all = "camelCase")]` — externally tagged — so it produces clearer errors.)

**Workaround:** When debugging, temporarily switch to an externally tagged enum to see the real error. In production, log the raw JSON before attempting deserialization.

### `#[serde(default)]` vs `Option<T>`

Using `#[serde(default)]` on a `Vec<T>` field means the field is present but empty when omitted from JSON. Using `Option<Vec<T>>` means it is absent (`None`).

The A2A spec distinguishes between "omitted" and "empty array" for fields like `history` and `artifacts`, so `Option<Vec<T>>` is the correct choice.

```rust
// Wrong: field is always present (empty vec if omitted)
#[serde(default)]
pub history: Vec<Message>,

// Correct: field is absent when not provided
#[serde(skip_serializing_if = "Option::is_none")]
pub history: Option<Vec<Message>>,
```

### `#[non_exhaustive]` on enums breaks downstream matches

Adding `#[non_exhaustive]` to `TaskState`, `ErrorCode`, etc. forces downstream crates to include a wildcard arm. This is intentional for forward compatibility:

```rust
match task.status.state {
    TaskState::Completed => { /* ... */ }
    TaskState::Failed => { /* ... */ }
    _ => { /* Handle future states */ }
}
```

## Hyper 1.x Pitfalls

### Body is not Clone

Hyper 1.x `Incoming` body is consumed on read. You cannot read the body twice. Buffer it first:

```rust
use http_body_util::BodyExt;

let bytes = req.into_body().collect().await?.to_bytes();
// Now work from `bytes` (can be cloned, read multiple times)
```

### Response builder panics on invalid header values

`hyper::Response::builder().header(k, v)` silently stores the error and panics when `.body()` is called.

**Solution:** Always use `unwrap_or_else` with a fallback response, or validate header values with `.parse::<HeaderValue>()` first. a2a-rust uses `unwrap_or_else(|_| fallback_error_response())` in production paths.

### `size_hint()` upper bound may be None

`hyper::body::Body::size_hint().upper()` returns `None` when Content-Length is absent. Always check for `None` before comparing against the body size limit:

```rust
if let Some(upper) = body.size_hint().upper() {
    if upper > MAX_BODY_SIZE {
        return Err(/* payload too large */);
    }
}
```

## SSE Streaming Pitfalls

### SSE parser must handle partial lines

SSE events may arrive split across TCP frames. The parser must buffer partial lines and only process complete `\n`-terminated lines. The `SseParser` in `a2a-protocol-client` handles this correctly, but naive `lines()` iterators will break on partial frames.

### Memory limit on buffered SSE data

A malicious server can send an infinite SSE event (no `\n\n` terminator). The client parser enforces a 16 MiB cap on buffered event data to prevent OOM.

## Push Notification Pitfalls

### SSRF via webhook URLs

Push notification `url` fields can point to internal services (e.g., `http://169.254.169.254/` — the cloud metadata endpoint).

**Solution:** The `HttpPushSender` resolves the URL and rejects private/loopback IP addresses *after DNS resolution*, not on the URL string alone. The `validate_webhook_url_with_dns()` function performs DNS resolution before IP validation, preventing DNS rebinding attacks where a hostname resolves to a public IP during validation but a private IP during the actual HTTP request.

### Header injection via credentials

Push notification `credentials` can contain newlines that inject additional HTTP headers.

**Solution:** The push sender validates that credential values contain no `\r` or `\n` characters before using them in HTTP headers.

## Async / Tokio Pitfalls

### Object-safe async traits need `Pin<Box<dyn Future>>`

Rust does not yet support `async fn` in traits that are used as `dyn Trait`. The `TaskStore`, `AgentExecutor`, and `PushSender` traits use explicit `Pin<Box<dyn Future<Output = ...> + Send + 'a>>` return types:

```rust
// The pattern for all object-safe async trait methods
fn my_method<'a>(
    &'a self,
    args: &'a Args,
) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>> {
    Box::pin(async move {
        // async code here
        Ok(result)
    })
}
```

### Cancellation token cleanup

`CancellationToken`s that are never cancelled accumulate in the token map. The handler cleans up already-cancelled tokens before inserting new ones, and enforces a hard cap of 10,000 tokens.

### Amortized eviction vs test expectations

Running O(n) eviction on every `save()` call is expensive. The task store amortizes eviction to every 64 writes. Tests that depend on eviction must call `run_eviction()` explicitly or account for the amortization interval.

### `std::sync::RwLock` poisoning — fail-fast vs silent ignore

When a thread panics while holding a `std::sync::RwLock`, the lock becomes "poisoned." There are two strategies:

1. **Silent ignore** (`lock.read().ok()?`) — returns `None`/no-op on poisoned locks. This hides bugs.
2. **Fail-fast** (`lock.read().expect("poisoned")`) — panics immediately, surfacing the problem.

a2a-rust uses fail-fast. If you see a "lock poisoned" panic, the root cause is a prior panic in another thread. Fix that panic first.

### Rate limiter atomics under read lock — CAS loop required

When multiple threads share a rate limiter bucket under a read lock, non-atomic multi-step operations (load window → check → store count) create TOCTOU races. Two threads can both see an old window and both reset the counter.

**Solution:** Use `compare_exchange` (CAS) to atomically swap the window number. Only one thread wins the CAS; others loop and retry with the updated state.

## Transport Pitfalls

### Query string parameters must be URL-encoded

When building REST transport query strings from JSON values, parameter values must be percent-encoded per RFC 3986. A value like `status=active&role=admin` without encoding becomes three separate query parameters instead of one.

**Solution:** The `build_query_string()` function in the REST transport encodes all non-unreserved characters (`A-Z a-z 0-9 - . _ ~`).

### WebSocket stream termination must use structured parsing

Detecting stream completion by checking `text.contains("stream_complete")` is fragile — it false-positives on any payload text containing that substring, and misses terminal status updates that don't contain that exact string.

**Solution:** Deserialize the JSON-RPC frame and check the result object for terminal task states (`completed`, `failed`, `canceled`, `rejected`) or the `stream_complete` sentinel.

### gRPC transport must not serialize requests through a Mutex

Wrapping the tonic `Channel` in a `Mutex` serializes all concurrent gRPC requests, destroying throughput. Since tonic `Channel` is internally multiplexed over HTTP/2 and cheap to clone, the correct approach is to clone the channel for each request.

**Solution:** Clone the `Channel` per request instead of holding a `Mutex` guard. This enables full concurrent throughput.

### WebSocket transport must not hold a reader lock across the stream

Holding a `Mutex` on the WebSocket reader for the entire duration of reading a response deadlocks when multiple requests are in flight — each request waits for the reader lock while the reader is blocked waiting for a response that may not be the one needed.

**Solution:** Use a dedicated background reader task that reads all incoming messages and routes them to the correct pending request via a `HashMap<RequestId, PendingRequest>`. This eliminates the deadlock and enables true concurrent request/response multiplexing.

### WebSocket upgrade requests must include auth headers

Auth interceptor headers were applied to JSON-RPC/REST HTTP requests but not to the WebSocket upgrade request. This caused authentication failures when connecting to WebSocket endpoints that require authentication.

**Solution:** Apply extra headers (including auth interceptor headers) to the WebSocket upgrade HTTP request via the tungstenite `IntoClientRequest` trait.

### Pre-bind listeners for all server types when building agent cards

The gRPC dispatcher had the same placeholder-URL bug as the HTTP dispatchers: the agent card was built before the server bound to a port, so the card contained `"http://placeholder"`. This applied to any transport that binds its own port.

**Solution:** Pre-bind a `TcpListener`, extract the address, build the handler with the correct URL, then pass the listener via `serve_with_listener()` (gRPC) or the accept loop (HTTP).

### Background tasks cannot propagate errors — log them

In a `tokio::spawn`-ed task, errors have no caller to return to. Using `let _ = store.save(...).await` silently drops failures, causing task state to diverge between the event queue and the persistent store.

**Solution:** Use `if let Err(e) = store.save(...).await { trace_error!(...) }` so failures are observable via logs and metrics.

## Push Config Store Pitfalls

### Per-resource limits are not enough — add global limits

A per-task push config limit (e.g., 100 configs per task) does not prevent an attacker from creating millions of tasks with 100 configs each. Always pair per-resource limits with a global cap (e.g., `max_total_configs = 100_000`). The global check must run before the per-resource check in the write path.

### Webhook URL scheme validation

Checking for private IPs and hostnames in webhook URLs is insufficient. URLs like `ftp://evil.com/hook` or `file:///etc/passwd` bypass IP-based SSRF checks entirely. Always validate that the URL scheme is `http` or `https` before performing any further validation.

## Performance Pitfalls

### `Vec<u8>` vs `Bytes` in retry loops

Cloning a `Vec<u8>` inside a retry loop allocates a full heap copy each time. Use `bytes::Bytes` (reference-counted) so that `.clone()` is just an atomic reference count increment. This matters for push notification delivery where large payloads may be retried 3+ times.

### Serialize once before retry loops

Deep-cloning a `serde_json::Value` tree on every retry attempt is expensive. Serialize the params to bytes once before the retry loop, then deserialize from bytes for each attempt. Deserialization from bytes is cheaper than a recursive deep-clone of the `Value` tree.

### Agent card fetch responses need a body size limit

Fetching an agent card from an untrusted URL without a body size limit allows a malicious endpoint to send an arbitrarily large response, causing OOM. Always enforce a body size limit (e.g., 2 MiB) on agent card fetch responses.

## Graceful Shutdown Pitfalls

### Bound executor cleanup with a timeout

`executor.on_shutdown().await` can hang indefinitely if the executor's cleanup routine blocks. Always wrap with `tokio::time::timeout()` — the default is 10 seconds. Users can override via `shutdown_with_timeout(duration)`.

### Shutdown polling interval matters

A 50ms polling interval in the shutdown loop wastes up to 50ms per cycle. Using a 10ms interval with deadline-aware sleep (sleeping the minimum of the remaining deadline and the interval) gives faster shutdown response without busy-waiting.

## gRPC Pitfalls

### Map all tonic status codes, not just the common ones

Only mapping 4 gRPC status codes to A2A error codes loses semantic information for `Unauthenticated`, `PermissionDenied`, `ResourceExhausted`, etc. Map all relevant codes explicitly so clients can distinguish between error categories.

### Feature-gated code paths need their own dogfood pass

The gRPC path behind `#[cfg(feature = "grpc")]` had the exact same placeholder URL bug that was already fixed for HTTP (Bug #12 → Bug #18). Feature-gated code is easy to miss during reviews. Always re-verify known bug patterns across all feature gates.

## Cancel / State Machine Pitfalls

### Check terminal state before emitting cancel

If an executor completes between the handler's cancel check and the cancel signal arrival, unconditionally emitting `TaskState::Canceled` causes an invalid `Completed → Canceled` state transition. Guard cancel emission with `cancellation_token.is_cancelled()` or a terminal-state check.

### Re-check cancellation between multi-phase work

If an executor performs multiple phases (e.g., two artifact emissions), check cancellation between phases. Otherwise, cancellation between phases is delayed until the next natural check point.

## Server Accept Loop Pitfalls

### `break` vs `continue` on transient accept errors

Using `Err(_) => break` in a TCP accept loop kills the entire server on a single transient error (e.g., `EMFILE` when the file descriptor limit is reached). Use `Err(_) => continue` to skip the bad connection and keep serving. Log the error for observability but do not let it take down the server.

## Workspace / Cargo Pitfalls

### `cargo-fuzz` needs its own workspace

The `fuzz/` directory contains its own `Cargo.toml` with `[workspace]` to prevent cargo-fuzz from conflicting with the main workspace. The fuzz crate references `a2a-protocol-types` via a relative path dependency.

### Feature unification across workspace

Enabling a feature in one crate (e.g., `signing` in `a2a-protocol-types`) enables it for all crates in the workspace during `cargo test --workspace`. Use `--no-default-features` or per-crate test commands when testing feature gates.

## Testing Pitfalls

### HashMap iteration order is non-deterministic

The `InMemoryTaskStore` uses a `HashMap` internally. `list()` must sort results by task ID before applying pagination; otherwise, page boundaries shift between runs and tests become flaky.

### Percent-encoded path traversal

Checking `path.contains("..")` is insufficient. Attackers can use `%2E%2E` (or mixed-case `%2e%2E`) to bypass the check.

**Solution:** The REST dispatcher percent-decodes the path *before* checking for `..` sequences.

## Scale & Durability Pitfalls

### SSE parser queue can grow unbounded (fixed)

A malicious SSE stream sending many oversized events could fill the parser's internal frame queue without bound. The fix caps the queue at 4096 frames (configurable via `with_max_queued_frames`), dropping the oldest when full.

### Retry backoff can overflow to infinity (fixed)

`cap_backoff()` computed `Duration::from_secs_f64(current * multiplier)`. With extreme multipliers, this produces `f64::INFINITY` which panics in `from_secs_f64`. The fix checks for non-finite results and clamps to `max_backoff`.

### Background event processor can miss fast executor events (known limitation)

In streaming mode, the background event processor subscribes to the broadcast channel *after* the executor starts. For very fast executors, events may be written before the subscription is active, meaning the task store is not updated. The SSE consumer always sees all events (it holds the reader from queue creation). See Bug #38 in [dogfooding-bugs](../deployment/dogfooding-bugs.md).

### Event queue writer silently swallows serialization errors (fixed)

`InMemoryQueueWriter::write()` used `unwrap_or(0)` when measuring event size via `CountingWriter`. If serialization failed, the size was reported as 0 and the event was sent through the channel without error. The fix propagates the serialization error via `?` operator, returning `A2aError::internal("event serialization failed: ...")`.

### Capacity eviction cannot evict non-terminal tasks (fixed)

`InMemoryTaskStore` capacity eviction only targeted terminal tasks. If the store was full of non-terminal (Working/Submitted) tasks, eviction found nothing to remove and silently gave up — leaving the store permanently over capacity. The fix adds a fallback path that evicts the oldest non-terminal tasks when there aren't enough terminal tasks.

### Lagged event queue reader drops diagnostic count (fixed)

The broadcast channel `Lagged(n)` error provides the exact count of dropped events, but the reader used `_n` (underscore prefix), discarding the count. The warning message said "skipping missed events" without saying how many. The fix exposes the count in the log: `"event queue reader lagged, {n} events skipped"`.

## Client Pitfalls

### `truncate_body` panics on multi-byte UTF-8 (fixed)

The error body truncation helper sliced at a fixed byte offset (`body[..512]`). For non-ASCII responses (common with international error messages), the offset could fall inside a multi-byte UTF-8 character, causing a panic. The fix uses `is_char_boundary()` to find the nearest safe truncation point.

### SSE parser `line_buf` can grow without bound (fixed)

The SSE parser's internal line buffer grew without limit for lines without newlines. A malicious server sending a single very long line could cause OOM. The fix caps `line_buf` at 2× `max_event_size`.

### REST path parameters are not percent-encoded (fixed)

Path parameters (task IDs, config IDs) were interpolated into REST URLs without encoding. An ID containing `/` or `..` could cause path traversal. The fix percent-encodes all path parameters using the same encoder as query parameters.

### `GetExtendedAgentCard` discards interceptor params (fixed)

The `get_extended_agent_card` method created an empty params object after running interceptors, discarding any modifications made by `before` interceptors. The fix forwards `req.params` instead.

## Validation Pitfalls

### Empty/whitespace-only IDs must be rejected

`TaskId` and `ContextId` should not accept empty or whitespace-only strings. Use `TryFrom` impls that validate input, rejecting values that are empty or contain only whitespace.

### `FileContent` must have at least one data source

A `FileContent` with neither `bytes` nor `uri` set is semantically invalid. Always validate that at least one of the two fields is present via a `validate()` method.

### Push notification URLs need validation

`TaskPushNotificationConfig` URLs should be validated for correct format. A `validate()` method on the config struct catches malformed URLs before they reach the push sender.

### Timestamps should be RFC 3339

`TaskStatus` timestamps should conform to RFC 3339. The `has_valid_timestamp()` method validates this at the type level.

### `CachingCardResolver` must not silently produce empty URLs

If `CachingCardResolver::new()` or `with_path()` receives an invalid base URL, silently producing an empty URL leads to confusing errors later. These constructors should return `ClientResult<Self>` so callers can handle the error immediately.

## Database Pitfalls

### Always use parameterized queries for user-controlled values

Using `format!` to interpolate values into SQL queries (e.g., `LIMIT`) allows SQL injection. Always use parameterized queries, even for values that appear safe like numeric limits.

## WebSocket Server Pitfalls

### Limit concurrent tasks per connection

Without a concurrency limit, a single WebSocket client can spawn unbounded tasks on the server by sending many requests in rapid succession. A per-connection `Semaphore` (e.g., 64 permits) bounds resource usage per client.

### Limit incoming message size

Without a message size check, a client can send arbitrarily large WebSocket frames, causing OOM on the server. Enforce a size limit (e.g., 4 MiB) on incoming WebSocket messages and reject oversized frames with a close code.

## REST Dispatch Pitfalls

### Tenant must be passed through all handler calls

When extracting a tenant from the URL path (e.g., `/tenants/{tenant}/tasks/{taskId}`), the extracted tenant must be forwarded to every handler method call. Missing the tenant on any method causes requests to be processed in the wrong tenant context or with no tenant at all.

## Error Handling Pitfalls

### Never silently swallow store lookup errors

`find_task_by_context` (or similar store lookups) should propagate errors via `?` rather than using patterns like `.ok()` or `unwrap_or(None)` that silently convert store failures into "not found" results.

## Next Steps

- **[Architecture Decision Records](./adrs.md)** — Design decisions behind these choices
- **[Configuration Reference](./configuration.md)** — All tunable parameters
