# Changelog

All notable changes to a2a-rust are documented in the project's [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).

## Versioning

a2a-rust follows [Semantic Versioning](https://semver.org/):

- **Major** (1.0.0) — Breaking API changes
- **Minor** (0.2.0) — New features, backward compatible
- **Patch** (0.2.1) — Bug fixes, backward compatible

All four workspace crates share the same version number and are released together.

## Release Process

Releases are triggered by pushing a version tag:

```bash
git tag v0.2.0
git push origin v0.2.0
```

The [release workflow](https://github.com/tomtom215/a2a-rust/blob/main/.github/workflows/release.yml) automatically:

1. Validates all crate versions match the tag
2. Runs the full CI suite
3. Publishes crates to crates.io in dependency order
4. Creates a GitHub release with notes

### Publish Order

```
a2a-protocol-types → a2a-protocol-client + a2a-protocol-server → a2a-protocol-sdk
```

This ensures each crate's dependencies are available before it publishes.

## Latest (Unreleased)

- **Axum framework integration** (`axum` feature) — `A2aRouter` for idiomatic
  Axum servers. All 11 REST methods, composable with other Axum routes/middleware.
- **TCK wire format conformance tests** — 44 tests validating wire format
  compatibility against the A2A v1.0 specification.
- **Wave 2 inline unit tests** — 110 new `#[cfg(test)]` tests added directly
  to 9 critical `a2a-protocol-server` source files (messaging, event_processing,
  push_config, lifecycle, handler/mod, REST/JSON-RPC/gRPC/WebSocket dispatchers).
  Total workspace test count: 1,750+ passing tests (with feature flags).

### Beyond-Spec Enhancements

- **OpenTelemetry metrics** (`otel` feature) — `OtelMetrics` with native OTLP export
- **Connection pool metrics** — `ConnectionPoolStats` and `on_connection_pool_stats` callback
- **Hot-reload agent cards** — `HotReloadAgentCardHandler` with file polling and SIGHUP
- **Store migration tooling** (`sqlite` feature) — `MigrationRunner` with V1–V3 built-in migrations
- **Per-tenant configuration** — `PerTenantConfig` and `TenantLimits` for differentiated service levels
- **`TenantResolver` trait** — `HeaderTenantResolver`, `BearerTokenTenantResolver`, `PathSegmentTenantResolver`
- **Agent card signing E2E** — test 79 in agent-team suite (`signing` feature)

### Bug Fixes (Passes 7–10)

- Event queue serialization error swallowing fixed with proper error propagation
- Capacity eviction now falls back to non-terminal tasks when terminal tasks are insufficient
- Lagged event count now exposed in reader warnings for observability
- Timeout errors now correctly classified as retryable (`ClientError::Timeout`)
- SSE parser O(n) dequeue replaced with `VecDeque` for O(1) `pop_front`
- Double-encoded path traversal bypass fixed with two-pass percent-decoding
- gRPC stream errors now preserve protocol error codes
- Rate limiter TOCTOU race fixed with CAS loop
- Push config store now enforces global limits (DoS prevention)

### v0.2.0 (2026-03-15)

Initial implementation of A2A v1.0.0 with all 11 protocol methods, dual transport (JSON-RPC + REST), SSE streaming, push notifications, agent card discovery, HTTP caching, enterprise hardening, and 600+ tests (workspace total now 1,750+ after subsequent waves).

For the complete version history, see [CHANGELOG.md](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md).
