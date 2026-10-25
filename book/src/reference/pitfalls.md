# Pitfalls & Lessons Learned

A catalog of non-obvious problems encountered during development. Each entry documents a real issue and its solution.

## Serde Pitfalls

### Untagged enums hide inner errors

`#[serde(untagged)]` swallows the real deserialization error and replaces it with a generic "data did not match any variant" message. `JsonRpcResponse<T>` hit this until 0.7; its `Deserialize` is now hand-written — it enforces exactly one of `result` / `error` (JSON-RPC 2.0 §5) and surfaces a mistyped `result`'s own error — and only its `Serialize` stays untagged. (`SendMessageResponse` and `StreamResponse` are externally tagged — a one-key object whose camelCase key names the variant — and their `Deserialize` is hand-written (`oneof_deserialize!`), so they name the problem too.)

**Workaround:** for an untagged enum of your own, hand-write `Deserialize`, or log the raw JSON before attempting deserialization.

### `#[serde(default)]` vs `Option<T>`

Using `#[serde(default)]` on a `Vec<T>` field means the field is present but empty when omitted from JSON. Using `Option<Vec<T>>` means it is absent (`None`).

The A2A spec distinguishes between "omitted" and "empty array" for fields like `history` and `artifacts`, so `Option<Vec<T>>` is the correct choice.

```rust
# use a2a_protocol_sdk::types::message::Message;
# #[derive(serde::Serialize, serde::Deserialize)]
# struct Wrong {
// Wrong: field is always present (empty vec if omitted)
#[serde(default)]
pub history: Vec<Message>,
# }
# #[derive(serde::Serialize, serde::Deserialize)]
# struct Correct {

// Correct: field is absent when not provided
#[serde(skip_serializing_if = "Option::is_none")]
pub history: Option<Vec<Message>>,
# }
# assert_eq!(serde_json::to_string(&Wrong { history: vec![] }).unwrap(), r#"{"history":[]}"#);
# assert_eq!(serde_json::to_string(&Correct { history: None }).unwrap(), "{}");
```

### `#[non_exhaustive]` on enums breaks downstream matches

Adding `#[non_exhaustive]` to `TaskState`, `ErrorCode`, etc. forces downstream crates to include a wildcard arm. This is intentional for forward compatibility:

```rust
# use a2a_protocol_sdk::prelude::*;
# fn f(task: Task) {
match task.status.state {
    TaskState::Completed => { /* ... */ }
    TaskState::Failed => { /* ... */ }
    _ => { /* Handle future states */ }
}
# }
```

## Hyper 1.x Pitfalls

### Body is not Clone

Hyper 1.x `Incoming` body is consumed on read. You cannot read the body twice. Buffer it first:

```rust,no_run
# async fn f(req: hyper::Request<hyper::body::Incoming>) -> Result<(), Box<dyn std::error::Error>> {
use http_body_util::BodyExt;

let bytes = req.into_body().collect().await?.to_bytes();
// Now work from `bytes` (can be cloned, read multiple times)
# Ok(())
# }
```

### Response builder panics on invalid header values

`hyper::Response::builder().header(k, v)` silently stores the error and `.body()` returns it as an `Err`; unwrapping it is what panics.

**Solution:** Always use `unwrap_or_else` with a fallback response, or validate header values with `.parse::<HeaderValue>()` first. a2a-rust uses `unwrap_or_else(|_| fallback_error_response())` in production paths.

### `size_hint()` upper bound may be None

`hyper::body::Body::size_hint().upper()` returns `None` when Content-Length is absent — a chunked or HTTP/2 body. So the size hint can only reject early; it cannot enforce the limit. Enforce it while reading, with `http_body_util::Limited`:

