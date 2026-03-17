<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# Roadmap

## Recently Completed Enhancements

All proposed beyond-spec features have been implemented:

| Feature | Location | Details |
|---|---|---|
| **OpenTelemetry integration** | `crates/a2a-server/src/otel.rs` | `OtelMetrics` with OTLP export via `opentelemetry-otlp`; feature-gated under `otel` |
| **Connection pooling metrics** | `crates/a2a-server/src/metrics.rs` | `ConnectionPoolStats` struct; `on_connection_pool_stats` on `Metrics` trait |
| **Hot-reload agent cards** | `crates/a2a-server/src/agent_card/hot_reload.rs` | `HotReloadAgentCardHandler` with file polling and SIGHUP reload |
| **Store migration tooling** | `crates/a2a-server/src/store/migration.rs` | `MigrationRunner` with `BUILTIN_MIGRATIONS` (V1–V3), `schema_versions` table |
| **Per-tenant configuration** | `crates/a2a-server/src/tenant_config.rs` | `PerTenantConfig`, `TenantLimits` with per-tenant overrides |
| **TenantResolver trait** | `crates/a2a-server/src/tenant_resolver.rs` | `HeaderTenantResolver`, `BearerTokenTenantResolver`, `PathSegmentTenantResolver` |
| **Agent card signing E2E** | `examples/agent-team/src/tests/coverage_gaps.rs` | `test_agent_card_signing` with ES256 key generation (`#[cfg(feature = "signing")]`) |

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
