# ADR 0008: Object-Safe `AgentExecutor` Trait Shape

**Date:** 2026-04-15
**Status:** Accepted
**Author:** Tom F.

---

## Context

`AgentExecutor` is the primary extension point users implement to plug
business logic into the A2A server. It is called by `RequestHandler` for
every incoming `SendMessage` / `SendStreamingMessage` request, and again for
`tasks/cancel`. Every concrete server type downstream of the executor —
`RequestHandler`, `JsonRpcDispatcher`, `RestDispatcher`, the Axum integration
— stores the executor and must remain usable as a non-generic type.

The trait currently looks like this:

```rust
pub trait AgentExecutor: Send + Sync + 'static {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { /* default */ }

    fn on_shutdown<'a>(&'a self)
        -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> { /* default */ }
}
```

Every user implementation therefore looks like:

```rust
impl AgentExecutor for MyAgent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { /* ... */ Ok(()) })
    }
}
```

The `Pin<Box<dyn Future + Send + 'a>>` return type is visible in every
example in the repo. The obvious question a reviewer asks when opening
`executor.rs` is: *why not `async fn execute(...)`?*

This ADR records the reason and the alternatives we rejected.

## Decision

Keep `AgentExecutor` as a manual `Pin<Box<dyn Future + Send + 'a>>` trait.
Compensate for the ergonomic tax by shipping two helpers in
`a2a_protocol_server::executor_helpers`:

1. `boxed_future(async move { ... })` — a thin typed wrapper that replaces
   the ceremonial `Box::pin(async move { ... })` at the call site.
2. `agent_executor!(Type, |ctx, queue| async { ... })` — a declarative macro
   that expands to the full trait impl so simple executors are a one-liner.

Both helpers are used extensively in the repo's examples and tests and keep
the 95% case free of the trait's object-safety boilerplate.

## Rationale

### Why object safety is load-bearing

`RequestHandler` holds the executor as `Arc<dyn AgentExecutor>`, not as a
generic parameter:

```rust
pub struct RequestHandler {
    executor: Arc<dyn AgentExecutor>,
    // ... stores, queues, limits, ...
}
```

This is the single most important shape decision in the server crate,
because it removes a generic parameter from *everything* downstream:

- `RequestHandler` itself
- `JsonRpcDispatcher`, `RestDispatcher`, `GrpcDispatcher`
- The `A2aRouter` Axum integration
- The `RequestHandlerBuilder` fluent API
- Every test fixture that constructs a handler

With a generic `E: AgentExecutor`, `RequestHandler<E>` would leak that type
parameter up through the dispatcher trait, the Axum layer, and every
doc-test signature. Users would have to spell out their executor type in
every place they stored a handler, and `Box<dyn Dispatcher>` and
`Arc<RequestHandler>` storage patterns would become impossible without
a separate erasure step.

The cost of keeping `AgentExecutor` object-safe is one `Box::pin` per call
(amortized over network RTT, effectively free). The cost of *not* keeping
it object-safe is a viral generic parameter through the entire public API.
The tradeoff is not close.

### Why `async fn execute(...)` does not work today

Rust stabilised async-fn-in-trait (AFIT) in 1.75, but AFIT traits are **not
object-safe** on stable Rust at the time of writing. An `async fn execute`
produces an anonymous, per-impl future type, and there is no mechanism on
stable to erase that to a single `dyn`-compatible shape without either:

1. `#[trait_variant::make]` (crate-level macro, requires
   `trait_variant` dep, still produces a separate non-object-safe
   `LocalAgentExecutor` trait that users have to know about).
2. `dyn*` types (unstable / design still evolving).
3. Hand-writing the `Pin<Box<dyn Future>>` shape the compiler would produce
   anyway.

We chose (3): spell the shape out in the trait so users see exactly what
the runtime cost is, no nightly features, no third-party macro crate in the
public API, and no leaking of "local vs. non-local" variants.

When `dyn AsyncFn` / `dyn*` stabilise in a form that is object-safe and
doesn't require a separate trait, we'll revisit.

### Why we didn't use `async-trait`

`async-trait` (the crate) produces exactly the same `Pin<Box<dyn Future +
Send>>` shape as the manual form, but hides it behind an attribute macro.
It was rejected for two reasons:

1. **A public trait should not silently allocate.** `#[async_trait]` emits
   `Box::pin(async move { ... })` on every call. This is the same cost
   we're paying now, but it's invisible in the trait definition — a user
   can't tell from reading the trait that every `execute` call is a heap
   allocation. The manual form makes the cost honest.

