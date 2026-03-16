<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Roadmap — Beyond-Spec Extensions

This document tracks planned enhancements that go beyond the A2A v1.0.0 spec.
Each section describes the extension, its motivation, and recommended
implementation approach.

---

## 7.1 Request ID Propagation

**Status:** Implemented

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

**Status:** ✅ Implemented

The `Metrics` trait provides callbacks for request counts, error rates, queue
depths, and latency:

- `on_request(method)` — called at the start of every handler method
- `on_response(method)` — called on successful completion
- `on_error(method, error)` — called on failure
- `on_latency(method, duration)` — called with elapsed time for every request
- `on_queue_depth_change(active_queues)` — called when event queues are created/destroyed

`RequestHandlerBuilder::with_metrics(impl Metrics)` registers the hook.
The default implementation is a no-op. A blanket `impl Metrics for Arc<T>`
allows sharing a single metrics instance across multiple handlers without
wrapper types. Users can implement the trait for Prometheus, StatsD,
OpenTelemetry, etc.

---

## 7.3 Rate Limiting Hooks

**Status:** ✅ Implemented

**Motivation:** Public-facing agents need rate limiting to prevent abuse.

**Implementation:**

- `RateLimitInterceptor` — a built-in `ServerInterceptor` using a fixed-window
  per-caller counter. Configurable via `RateLimitConfig` (requests per window,
  window duration in seconds).
- Caller keys derived from `CallContext::caller_identity` (set by auth
  interceptors), `X-Forwarded-For` header (first IP), or `"anonymous"` fallback.
- When the limit is exceeded, requests are rejected with an A2A error.
- For advanced use cases (sliding windows, distributed counters), implement a
  custom `ServerInterceptor` or use a reverse proxy (nginx, Envoy).

```rust
use a2a_protocol_server::{RateLimitInterceptor, RateLimitConfig};
use std::sync::Arc;

let limiter = Arc::new(RateLimitInterceptor::new(RateLimitConfig {
    requests_per_window: 100,
    window_secs: 60,
}));
// Add to handler: builder.with_interceptor(limiter)
```

---

## 7.4 WebSocket Transport

**Status:** Planned (blocked on spec evolution)

**Motivation:** SSE is unidirectional (server-to-client). WebSocket enables
bidirectional streaming, which is useful for interactive agents that need
client-to-server messages mid-stream.

**Approach:**

- The `Transport` trait in `a2a-protocol-client` already abstracts the transport layer.
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

**Status:** ✅ Implemented

Reference implementations of `SqliteTaskStore` and `SqlitePushConfigStore` are
provided behind the `sqlite` feature flag. They use `sqlx` for async SQLite
access with schema auto-creation, cursor-based pagination, upsert support,
and integration tests using in-memory SQLite.

```toml
[dependencies]
a2a-protocol-server = { version = "0.2", features = ["sqlite"] }
```

The `TaskStore` and `PushConfigStore` traits remain the extension points for
custom backends (PostgreSQL, Redis, etc.). See the SQLite implementations as
reference.
