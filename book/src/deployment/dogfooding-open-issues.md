# Dogfooding: Open Issues & Future Work

All architecture, ergonomics, observability, performance, and durability issues from passes 1–8 have been resolved — see [Bugs Found & Fixed](./dogfooding-bugs.md) for the full list (36 bugs across 8 passes).

## E2E Test Coverage Gaps — Resolved

All previously identified coverage gaps have been addressed:

| Area | Unit/Integration Coverage | E2E Status |
|---|---|---|
| **Agent card signing** | `signing` module tests (JWS/ES256, RFC 8785) | ✅ `test_agent_card_signing` in `examples/agent-team/src/tests/coverage_gaps.rs` (`#[cfg(feature = "signing")]`) |

## Future Work — Implemented

All proposed features have been implemented:

| Feature | Location | Details |
|---|---|---|
| **OpenTelemetry integration** | `crates/a2a-server/src/otel.rs` | `OtelMetrics`, `OtelMetricsBuilder`, `init_otlp_pipeline` — feature-gated under `otel` |
| **Connection pooling metrics** | `crates/a2a-server/src/metrics.rs` | `ConnectionPoolStats` struct; `on_connection_pool_stats` method on `Metrics` trait |
| **Hot-reload agent cards** | `crates/a2a-server/src/agent_card/hot_reload.rs` | `HotReloadAgentCardHandler` with `reload_from_file`, `spawn_poll_watcher`, `spawn_signal_watcher` (SIGHUP) |
| **Store migration tooling** | `crates/a2a-server/src/store/migration.rs` | `MigrationRunner`, `BUILTIN_MIGRATIONS` (V1–V3), `schema_versions` tracking table |
| **Per-tenant configuration** | `crates/a2a-server/src/tenant_config.rs` | `PerTenantConfig`, `TenantLimits` with builder pattern for per-tenant overrides |
| **TenantResolver trait** | `crates/a2a-server/src/tenant_resolver.rs` | `TenantResolver` trait with `HeaderTenantResolver`, `BearerTokenTenantResolver`, `PathSegmentTenantResolver` |
