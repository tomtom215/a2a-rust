# Pitfalls & Lessons Learned

A catalog of non-obvious problems encountered during development. Each entry documents a real issue and its solution.

## Serde Pitfalls

### Untagged enums hide inner errors

`#[serde(untagged)]` on `SendMessageResponse` and `StreamResponse` swallows the real deserialization error and replaces it with a generic "data did not match any variant" message.

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

SSE events may arrive split across TCP frames. The parser must buffer partial lines and only process complete `\n`-terminated lines. The `EventBuffer` in `a2a-protocol-client` handles this correctly, but naive `lines()` iterators will break on partial frames.

### Memory limit on buffered SSE data

A malicious server can send an infinite SSE event (no `\n\n` terminator). The client parser enforces a 16 MiB cap on buffered event data to prevent OOM.

## Push Notification Pitfalls

### SSRF via webhook URLs

Push notification `url` fields can point to internal services (e.g., `http://169.254.169.254/` — the cloud metadata endpoint).

**Solution:** The `HttpPushSender` resolves the URL and rejects private/loopback IP addresses *after DNS resolution*, not on the URL string alone. This prevents DNS rebinding attacks.

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

## Next Steps

- **[Architecture Decision Records](./adrs.md)** — Design decisions behind these choices
- **[Configuration Reference](./configuration.md)** — All tunable parameters
