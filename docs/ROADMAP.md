<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Roadmap

## Open Items

### Potential Future Enhancements

Features not part of A2A v1.0.0 that could add value:

| Feature | Description | Effort |
|---|---|---|
| **OpenTelemetry integration** | Native OTLP export for traces and metrics instead of the callback-based `Metrics` trait. | Medium |
| **Connection pooling metrics** | Expose hyper connection pool stats (active/idle connections) via the `Metrics` trait. | Small |
| **Hot-reload agent cards** | File-watch or signal-based agent card reload without server restart. | Small |
| **Store migration tooling** | Schema versioning and migration support for `SqliteTaskStore`. | Medium |
| **Per-tenant configuration** | Per-tenant timeouts, capacity limits, executor selection. | Medium |
| **TenantResolver trait** | Custom tenant ID extraction from requests. | Small |
| **Agent card signing E2E** | Add JWS key setup to agent-team for E2E signing test coverage. | Small |

---

## Completed Extensions (Reference)

All beyond-spec extensions have been implemented. See the book's [feature documentation](../book/src/reference/configuration.md) for usage details.

### Request ID Propagation

UUID v4 request IDs via `X-Request-Id` header. Server extracts (or generates) and includes in `tracing` span fields. Implementable as a `CallInterceptor`.

### Metrics Hooks

`Metrics` trait with callbacks: `on_request`, `on_response`, `on_error`, `on_latency`, `on_queue_depth_change`. Registered via `RequestHandlerBuilder::with_metrics()`. Blanket `impl Metrics for Arc<T>`.

### Rate Limiting

Built-in `RateLimitInterceptor` with fixed-window per-caller counters. Configurable via `RateLimitConfig`. Caller keys from auth identity, `X-Forwarded-For`, or `"anonymous"`.

### gRPC Transport

`grpc` feature flag enables `GrpcDispatcher` (server) and `GrpcTransport` (client) via `tonic`. JSON payloads in protobuf `bytes` fields. Configure via `GrpcConfig` / `GrpcTransportConfig`.

### WebSocket Transport

`websocket` feature flag enables `WebSocketDispatcher` (server) and `WebSocketTransport` (client) via `tokio-tungstenite`. JSON-RPC 2.0 over WebSocket text frames.

### Multi-Tenancy

In-memory: `TenantAwareInMemoryTaskStore` via `tokio::task_local!` + `TenantContext::scope()`.
SQLite: `TenantAwareSqliteTaskStore` via `tenant_id` column partitioning.

### Persistent Task Store

`SqliteTaskStore` and `SqlitePushConfigStore` behind `sqlite` feature flag. Async via `sqlx`, schema auto-creation, cursor-based pagination, upsert support.