2. **It adds a dependency to every user of the library.** `async-trait`
   re-exports the `Pin<Box<dyn Future>>` type in its own proc-macro crate
   namespace, and downstream crates that implement the trait have to pull
   in `async-trait` as a dep. For a protocol SDK whose explicit philosophy
   is minimal mandatory dependencies (see ADR 0002), this is a
   non-starter.

### Why we didn't use AFIT + `BoxedAgentExecutor` erasure wrapper

The pattern where you define a public `trait AgentExecutor { async fn
execute(...) }` and an internal `struct BoxedAgentExecutor<E>(E)` that
implements an object-safe `DynAgentExecutor` trait would let users write
`async fn` while keeping the handler non-generic. Rejected because it
doubles the trait surface (every method lives in two places), and the
wrapper leaks into error messages and docs. Ergonomic win for users who
implement the trait, usability loss for users who *read* the trait trying
to understand the server.

### Ergonomic mitigation

The ceremonial boilerplate is real; `Box::pin(async move { ... })` wrapping
is noise. So we ship two helpers:

```rust
// boxed_future — typed wrapper over Box::pin, removes the type annotation
fn execute<'a>(
    &'a self,
    ctx: &'a RequestContext,
    queue: &'a dyn EventQueueWriter,
) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
    boxed_future(async move {
        // your logic here
        Ok(())
    })
}

// agent_executor! — declarative macro, full impl in a single line
agent_executor!(EchoAgent, |ctx, queue| async move {
    queue.write(/* ... */).await?;
    Ok(())
});
```

The macro expands to the full trait impl including the default `cancel`
and `on_shutdown` methods. In `crates/a2a-protocol-server/src/handler/lifecycle/`
every test fixture uses the macro form — `DummyExecutor`,
`CancelableExecutor`, etc. For production executors with more than trivial
logic, `boxed_future` is the idiomatic choice: it preserves the imperative
method-body shape while eliminating the `Box::pin` type annotation.

## Consequences

### Positive

- `RequestHandler`, every dispatcher, the Axum integration, and the
  builder API are non-generic. One storage pattern (`Arc<dyn
  AgentExecutor>`) works everywhere.
- No nightly features, no `async-trait` dep, no `trait_variant` dep — the
  server crate has zero trait-erasure machinery in its public API.
- The cost of async-in-trait (one heap allocation per call) is honest and
  visible in the trait definition. Users can reason about it.
- The `agent_executor!` macro gives trivial executors the same ergonomics
  as `async fn` without introducing a second trait.

### Negative

- Manual impls have a verbose signature. The first impression on a reader
  who doesn't know why is "why not `async fn`?" — hence this ADR.
- Users who don't find the macro / helper (e.g. because they started from
  the trait rustdoc) may end up writing `Box::pin(async move { ... })` by
  hand. We mitigate this by linking `boxed_future` and `agent_executor!`
  from the trait-level rustdoc.
- Every executor allocates once per call. This is measured in
  `benches/src/executor.rs`; at protocol RTT scales it is not visible.

## Alternatives Considered

| Alternative | Rejection reason |
|---|---|
| `async fn execute(...)` (AFIT) | Not object-safe on stable — forces `RequestHandler<E>` generic parameter, which then leaks through every dispatcher, the Axum layer, and the builder. |
| `#[async_trait]` crate | Produces the same `Pin<Box<dyn Future>>` shape we have now but hides it — dishonest about heap allocation, and pulls `async-trait` into every downstream user's dep graph, which contradicts ADR 0002. |
| `#[trait_variant::make]` | Produces two traits (object-safe `AgentExecutor`, non-object-safe `LocalAgentExecutor`); users have to understand the split. Adds a proc-macro dep to the public API. |
| AFIT + hand-rolled `BoxedAgentExecutor<E>` erasure wrapper | Doubles the trait surface; the wrapper leaks into docs and error messages. Net ergonomic loss for readers. |
| `dyn*` / object-safe async fn (unstable) | Not stable. Revisit when it is. |

## Revisit Trigger

Reopen this ADR when one of the following becomes true on stable Rust:

1. `dyn AsyncFnTrait` or `dyn*` is stabilised in an object-safe shape that
   doesn't require a second trait.
2. A concrete benchmark shows the per-call `Box::pin` allocation is a
   measurable fraction of server request latency under any realistic
   workload. (The current benches in `benches/src/executor.rs` show it is
   deep in the noise floor relative to network RTT and JSON encoding.)

Until then, the current shape is the right tradeoff.
