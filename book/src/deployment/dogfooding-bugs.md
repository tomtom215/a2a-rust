# Dogfooding: Bugs Found & Fixed

Eight dogfooding passes across `v0.1.0` and `v0.2.0` uncovered **36 real bugs** that 600+ unit tests, integration tests, property tests, and fuzz tests did not catch. All 36 have been fixed.

## Summary

| Pass | Focus | Bugs | Severity |
|------|-------|------|----------|
| [Pass 1](./dogfooding-bugs-early.md#pass-1-initial-dogfood-3-bugs) | Initial dogfood | 3 | 2 Medium, 1 Low |
| [Pass 2](./dogfooding-bugs-early.md#pass-2-hardening-audit-6-bugs) | Hardening audit | 6 | 1 High, 2 Medium, 3 Low |
| [Pass 3](./dogfooding-bugs-early.md#pass-3-stress-testing-1-bug) | Stress testing | 1 | 1 High |
| [Pass 4](./dogfooding-bugs-early.md#pass-4-sdk-regression-testing-3-bugs) | SDK regressions | 3 | 2 Critical, 1 Medium |
| [Pass 5](./dogfooding-bugs-hardening.md#pass-5-hardening--concurrency-audit-4-bugs) | Concurrency | 4 | 2 High, 1 Medium, 1 Low |
| [Pass 6](./dogfooding-bugs-hardening.md#pass-6-architecture-audit-5-bugs) | Architecture | 5 | 1 Critical, 1 High, 3 Medium |
| [Pass 7](./dogfooding-bugs-deep.md#pass-7-deep-dogfood-9-bugs) | Deep dogfood | 9 | 1 Critical, 2 High, 4 Medium, 2 Low |
| [Pass 8](./dogfooding-bugs-deep.md#pass-8-deep-dogfood-5-bugs) | Performance & security | 5 | 1 Critical, 2 Medium, 1 Medium, 1 Low |

### By Severity

| Severity | Count | Examples |
|----------|-------|---------|
| **Critical** | 5 | Timeout retry broken (#32), push config DoS (#26), placeholder URLs (#11, #12, #18) |
| **High** | 6 | Concurrent SSE (#9), return_immediately ignored (#10), TOCTOU race (#15), SSRF bypass (#25) |
| **Medium** | 16 | REST field stripping (#1), query encoding (#19), path traversal (#35) |
| **Low** | 9 | Metrics hooks (#2, #6, #7), gRPC error context (#36) |

### Configuration Hardening

Extracted all hardcoded constants into configurable structs during passes 2-7:

| Struct | Fields | Where |
|---|---|---|
| `DispatchConfig` | `max_request_body_size`, `body_read_timeout`, `max_query_string_length` | Both dispatchers |
| `PushRetryPolicy` | `max_attempts`, `backoff` | `HttpPushSender` |
| `HandlerLimits` | `max_id_length`, `max_metadata_size`, `max_cancellation_tokens`, `max_token_age` | `RequestHandler` |

## Sub-pages

- **[Passes 1–4: Foundation](./dogfooding-bugs-early.md)** — Initial discovery, hardening, stress testing, SDK regressions (13 bugs)
- **[Passes 5–6: Hardening](./dogfooding-bugs-hardening.md)** — Concurrency, architecture, durability (9 bugs)
- **[Passes 7–8: Deep Dogfood](./dogfooding-bugs-deep.md)** — Security, performance, error handling (14 bugs)
