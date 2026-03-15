<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Roadmap — Beyond-Spec Extensions

This document tracks planned enhancements that go beyond the A2A v1.0.0 spec.
Each section describes the extension, its motivation, and recommended
implementation approach.

---

## 7.1 Request ID Propagation

**Status:** Planned

**Motivation:** Correlating logs across client and server for distributed
tracing. Without request IDs, debugging production issues requires manual
timestamp correlation.

**Approach:**

- Generate a UUID v4 request ID in the client for every RPC call.
- Send it as an `X-Request-Id` HTTP header.
- The server extracts the header (or generates one if absent) and includes it
  in all log lines for that request via a `tracing` span field.
- The interceptor infrastructure already supports header injection
  (`ClientRequest::extra_headers`), so a `RequestIdInterceptor` can be
  implemented as a `CallInterceptor` without any framework changes.

```rust
// Example usage:
// client_builder.with_interceptor(RequestIdInterceptor::new())
```

---

## 7.2 Metrics Hooks

**Status:** Planned

**Motivation:** Production deployments need request counts, latency histograms,
error rates, and queue depths. The framework should expose integration points
without mandating a specific metrics library.

**Approach:**

- Define a `Metrics` trait with methods like `on_request`, `on_response`,
  `on_error`, `on_queue_depth_change`.
- `RequestHandlerBuilder::with_metrics(impl Metrics)` registers the hook.
- The default implementation is a no-op.
- Users can implement the trait for Prometheus, StatsD, OpenTelemetry, etc.
- The client-side `CallInterceptor` already provides `before`/`after` hooks
  suitable for latency and count metrics.

---

## 7.3 Rate Limiting Hooks

**Status:** Planned

**Motivation:** Public-facing agents need rate limiting to prevent abuse.
Rate limiting policy varies widely (per-IP, per-tenant, per-method), so the
framework should provide a hook rather than a built-in implementation.

**Approach:**

- Define a `RateLimiter` trait:

```rust
pub trait RateLimiter: Send + Sync + 'static {
    /// Returns `Ok(())` if the request is allowed, or `Err` with a
    /// retry-after duration if rate-limited.
    fn check(&self, method: &str, tenant: Option<&str>)
        -> Result<(), Duration>;
}
```

- The dispatcher calls `check()` before routing to the handler.
- On rejection, return HTTP 429 with a `Retry-After` header.
- Default: no rate limiting (always allows).

---

## 7.4 WebSocket Transport

**Status:** Planned (blocked on spec evolution)

**Motivation:** SSE is unidirectional (server-to-client). WebSocket enables
bidirectional streaming, which is useful for interactive agents that need
client-to-server messages mid-stream.

**Approach:**

- The `Transport` trait in `a2a-client` already abstracts the transport layer.
  A `WebSocketTransport` would implement the same trait.
- Server-side: add a WebSocket upgrade handler alongside the existing HTTP
  handler. The `RequestHandler` methods are transport-agnostic.
- Wait for the A2A spec to formalize WebSocket semantics before implementing.

---

## 7.5 Multi-Tenancy

**Status:** Partially implemented (tenant field in params)

**Motivation:** Hosting multiple logical agents on a single server instance,
each with isolated task stores and configurations.

**Current state:** The `tenant` field exists on request params and is passed
through to the store layer. The REST dispatcher strips a `/tenant/{id}` prefix.

**Remaining work:**

- Tenant-aware task store partitioning (currently all tenants share one store).
- Per-tenant configuration (timeouts, capacity limits, executor selection).
- Tenant authentication and authorization hooks.
- Consider a `TenantResolver` trait that maps incoming requests to tenant IDs.

---

## 7.6 Persistent Task Store

**Status:** Planned

**Motivation:** `InMemoryTaskStore` loses all data on process restart. Production
deployments need durable storage.

**Approach:**

The `TaskStore` trait is already designed for this:

```rust
pub trait TaskStore: Send + Sync + 'static {
    fn save(&self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send>>;
    fn get(&self, id: &TaskId) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send>>;
    fn list(&self, params: &ListTasksParams) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send>>;
    fn delete(&self, id: &TaskId) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send>>;
}
```

**Implementation guide for custom stores:**

1. Implement `TaskStore` for your storage backend (SQLite, PostgreSQL, Redis, etc.).
2. Use `RequestHandlerBuilder::with_task_store(Box::new(MyStore::new()))` to
   register it.
3. The `list()` method must handle filtering by `context_id` and `status`,
   cursor-based pagination via `page_token`, and `page_size` limits.
4. Terminal state eviction (TTL, capacity) should be handled by the store
   implementation or an external cleanup job.
5. Ensure your implementation is `Send + Sync` — it will be called from
   multiple tokio tasks concurrently.

**Example skeleton:**

```rust
struct PostgresTaskStore { pool: sqlx::PgPool }

impl TaskStore for PostgresTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::query("INSERT INTO tasks ...")
                .execute(&self.pool).await
                .map_err(|e| /* convert to A2aError */)?;
            Ok(())
        })
    }
    // ... other methods
}
```