```rust
# use http_body_util::{BodyExt, Limited};
# use hyper::body::{Body, Bytes, Incoming};
# const MAX_BODY_SIZE: u64 = 4 * 1024 * 1024;
# async fn read(body: Incoming) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
// Fast path: an honest Content-Length over the limit is refused unread.
if let Some(upper) = body.size_hint().upper() {
    if upper > MAX_BODY_SIZE {
        return Err("payload too large".into());
    }
}
// `None` means no declared length: without `Limited`, the whole body would
// be buffered before any check ran.
let bytes = Limited::new(body, MAX_BODY_SIZE as usize).collect().await?.to_bytes();
# Ok(bytes)
# }
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
# use std::future::Future;
# use std::pin::Pin;
# struct Args;
# struct T;
# type Result<X> = std::result::Result<X, ()>;
# struct S;
# impl S {
// The pattern for all object-safe async trait methods
fn my_method<'a>(
    &'a self,
    args: &'a Args,
) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>> {
    Box::pin(async move {
        // async code here
#         let result = T;
        Ok(result)
    })
}
# }
```

### Cancellation token cleanup

`CancellationToken`s that are never cancelled accumulate in the token map. Once the map reaches `max_cancellation_tokens` (10,000 by default) the handler sweeps the cancelled tokens, and the aged ones whose executor has finished, before inserting a new one. It is a sweep threshold, not a hard cap: a token belonging to a live task is never removed, so with more tasks in flight than that the map tracks the in-flight count.

### Amortized eviction vs test expectations

Running O(n) eviction on every `save()` call is expensive. The task store amortizes eviction to every 64 writes. Tests that depend on eviction must call `run_eviction()` explicitly or account for the amortization interval.

### `std::sync::RwLock` poisoning — fail-fast vs silent ignore

When a thread panics while holding a `std::sync::RwLock`, the lock becomes "poisoned." There are two strategies:

1. **Silent ignore** (`lock.read().ok()?`) — returns `None`/no-op on poisoned locks. This hides bugs.
2. **Fail-fast** (`lock.read().expect("poisoned")`) — panics immediately, surfacing the problem.

a2a-rust uses both, on purpose. Where a poisoned lock could hide a security-relevant failure it fails fast: `InMemoryCredentialsStore` panics, because a lookup that quietly returned `None` would be a silent auth downgrade. Where the guarded value cannot be left half-updated (a whole-value assignment, one `HashMap` insert or remove) it recovers with `unwrap_or_else(PoisonError::into_inner)`, as the hot-reload card handler and the WebSocket client's pending-request map do, because one panic must not turn every later request into a panic or, under this workspace's release profile (`panic = "abort"`), a process abort. If you see a "lock poisoned" panic, the root cause is a prior panic in another thread. Fix that panic first.

### Rate limiter atomics under read lock — CAS loop required

When multiple threads share a rate limiter bucket under a read lock, non-atomic multi-step operations (load window → check → store count) create TOCTOU races. Two threads can both see an old window and both reset the counter.

**Solution:** Use `compare_exchange` (CAS) to atomically swap the window number. Only one thread wins the CAS; others loop and retry with the updated state.

## Transport Pitfalls

### Query string parameters must be URL-encoded

When building REST transport query strings from JSON values, parameter values must be percent-encoded per RFC 3986. A value like `status=active&role=admin` without encoding becomes three separate query parameters instead of one.

**Solution:** The `build_query_string()` function in the REST transport encodes all non-unreserved characters (`A-Z a-z 0-9 - . _ ~`).

### WebSocket stream termination must use structured parsing

Detecting stream completion by checking `text.contains("stream_complete")` is fragile — it false-positives on any payload text containing that substring, and misses terminal status updates that don't contain that exact string.

**Solution:** Deserialize the JSON-RPC frame and check the result object for terminal task states (`TASK_STATE_COMPLETED`, `TASK_STATE_FAILED`, `TASK_STATE_CANCELED`, `TASK_STATE_REJECTED`) or the `stream_complete` sentinel.

### gRPC transport must not serialize requests through a Mutex

Wrapping the tonic `Channel` in a `Mutex` serializes all concurrent gRPC requests, destroying throughput. Since tonic `Channel` is internally multiplexed over HTTP/2 and cheap to clone, the correct approach is to clone the channel for each request.

