# ADR 0003: Async Runtime Strategy

**Date:** 2026-03-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

Rust async code is runtime-agnostic at the language level, but practical HTTP libraries are not. `hyper` 1.x uses `tokio` internally for its connection management. Making the SDK runtime-agnostic would require wrapping every I/O call behind an abstraction layer — a significant complexity cost with negligible real-world benefit, since >95% of Rust async production code runs on tokio.

## Decision

**Tokio is the mandatory async runtime.** It is listed as a mandatory dep (not optional, not feature-gated for the I/O crates).

Tokio features are pinned to the minimum needed:

| Crate | Tokio features |
|---|---|
| `a2a-protocol-types` | none (no async) |
| `a2a-protocol-client` | `rt, net, io-util, sync, time, macros` |
| `a2a-protocol-server` | `rt, net, io-util, sync, time, macros, signal` |

`rt-multi-thread` is **not** forced. Users who run single-threaded runtimes (`#[tokio::main(flavor = "current_thread")]`) are supported without modification.

### Object-Safe Async Traits via `Pin<Box<dyn Future>>`

Key traits (`AgentExecutor`, `TaskStore`, `PushConfigStore`, `PushSender`) use
`Pin<Box<dyn Future<Output = T> + Send + 'a>>` return types instead of native
`async fn` in traits. This ensures **object safety** — these traits can be stored
as `Arc<dyn AgentExecutor>` and shared across threads without generic parameters
on `RequestHandler`, `JsonRpcDispatcher`, or `RestDispatcher`:

```rust
pub trait AgentExecutor: Send + Sync + 'static {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}
```

The `agent_executor!` macro and `boxed_future` helper eliminate the boilerplate
for implementors. The `Send + Sync + 'static` bound ensures futures can be
spawned on tokio's multi-thread scheduler.

### No Blocking I/O in Async Contexts

The `tokio::task::spawn_blocking` primitive is used for any CPU-bound work that cannot be expressed as async (e.g., blocking crypto operations, if ever needed). No `std::thread::sleep` in async contexts — use `tokio::time::sleep`.

### `#[tokio::test]` for All Tests

All async tests use the `#[tokio::test]` macro. No manual `tokio::runtime::Runtime::new()` or `block_on` in test code.

## Consequences

### Positive

- No runtime abstraction layer to maintain.
- `hyper` integration is direct and correct.
- Object-safe async traits via `Pin<Box<dyn Future>>` enable dynamic dispatch without generics.
- All spawned tasks are `Send`, enabling multi-threaded runtimes.

### Negative

- Users on `async-std` or `smol` runtimes cannot use this SDK as-is.
- `tokio` appears in `a2a-protocol-client` and `a2a-protocol-server`'s public dep trees.

## Alternatives Considered

### Runtime-Agnostic via `async-trait` Crate

The `async-trait` proc-macro desugars `async fn` in traits to `Pin<Box<dyn Future + Send>>`. Rejected because:
- Adds an extra proc-macro dep.
- The same boxing is achieved manually with `Box::pin(async move { ... })` or the `boxed_future` helper, without the dep.

### Runtime-Agnostic via `futures` Traits

Abstract over `AsyncRead`/`AsyncWrite` using `futures::io`. Rejected because:
- `futures` brings 12+ sub-crates.
- `hyper` and `tokio` use `tokio::io` traits, not `futures::io` — adapter layer required.
- No meaningful portability gain since `hyper` mandates tokio anyway.

### Use `async-std`

Rejected: `hyper` 1.x does not officially support `async-std`. Community wrappers exist but are not production-hardened.
