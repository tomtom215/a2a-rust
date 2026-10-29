# Dogfooding: Bugs Found & Fixed

Thirteen documented dogfooding passes across `v0.1.0`–`v0.3.0` uncovered **68 bugs** that the unit, integration, property and fuzz tests of the time (about 1,630 at v0.3.0, about 1,850 with all feature flags) did not catch. All have been fixed.

> **Note:** Pass 11 is recorded in the [CHANGELOG](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md) (v0.3.0); Passes 13 and 14 are not recorded in this repository. None of the three is included in the count below. Fix descriptions are as first applied; several were later changed (Bugs 17, 27, 35, 36, 43, 45, P16-1).

## Summary

| Pass | Focus | Bugs | Severity |
|------|-------|------|----------|
| [Pass 1](./dogfooding-bugs/passes-1-6.md#pass-1-initial-dogfood-3-bugs) | Initial dogfood | 3 | 1 Medium, 2 Low |
| [Pass 2](./dogfooding-bugs/passes-1-6.md#pass-2-hardening-audit-6-bugs) | Hardening audit | 6 | 1 High, 3 Medium, 2 Low |
| [Pass 3](./dogfooding-bugs/passes-1-6.md#pass-3-stress-testing-1-bug) | Stress testing | 1 | 1 High |
| [Pass 4](./dogfooding-bugs/passes-1-6.md#pass-4-sdk-regression-testing-3-bugs) | SDK regressions | 3 | 2 Critical, 1 Medium |
| [Pass 5](./dogfooding-bugs/passes-1-6.md#pass-5-hardening--concurrency-audit-4-bugs) | Concurrency | 4 | 2 High, 1 Medium, 1 Low |
| [Pass 6](./dogfooding-bugs/passes-1-6.md#pass-6-architecture-audit-5-bugs) | Architecture | 5 | 1 Critical, 2 High, 2 Medium |
| [Pass 7](./dogfooding-bugs/passes-7-10.md#pass-7-deep-dogfood-9-bugs) | Deep dogfood | 9 | 1 Critical, 2 High, 4 Medium, 2 Low |
| [Pass 8](./dogfooding-bugs/passes-7-10.md#pass-8-deep-dogfood-5-bugs) | Performance & security | 5 | 1 Critical, 3 Medium, 1 Low |
| [Pass 9](./dogfooding-bugs/passes-7-10.md#pass-9-scale-probing-4-bugs) | Scale probing | 4 | 1 High, 2 Medium, 1 Low |
| [Pass 10](./dogfooding-bugs/passes-7-10.md#pass-10-exhaustive-audit-3-bugs) | Exhaustive audit | 3 | 1 High, 1 Medium, 1 Low |
| [Pass 12](./dogfooding-bugs/passes-12-16.md#pass-12-pre-release-hardening-4-bugs) | Pre-release hardening | 4 | 1 Critical, 2 High, 1 Medium |
| [Pass 15](./dogfooding-bugs/passes-12-16.md#pass-15-concurrency-security--robustness-18-bugs) | Concurrency, security & robustness | 18 | 3 High, 11 Medium, 4 Low |
| [Pass 16](./dogfooding-bugs/passes-12-16.md#pass-16-deep-e2e-dogfood-3-bugs) | Deep E2E dogfood | 3 | 2 Critical, 1 Medium |

### By Severity

| Severity | Count | Examples |
|----------|-------|---------|
| **Critical** | 8 | Timeout retry broken (#32), push config DoS (#26), JSON-RPC push-config listing broken (#11), placeholder URLs (#12, #18), truncate_body panic (#44), ListPushConfigs wrong format (P16-#1), push config validation ignores `allow_private_urls` (P16-#2) |
| **High** | 15 | Concurrent SSE (#9), return_immediately ignored (#10), TOCTOU race (#15), SSRF bypass (#25), credentials-store lock poisoning masked (#14), query encoding (#19), shutdown hangs (#23), store write failures dropped (#21), fast-executor events missed (#38), serialization error swallowed (#41), SSE line_buf OOM (#45), interceptor params discarded (#46), gRPC Mutex concurrency (C1), WebSocket deadlock (C2), SSRF DNS rebinding (H6) |
| **Medium** | 31 | REST field stripping (#1), path traversal (#35), capacity eviction fails (#42), test coverage gaps (#40), WebSocket auth (C3), retry serialization (H7), agent card OOM (H8), error propagation (M3), WebSocket limits (M9, M10), validation (M13, M14, M17, M19), tenant dropped by REST dispatch (M16), REST path params (#47), background processor silent exit (P16-#3) |
| **Low** | 14 | Metrics hooks (#2, #6, #7), gRPC error context (#36), lagged event count hidden (#43), shutdown polling (L4), SQLite parameterized queries (L7), timestamp validation (L14), batch size confirmed (M8) |

### Configuration Hardening

Extracted all hardcoded constants into configurable structs during passes 2-7:

| Struct | Fields | Where |
|---|---|---|
| `DispatchConfig` | `max_request_body_size`, `body_read_timeout`, `max_query_string_length`, `sse_keep_alive_interval`, `sse_channel_capacity`, `max_batch_size` | Both dispatchers |
| `PushRetryPolicy` | `max_attempts`, `backoff` | `HttpPushSender` |
| `HandlerLimits` | `max_id_length`, `max_metadata_size`, `max_cancellation_tokens`, `max_token_age`, `push_delivery_timeout`, `max_artifacts_per_task` | `RequestHandler` |

## The write-ups

Each bug has a write-up: what was wrong, how it was found, and the fix as
first applied.

| Page | Passes | Bugs |
|---|---|---|
| [Passes 1 to 6](./dogfooding-bugs/passes-1-6.md) | Initial dogfood through the architecture audit | 1-22 |
| [Passes 7 to 10](./dogfooding-bugs/passes-7-10.md) | Deep dogfood, scale probing, exhaustive audit | 23-43 |
| [Passes 12 to 16](./dogfooding-bugs/passes-12-16.md) | Pre-release hardening; concurrency, security and robustness; deep end-to-end | 44-47, C1-L14, P16-1 to P16-3 |

Several have a shorter entry, written as a rule to follow, in
[Pitfalls & Lessons Learned](../reference/pitfalls.md).
