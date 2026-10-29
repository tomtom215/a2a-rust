<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# HTTP, streaming and transports

Part of [Pitfalls & Lessons Learned](../pitfalls.md).

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

## gRPC Pitfalls

### The `grpc` and `proto` features need no system `protoc`

Their build scripts compile `a2a.proto` with a vendored `protoc` (`protoc-bin-vendored`; Linux and macOS on x86_64 and aarch64, Windows x86_64), so `--all-features`, `cargo package` and `cargo publish` work on a stock CI runner. Set `PROTOC` only to use your own binary, or on a platform the vendored ones do not cover; `build.rs` leaves a `PROTOC` you set alone.

### Map all tonic status codes, not just the common ones

Only mapping 4 gRPC status codes to A2A error codes loses semantic information for `Unauthenticated`, `PermissionDenied`, `ResourceExhausted`, etc. Map all relevant codes explicitly so clients can distinguish between error categories.

### Feature-gated code paths need their own dogfood pass

The gRPC path behind `#[cfg(feature = "grpc")]` had the exact same placeholder URL bug that was already fixed for HTTP (Bug #12 → Bug #18). Feature-gated code is easy to miss during reviews. Always re-verify known bug patterns across all feature gates.

## WebSocket Server Pitfalls

### Limit concurrent tasks per connection

Without a concurrency limit, a single WebSocket client can spawn unbounded tasks on the server by sending many requests in rapid succession. A per-connection `Semaphore` (e.g., 64 permits) bounds resource usage per client.

### Limit incoming message size

Without a message size check, a client can send arbitrarily large WebSocket frames, causing OOM on the server. Enforce a size limit (e.g., 4 MiB) on incoming WebSocket messages and reject oversized frames with a close code.

## REST Dispatch Pitfalls

### Tenant must be passed through all handler calls

When extracting a tenant from the URL path (e.g., `/tenants/{tenant}/tasks/{taskId}`), the extracted tenant must be forwarded to every handler method call. Missing the tenant on any method causes requests to be processed in the wrong tenant context or with no tenant at all.
