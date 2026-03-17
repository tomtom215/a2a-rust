# Dogfooding: Open Issues & Future Work

All architecture, ergonomics, observability, performance, and durability issues from passes 1–8 have been resolved — see [Bugs Found & Fixed](./dogfooding-bugs.md) for the full list (36 bugs across 8 passes).

## E2E Test Coverage Gaps

These features have dedicated unit/integration tests but are **not yet exercised** in the agent-team E2E suite:

| Area | Unit/Integration Coverage | E2E Status | Risk |
|---|---|---|---|
| **Agent card signing** | `signing` module tests (JWS/ES256, RFC 8785) | Requires JWS key setup in agent-team | Low |

## Potential Future Work

Features that are not part of the current A2A v1.0.0 spec but could add value:

| Feature | Description | Effort |
|---|---|---|
| **OpenTelemetry integration** | Native OTLP export for traces and metrics instead of the callback-based `Metrics` trait. | Medium |
| **Connection pooling metrics** | Expose hyper connection pool stats (active/idle connections) via the `Metrics` trait. | Small |
| **Hot-reload agent cards** | File-watch or signal-based agent card reload without server restart. | Small |
| **Store migration tooling** | Schema versioning and migration support for `SqliteTaskStore`. | Medium |
| **Per-tenant configuration** | Per-tenant timeouts, capacity limits, executor selection. | Medium |
| **TenantResolver trait** | Custom tenant ID extraction from requests. | Small |
