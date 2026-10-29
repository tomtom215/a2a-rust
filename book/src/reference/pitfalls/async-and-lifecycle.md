<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Async, cancellation and shutdown

Part of [Pitfalls & Lessons Learned](../pitfalls.md).

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

## Cancel / State Machine Pitfalls

### Check terminal state before emitting cancel

If an executor completes between the handler's cancel check and the cancel signal arrival, unconditionally emitting `TaskState::Canceled` causes an invalid `Completed → Canceled` state transition. Guard cancel emission with `cancellation_token.is_cancelled()` or a terminal-state check.

### Re-check cancellation between multi-phase work

If an executor performs multiple phases (e.g., two artifact emissions), check cancellation between phases. Otherwise, cancellation between phases is delayed until the next natural check point.

## Server Accept Loop Pitfalls

### `break` vs `continue` on transient accept errors

Using `Err(_) => break` in a TCP accept loop kills the entire server on a single transient error (e.g., `EMFILE` when the file descriptor limit is reached). Use `Err(_) => continue` to skip the bad connection and keep serving. Log the error for observability but do not let it take down the server.

## Graceful Shutdown Pitfalls

### Bound executor cleanup with a timeout

`executor.on_shutdown().await` can hang indefinitely if the executor's cleanup routine blocks. Always wrap with `tokio::time::timeout()`. `RequestHandler::shutdown()` bounds the hook at 10 seconds; `shutdown_with_timeout(duration)` takes one budget for the whole call — draining the event queues, then the cleanup hook with whatever the drain left — so size `duration` against your platform's grace period, not per phase.

### Shutdown polling interval matters

A 50ms polling interval in the shutdown loop wastes up to 50ms per cycle. Using a 10ms interval with deadline-aware sleep (sleeping the minimum of the remaining deadline and the interval) gives faster shutdown response without busy-waiting.

## Error Handling Pitfalls

### Never silently swallow store lookup errors

`find_task_by_context` (or similar store lookups) should propagate errors via `?` rather than using patterns like `.ok()` or `unwrap_or(None)` that silently convert store failures into "not found" results.
