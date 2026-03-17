<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# LESSONS — Pitfalls Catalog

Running list of non-obvious problems encountered during development.
Each entry is added at the time the issue is first hit.

---

## Serde Pitfalls

### Untagged enums hide inner errors

`#[serde(untagged)]` on `SendMessageResponse` and `StreamResponse` swallows
the real deserialization error and replaces it with a generic "data did not
match any variant" message. When debugging, temporarily switch to an externally
tagged enum to see the real error.

### `#[serde(default)]` vs `Option<T>`

Using `#[serde(default)]` on a `Vec<T>` field means the field is present but
empty when omitted from JSON. Using `Option<Vec<T>>` means it is absent
(`None`). The A2A spec distinguishes between "omitted" and "empty array" for
fields like `history` and `artifacts`, so `Option<Vec<T>>` is correct.

### `#[non_exhaustive]` on enums breaks downstream matches

Adding `#[non_exhaustive]` to `TaskState`, `ErrorCode`, etc. forces
downstream crates to include a wildcard arm. This is intentional for forward
compatibility but must be documented clearly.

## Hyper 1.x API Pitfalls

### `Body` is not `Clone`

Hyper 1.x `Incoming` body is consumed on read. You cannot read the body twice.
Use `http_body_util::BodyExt::collect()` to buffer it into `Bytes` first,
then work from the buffered copy.

### Response builder panics on invalid header values

`hyper::Response::builder().header(k, v)` silently stores the error and panics
when `.body()` is called. Always use `unwrap_or_else` with a fallback response,
or validate header values with `.parse::<HeaderValue>()` first.

### `size_hint()` upper bound may be `None`

`hyper::body::Body::size_hint().upper()` returns `None` when Content-Length is
absent. Always check for `None` before comparing against the body size limit.

## SSE Streaming Pitfalls

### SSE parser must handle partial lines

SSE events may arrive split across TCP frames. The parser must buffer partial
lines and only process complete `\n`-terminated lines. The `EventBuffer` in
`a2a-protocol-client` handles this, but naive `lines()` iterators will break.

### Memory limit on buffered SSE data

A malicious server can send an infinite SSE event (no `\n\n` terminator).
The client parser enforces a 16 MiB cap on buffered event data to prevent OOM.

## Push Notification Pitfalls

### SSRF via webhook URLs

Push notification `url` fields can point to internal services (e.g.
`http://169.254.169.254/`). The `HttpPushSender` resolves the URL and rejects
private/loopback IP addresses before sending. This check must happen after DNS
resolution, not on the URL string alone.

### Header injection via credentials

Push notification `credentials` can contain newlines that inject additional
HTTP headers. The push sender validates that credential values contain no
`\r` or `\n` characters.

## Async / Tokio Pitfalls

### Object-safe async traits need `Pin<Box<dyn Future>>`

Rust does not yet support `async fn` in traits that are used as `dyn Trait`.
The `TaskStore` and `AgentExecutor` traits use explicit
`Pin<Box<dyn Future<Output = ...> + Send + 'a>>` return types. Implementors
use `Box::pin(async move { ... })`.

### Cancellation token cleanup

`CancellationToken`s that are never cancelled accumulate in the token map.
The handler cleans up already-cancelled tokens before inserting new ones, and
enforces a hard cap of 10,000 tokens.

### `std::sync::RwLock` poisoning — fail-fast vs silent ignore

When a thread panics while holding a `std::sync::RwLock`, the lock becomes
"poisoned." Using `.ok()?` silently returns `None`, hiding the underlying bug.
Using `.expect("poisoned")` panics immediately, surfacing the problem. a2a-rust
uses fail-fast. If you see a "lock poisoned" panic, the root cause is a prior
panic in another thread — fix that panic first.

### Rate limiter atomics under read lock — CAS loop required

When multiple threads share a rate limiter bucket under a read lock, non-atomic
multi-step operations (load window → check → store count) create TOCTOU races.
Two threads can both see an old window and both reset the counter. Use
`compare_exchange` (CAS) to atomically swap the window number so only one thread
wins the reset.