**Solution:** Clone the `Channel` per request instead of holding a `Mutex` guard. This enables full concurrent throughput.

### WebSocket transport must not hold a reader lock across the stream

Holding a `Mutex` on the WebSocket reader for the entire duration of reading a response deadlocks when multiple requests are in flight — each request waits for the reader lock while the reader is blocked waiting for a response that may not be the one needed.

**Solution:** Use a dedicated background reader task that reads all incoming messages and routes them to the correct pending request via a `HashMap<String, PendingRequest>` keyed by request id. This eliminates the deadlock and enables true concurrent request/response multiplexing.

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

A per-task push config limit (e.g., 100 configs per task) does not prevent an attacker from creating millions of tasks with 100 configs each. Always pair per-resource limits with a global cap (e.g., `max_total_configs = 100_000`). Enforce both limits on every create. The order is not what matters: in this SDK's handler the per-task check runs first and the global one second, and the global one is approximate under concurrent creates for different tasks.

### Webhook URL scheme validation

Checking for private IPs and hostnames in webhook URLs is insufficient. URLs like `ftp://evil.com/hook` or `file:///etc/passwd` bypass IP-based SSRF checks entirely. Always validate that the URL scheme is `http` or `https` before performing any further validation.

## Performance Pitfalls

### Deserialization allocation overhead

Standard `serde_json::to_vec` and `serde_json::from_str` allocate fresh buffers on every call. For hot paths (SSE frame building, transport payload serialization), this overhead compounds. The `serde_helpers` module in `a2a-protocol-types` provides optimized alternatives:

- **`SerBuffer`** — Thread-local reusable serialization buffer. `SerBuffer::serialize(&value)` serializes through it and returns the bytes as a new `Vec<u8>` (a copy of the buffer), which saves the buffer's growth reallocations on small payloads; `SerBuffer::serialize_into(writer, &value)` writes straight into your own writer with no intermediate buffer.
- **`deser_from_str` / `deser_from_slice`** — Thin wrappers over `serde_json::from_str` / `from_slice`. They help only for types with borrowed (`&str` / `Cow`) fields, which the A2A types are not.

Use these in performance-sensitive paths where serialization/deserialization is called repeatedly (e.g., per-event SSE frame building, transport payload encoding).

### `Vec<u8>` vs `Bytes` in retry loops

Cloning a `Vec<u8>` inside a retry loop allocates a full heap copy each time. Use `bytes::Bytes` (reference-counted) so that `.clone()` is just an atomic reference count increment. This matters for push notification delivery where large payloads may be retried 3+ times.

### Serialize once before retry loops

Deep-cloning a `serde_json::Value` tree on every retry attempt is expensive. Serialize the params to bytes once before the retry loop, then deserialize from bytes for each attempt. Deserialization from bytes is cheaper than a recursive deep-clone of the `Value` tree.

### Agent card fetch responses need a body size limit

Fetching an agent card from an untrusted URL without a body size limit allows a malicious endpoint to send an arbitrarily large response, causing OOM. Always enforce a body size limit (e.g., 2 MiB) on agent card fetch responses.

## Graceful Shutdown Pitfalls

### Bound executor cleanup with a timeout

`executor.on_shutdown().await` can hang indefinitely if the executor's cleanup routine blocks. Always wrap with `tokio::time::timeout()`. `RequestHandler::shutdown()` bounds the hook at 10 seconds; `shutdown_with_timeout(duration)` takes one budget for the whole call — draining the event queues, then the cleanup hook with whatever the drain left — so size `duration` against your platform's grace period, not per phase.

### Shutdown polling interval matters

A 50ms polling interval in the shutdown loop wastes up to 50ms per cycle. Using a 10ms interval with deadline-aware sleep (sleeping the minimum of the remaining deadline and the interval) gives faster shutdown response without busy-waiting.

## gRPC Pitfalls

### The `grpc` and `proto` features need no system `protoc`

