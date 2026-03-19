<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Agent Team — Full SDK Dogfood & E2E Test Suite

A comprehensive end-to-end example that deploys 4 specialized A2A agents communicating over multiple transports, then runs a full test suite exercising every major SDK feature.

This is the project's primary integration test and SDK dogfood — if it passes, the SDK works.

## Agents

| Agent | Transport | Role | Key features exercised |
|-------|-----------|------|----------------------|
| **CodeAnalyzer** | JSON-RPC | File analysis, LOC counting | Streaming, artifacts, multi-part messages |
| **BuildMonitor** | REST | Cargo check/test runner | Streaming, cancellation, push notifications |
| **HealthMonitor** | JSON-RPC | Agent health checks | Push notifications, interceptors |
| **Coordinator** | REST | Orchestration, delegation | A2A client calls, task aggregation, metrics |
| **GrpcAnalyzer** | gRPC | Same as CodeAnalyzer | gRPC transport (requires `grpc` feature) |

## Running

```bash
# Base test suite (81 tests):
cargo run -p agent-team

# With all optional features (94+ tests):
cargo run -p agent-team --all-features

# With structured logging:
RUST_LOG=debug cargo run -p agent-team --features tracing

# Individual feature flags:
cargo run -p agent-team --features websocket   # +2 WebSocket tests
cargo run -p agent-team --features grpc        # +3 gRPC tests
cargo run -p agent-team --features axum        # +3 Axum framework tests
cargo run -p agent-team --features sqlite      # +2 SQLite store tests
cargo run -p agent-team --features signing     # +1 JWS signing test
cargo run -p agent-team --features otel        # +1 OpenTelemetry test
```

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
- `ClientBuilder` (JSON-RPC + REST bindings)
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
│  2. Pre-bind 4 TCP listeners (get real addresses)            │
│  3. Build agent cards with correct URLs                      │
│  4. Build RequestHandlers with interceptors + metrics        │
│  5. Start 4 agent servers (JSON-RPC + REST)                  │
│  6. Run 81-94 E2E tests against the live agents              │
│  7. Print results + metrics summary                          │
└──────────┬──────────┬──────────┬──────────┬──────────────────┘
           │          │          │          │
    ┌──────▼──────┐ ┌─▼────┐ ┌───▼───┐ ┌────▼───────┐
    │CodeAnalyzer │ │Build │ │Health │ │Coordinator │
    │  JSON-RPC   │ │ REST │ │JSONRPC│ │    REST    │
    └─────────────┘ └──────┘ └───────┘ └────────────┘
```

## Prerequisites

- Rust 1.93+ (MSRV)
- `protoc` (only when `--features grpc` is enabled)
- No external services — everything runs in-process

## License

Apache-2.0
