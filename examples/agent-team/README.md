<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Agent Team — Full SDK Dogfood & E2E Test Suite

An end-to-end example that deploys five specialized A2A agents over JSON-RPC, REST and gRPC, runs 100 end-to-end tests against them, then sweeps every A2A method over every binding against a sixth agent that serves all four.

This is the project's primary integration test and SDK dogfood — if it passes, the SDK works.

## Agents

| Agent | Transport | Role | Key features exercised |
|-------|-----------|------|----------------------|
| **CodeAnalyzer** | JSON-RPC | File analysis, LOC counting | Streaming, artifacts, multi-part messages |
| **BuildMonitor** | REST | Simulated cargo builds | Streaming, cancellation, push notifications |
| **HealthMonitor** | JSON-RPC | Agent health checks | Push notifications, interceptors |
| **Coordinator** | REST | Orchestration, delegation | A2A client calls, task aggregation, metrics |
| **GrpcAnalyzer** | gRPC | Same as CodeAnalyzer | gRPC transport |
| **Surface agent** | all four | Exists to be swept | Every method over every binding (`src/surface.rs`) |

## Running

```bash
cargo run -p agent-team                                   # 102 results: 100 tests + the surface sweep
RUST_LOG=debug cargo run -p agent-team --features tracing # with structured logging
```

Every feature but `tracing` is on by default (`websocket`, `grpc`, `axum`,
`sqlite`, `signing`, `otel`), so a bare run is the full suite. On 2026-10-15
it printed `All 102 tests passed` and `Every claimed SDK feature was
exercised`, and the sweep `44 exercised, 0 not applicable, 0 missing, of 44
cells`. CI runs it with `--all-features` (`ci.yml`, job `dogfood`).

A narrowed build (`--no-default-features --features …`) runs fewer tests,
prints each feature area it did not build, and exits `2` by design: a
dogfood run that skipped a feature area is not complete.

Exit codes: `0` everything passed and every claimed area was built; `1` a test
failed or the printed feature table drifted from the tests behind it; `2` a
matrix gap or a feature area not built.

## Test categories

| Range | Category | What it covers |
|-------|----------|---------------|
| 1-10 | **Basic** | Sync/stream send, REST/JSON-RPC, get/list tasks, push config, multi-part |
| 11-20 | **Lifecycle** | Orchestration, metadata, cancel, agent cards, pagination |
| 21-30 | **Edge cases** | Cancel nonexistent, return_immediately, concurrent requests, metrics |
| 31-40 | **Stress** | High concurrency, mixed transport, large payloads, event ordering |
| 41-50 | **Dogfood** | Agent card correctness, push events, multiple artifacts, history |
| 51-58 | **Transport** | WebSocket, gRPC, multi-tenancy |
| 61-99 | **Coverage gaps** | Batch JSON-RPC, auth rejection, dynamic cards, caching, backpressure, SQLite, Axum, signing |

## SDK features exercised

The test suite exercises 40+ SDK features including:

- `AgentExecutor` trait (4 implementations)
- `RequestHandlerBuilder` (all configuration options)
- `JsonRpcDispatcher` + `RestDispatcher`
- `ClientBuilder` (JSON-RPC + REST bindings; `from_card` + `build_grpc` on a bare `host:port` gRPC target)
- Sync and streaming `SendMessage`
- `GetTask`, `ListTasks` (pagination, context, status filters)
- `CancelTask` with executor override
- Push notification config CRUD + `HttpPushSender` delivery
- `ServerInterceptor` (audit logging + auth)
- Custom `Metrics` observer
- Agent card discovery (correct URLs via pre-bind)
- Multi-part messages (text + data + file)
- Artifact append mode + multiple artifacts
- `TenantAwareInMemoryTaskStore` isolation
- Batch JSON-RPC (single, multi, empty, mixed, streaming rejection)
- Dynamic agent cards + HTTP caching (ETag + 304)
- Backpressure / lagged event queue
- State transition validation
- Executor timeout + error propagation

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                        main()                                │
│                                                              │
│  1. Start webhook receiver (for push notification tests)     │
│  2. Pre-bind the agents' listeners (get real addresses)      │
│  3. Build agent cards with correct URLs                      │
│  4. Build RequestHandlers with interceptors + metrics        │
│  5. Start the agents (JSON-RPC, REST, gRPC)                  │
│  6. Run 100 E2E tests, then the 44-cell surface sweep        │
│  7. Print results + metrics summary                          │
└──────────┬──────────┬──────────┬──────────┬──────────────────┘
           │          │          │          │
    ┌──────▼──────┐ ┌─▼────┐ ┌───▼───┐ ┌────▼───────┐
    │CodeAnalyzer │ │Build │ │Health │ │Coordinator │
    │  JSON-RPC   │ │ REST │ │JSONRPC│ │    REST    │
    └─────────────┘ └──────┘ └───────┘ └────────────┘
```

## Prerequisites

- Rust 1.94+ (MSRV)
- No external services — everything runs in-process

## License

Apache-2.0
