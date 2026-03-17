<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# A2A Rust SDK — Dogfood Report

> **This document has been consolidated into the project book.**
> See the canonical versions at:
>
> - [Dogfooding Overview](../../book/src/deployment/dogfooding.md)
> - [Bugs Found & Fixed](../../book/src/deployment/dogfooding-bugs.md) — 13 bugs across 4 passes
> - [Test Coverage Matrix](../../book/src/deployment/dogfooding-tests.md) — 66 E2E tests (69 with gRPC)
> - [Open Issues & Roadmap](../../book/src/deployment/dogfooding-open-issues.md) — design debt and future work

## Quick Summary

| Category | Count |
|----------|-------|
| Critical bugs fixed | 3 (SDK) + 3 (example) |
| Design issues identified | 5 |
| Test gaps found | 9 |
| New tests added (pass 4) | 10 |
| Total E2E tests | 66 (69 with optional gRPC) |

### Critical SDK Bug Fixed

**JSON-RPC `ListTaskPushNotificationConfigs` param type mismatch**
(`crates/a2a-server/src/dispatch/jsonrpc.rs`): Parsed `TaskIdParams` instead
of `ListPushConfigsParams`, breaking push config listing via JSON-RPC. REST
worked because it uses path-based routing.

### Critical Example Bugs Fixed

1. **Agent card URLs** were `"http://placeholder"` — solved with pre-bind
   listener pattern.
2. **Webhook event classifier** checked wrong field names (`status` vs
   `statusUpdate`).
3. **Push notification drain** — final report always showed 0 events because
   test 36 drained the receiver first. Fixed with `snapshot()`.