Their build scripts compile `a2a.proto` with a vendored `protoc` (`protoc-bin-vendored`; Linux and macOS on x86_64 and aarch64, Windows x86_64), so `--all-features`, `cargo package` and `cargo publish` work on a stock CI runner. Set `PROTOC` only to use your own binary, or on a platform the vendored ones do not cover; `build.rs` leaves a `PROTOC` you set alone.

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

### Collection iteration order and determinism

The `InMemoryTaskStore` uses a `HashMap<TaskId, TaskEntry>` for O(1) get/save, plus a `BTreeMap` keyed by a monotonic per-write sequence and a `HashMap<String, BTreeMap<…>>` context index. `list` walks them in reverse, so tasks come back most-recently-updated first (spec §3.1.4) in O(log n + page\_size) with a collision-free cursor, without the O(n log n) per-call sort that previously caused 20-70× regressions at scale. Other in-memory stores (e.g., `InMemoryPushConfigStore`) also use a `HashMap`, whose iteration order is arbitrary. Always sort results before applying pagination or returning them to clients.

### Percent-encoded path traversal

Checking `path.contains("..")` is insufficient. Attackers can use `%2E%2E` (or mixed-case `%2e%2E`) to bypass the check.

**Solution:** The REST dispatcher percent-decodes the path, repeatedly (up to eight passes), *before* checking for a dot segment — `.` or `..` as a whole path segment, with `\` read as a separator — so `%2E%2E`, `%252E%252E` and `..\` are refused while an id such as `v1..prod` is served. A path still changing after eight passes is refused.

### A cargo-mutants baseline failure looks exactly like a clean file

`cargo mutants` writes `caught.txt`, `missed.txt` and friends whether or not it
tested anything. When the *baseline* build or test run fails — the unmutated
tree — it tests no mutants at all and every one of those files is empty. A
summary that reports counts alone prints `caught=0 missed=0`, which is
indistinguishable from a file with nothing left to kill.

Only the exit code separates them: `0` all caught, `2` surviving mutants, `3`
timeout, `4` baseline failed. Two baseline failures in one afternoon on this
repository were a full disk and a stopped Postgres — neither had anything to do
with the code under test, and both would have been recorded as a perfect score
by a reader who trusted the numbers.

**Solution:** always report the exit code next to the counts, and treat an
empty denominator as "did not run" rather than "passed". `mutants.yml` does
this at the aggregation step for the same reason.

### A mutation TIMEOUT is a third outcome, and the score hides it

`caught` and `missed` are not the whole result. A mutant that makes a test
*hang* rather than fail is reported `TIMEOUT`, and the usual score —
`caught / (caught + missed)`, which is what `mutants.yml` computes — puts it in
neither term. It is not a mutant the score counts against; it is a mutant the
score does not describe. `0 missed` and `0 timeouts` are different statements
and this repository has published the first as if it were the second.

Hangs are also the failure mode that reads worst in CI: the job wedges with no
assertion named, and under nextest's run-wide fail-fast a suite cannot exit
while a hung test is still running, so tests that *do* catch the mutant fail in
milliseconds and never get to report.

**Solution:** bound it at the harness, not test by test. `.config/nextest.toml`
kills any single test at 45 seconds, so the hang becomes a named failure and the
mutant is scored. Read the timeout column on every sweep, and treat a non-zero
one as unfinished work.

### The config file a tool ignores is worse than no config file

cargo-mutants 27.1.0 discovers `.cargo/mutants.toml`. This repository's
`mutants.toml` is at the workspace root, so none of it — file globs, exclusions,
timeout multiplier, cap — has ever applied to a run. It was believed to for
months, and three of its keys had individually been written off as "silently
ignored" without anyone asking why three unrelated keys would all misbehave.

Two checks tell you which situation you are in, and neither takes a full run:
compare `--list` output with and without `--no-config` (identical output means
the config is not being read, because disabling something inert changes
nothing), and pass the file explicitly with `--config <file>` (a config that is
never discovered can carry a fatal parse error indefinitely — this one does).

**Solution:** treat "is this configuration loaded?" as a question with an
answer, separate from "is this configuration right?" — the same distinction as
"can this gate fail?" versus "is this gate pointed at what it claims to cover?"
Assert it if the configuration matters.

### Matching CI's commands is not the same as matching CI

`scripts/preflight.sh` reads its gate list from `.github/workflows/ci.yml` so
the two cannot drift. Copying the commands alone still is not parity, because
this repository's CI also sets an `env:` block those commands run under:
`RUSTFLAGS: "-D warnings"` (a warning CI denies would pass locally without it)
and `CARGO_PROFILE_DEV_DEBUG: 0` (without which the all-features link can die
with `ld terminated with signal 7 [Bus error]` on a constrained machine — a
failure that reads as a broken test but is not one).

**Solution:** `preflight.sh` exports CI's top-level `env:` block, parsed from
the same file as the gates. Be aware that `RUSTFLAGS` and the profile setting
are part of cargo's fingerprint, so adopting them rebuilds a workspace that was
previously built without them.

## Scale & Durability Pitfalls

### SSE parser queue can grow unbounded (fixed)

A malicious SSE stream sending many oversized events could fill the parser's internal frame queue without bound. The fix caps the queue at 4096 frames (configurable via `with_max_queued_frames`), dropping the oldest when full.

### Retry backoff can overflow to infinity (fixed)

`cap_backoff()` computed `Duration::from_secs_f64(current * multiplier)`. With extreme multipliers, this produces `f64::INFINITY` which panics in `from_secs_f64`. The fix checks for non-finite results and clamps to `max_backoff`.

### Background event processor can miss fast executor events (fixed)

In streaming mode, the background event processor previously subscribed to the broadcast channel *after* the executor started. For very fast executors, events could be written before the subscription was active, meaning the task store was not updated. The fix introduces a dedicated persistence channel (mpsc) that is independent of the broadcast channel used for SSE delivery, so a fast executor cannot outrun the subscription; a persistence channel still full after the queue's write timeout fails the write with an error rather than dropping the event. See Bug #38 in [dogfooding-bugs](../deployment/dogfooding-bugs.md).

### Event queue writer silently swallows serialization errors (fixed)

`InMemoryQueueWriter::write()` used `unwrap_or(0)` when measuring event size via `CountingWriter`. If serialization failed, the size was reported as 0 and the event was sent through the channel without error. The fix propagates the serialization error via `?` operator, returning `A2aError::internal("event serialization failed: ...")`.

### Capacity eviction cannot evict non-terminal tasks (fixed)

`InMemoryTaskStore` capacity eviction only targeted terminal tasks. If the store was full of non-terminal (Working/Submitted) tasks, eviction found nothing to remove and silently gave up — leaving the store permanently over capacity. The fix adds a fallback path that evicts the oldest non-terminal tasks when there aren't enough terminal tasks.

### Lagged event queue reader drops diagnostic count (fixed)

The broadcast channel `Lagged(n)` error provides the exact count of dropped events, but the reader used `_n` (underscore prefix), discarding the count. The warning message said "skipping missed events" without saying how many. The fix exposes the count in the log: `"event queue reader lagged, {n} events dropped"`. The reader no longer skips ahead either: it returns `A2aError::stream_lagged(n)`, and the stream ends with that error. Since the event-log catch-up (spec §3.5.2), a stream reads the dropped events from the task's log first, and ends with that error only when the log cannot supply them all.

## Client Pitfalls

### `truncate_body` panics on multi-byte UTF-8 (fixed)

The error body truncation helper sliced at a fixed byte offset (`body[..512]`). For non-ASCII responses (common with international error messages), the offset could fall inside a multi-byte UTF-8 character, causing a panic. The fix uses `is_char_boundary()` to find the nearest safe truncation point.

### SSE parser `line_buf` can grow without bound (fixed)

The SSE parser's internal line buffer grew without limit for lines without newlines. A malicious server sending a single very long line could cause OOM. It was first capped at 2× `max_event_size`, past which bytes were dropped silently; a line that outgrows `max_event_size` is now refused as soon as it does, with `SseParseError::EventTooLarge`, and none of the rest of it is buffered.

### REST path parameters are not percent-encoded (fixed)

Path parameters (task IDs, config IDs) were interpolated into REST URLs without encoding. An ID containing `/` or `..` could cause path traversal. The fix percent-encodes all path parameters using the same encoder as query parameters.

### `GetExtendedAgentCard` discards interceptor params (fixed)

The `get_extended_agent_card` method created an empty params object after running interceptors, discarding any modifications made by `before` interceptors. The fix forwards `req.params` instead.

## Validation Pitfalls

### Empty/whitespace-only IDs must be rejected

`TaskId` and `ContextId` should not accept empty or whitespace-only strings. Build them with `TaskId::try_new` / `ContextId::try_new`, which reject such values; `new`, `From<String>` and `From<&str>` accept them for backward compatibility.

### A file part needs a source

v1.0 carries a file as `Part::raw(bytes)` or `Part::url(uri)`. The v0.3 `FileContent` type is deprecated, and `Part::file(FileContent)` turns one with neither `bytes` nor `uri` into an empty raw part instead of refusing it, so call `FileContent::validate()` first if you still use it.

### Push notification URLs need validation

`TaskPushNotificationConfig` URLs should be validated for correct format. A `validate()` method on the config struct catches malformed URLs before they reach the push sender.

### Timestamps should be RFC 3339

`TaskStatus` timestamps should conform to RFC 3339. `has_valid_timestamp()` is only a coarse check (at least 19 characters and a `T` separator); `parse_iso8601_to_unix_millis(ts).is_some()` is the real test, and is what the server applies to `statusTimestampAfter`.

### Artifact parts must not be empty

`Artifact` must contain at least one `Part` per the A2A spec, and an
appended chunk (`append: true`) is an `Artifact` too: a2a.proto makes no
exception for it. Call `artifact.validate()` before emitting artifacts from
your executor. The event queue refuses an artifact update with no parts
(`InvalidParams` to the executor's `write`), and event processing drops one
that reaches it another way. To mark the end of a streamed artifact, set
`lastChunk: true` on the chunk that carries its last part, rather than
sending an empty chunk after it; until 0.15 an empty appended chunk was
accepted.

### Artifact append must merge metadata

When `TaskArtifactUpdateEvent` has `append=true`, metadata from the new event
must be merged into the existing artifact's metadata (new keys override
existing). Simply pushing a new artifact entry loses the append semantics.

### TaskListResponse fields are always present

`next_page_token`, `page_size`, and `total_size` are required per the proto
spec (`ListTasksResponse`) — the SDK's `TaskListResponse` holds them as
`String` and `u32`, not `Option`, and a reader of a peer's response treats an
absent one as empty/zero. An empty `next_page_token` means no more pages.
Custom `TaskStore` implementations must populate all three.

### SendMessage to terminal tasks must be rejected

A `SendMessage` targeting a task in a terminal state (`Completed`, `Failed`,
`Canceled`, `Rejected`) via explicit `taskId` must return `UnsupportedOperation`,
not silently create a new task. The server enforces this at the handler level.

### `CachingCardResolver` must not silently produce empty URLs

If `CachingCardResolver::new()` or `with_path()` receives an invalid base URL, silently producing an empty URL leads to confusing errors later. These constructors should return `ClientResult<Self>` so callers can handle the error immediately.

## Database Pitfalls

### Always use parameterized queries for user-controlled values

Using `format!` to interpolate caller-supplied values into SQL allows SQL injection; bind them. sqlx 0.9 enforces the rule on the type level: a statement built at run time must be wrapped in `sqlx::AssertSqlSafe`, which is the author vouching that only static text and placeholder positions went in. The one value the SDK's own stores interpolate is the page-size `LIMIT`, an integer clamped by `max_page_size`; every caller-supplied value is bound.

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