### Amortized eviction vs test expectations

Running O(n) eviction on every `save()` call is expensive. The task store
amortizes eviction to every 64 writes. Tests that depend on eviction must
call `run_eviction()` explicitly or account for the amortization interval.

## Workspace / Cargo Pitfalls

### `cargo-fuzz` needs its own workspace

The `fuzz/` directory contains its own `Cargo.toml` with `[workspace]` to
prevent cargo-fuzz from conflicting with the main workspace. The fuzz crate
references `a2a-protocol-types` via a relative path dependency.

### Feature unification across workspace

Enabling a feature in one crate (e.g. `signing` in `a2a-protocol-types`) enables it
for all crates in the workspace during `cargo test --workspace`. Use
`--no-default-features` or per-crate test commands when testing feature gates.

## Testing Pitfalls

### HashMap iteration order is non-deterministic

The `InMemoryTaskStore` uses a `HashMap` internally. `list()` must sort
results by task ID before applying pagination; otherwise, page boundaries
shift between runs and tests become flaky.

### Percent-encoded path traversal

Checking `path.contains("..")` is insufficient. Attackers can use `%2E%2E`
to bypass the check. The REST dispatcher percent-decodes the path before
checking for `..` sequences.

## Server Accept Loop Pitfalls

### `break` vs `continue` on transient accept errors

Using `Err(_) => break` in a TCP accept loop kills the entire server on a
single transient error (e.g., `EMFILE` when the file descriptor limit is
reached). Use `Err(_) => continue` to skip the bad connection and keep serving.
Log the error for observability but do not let it take down the server.

## Push Config Store Pitfalls

### Per-resource limits are not enough — add global limits

A per-task push config limit (e.g., 100 configs per task) does not prevent an
attacker from creating millions of tasks with 100 configs each. Always pair
per-resource limits with a global cap (e.g., `max_total_configs = 100_000`).
The global check must run before the per-resource check in the write path.

### Webhook URL scheme validation

Checking for private IPs and hostnames in webhook URLs is insufficient. URLs
like `ftp://evil.com/hook` or `file:///etc/passwd` bypass IP-based SSRF checks
entirely. Always validate that the URL scheme is `http` or `https` before
performing any further validation.

## Performance Pitfalls

### `Vec<u8>` vs `Bytes` in retry loops

Cloning a `Vec<u8>` inside a retry loop allocates a full heap copy each time.
Use `bytes::Bytes` (reference-counted) so that `.clone()` is just an atomic
reference count increment. This matters for push notification delivery where
large payloads may be retried 3+ times.

## Graceful Shutdown Pitfalls

### Bound executor cleanup with a timeout

`executor.on_shutdown().await` can hang indefinitely if the executor's cleanup
routine blocks. Always wrap with `tokio::time::timeout()` — the default is
10 seconds. Users can override via `shutdown_with_timeout(duration)`.

## gRPC Pitfalls

### Map all tonic status codes, not just the common ones

Only mapping 4 gRPC status codes to A2A error codes loses semantic information
for `Unauthenticated`, `PermissionDenied`, `ResourceExhausted`, etc. Map all
relevant codes explicitly so clients can distinguish between error categories.

### Feature-gated code paths need their own dogfood pass

The gRPC path behind `#[cfg(feature = "grpc")]` had the exact same placeholder
URL bug that was already fixed for HTTP (Bug #12 → Bug #18). Feature-gated code
is easy to miss during reviews. Always re-verify known bug patterns across all
feature gates.

## Cancel / State Machine Pitfalls

### Check terminal state before emitting cancel

If an executor completes between the handler's cancel check and the cancel
signal arrival, unconditionally emitting `TaskState::Canceled` causes an invalid
`Completed → Canceled` state transition. Guard cancel emission with
`cancellation_token.is_cancelled()` or a terminal-state check.

### Re-check cancellation between multi-phase work

If an executor performs multiple phases (e.g., two artifact emissions), check
cancellation between phases. Otherwise, cancellation between phases is delayed
until the next natural check point.
