# Agent Team

The project's primary integration test and SDK dogfood. Deploys 4 specialized agents communicating over multiple transports, then runs **81+ E2E tests** (94 with all optional features) exercising every major SDK feature.

**Source:** [`examples/agent-team/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/agent-team)

If this passes, the SDK works.

## Agents

| Agent | Transport | Role | Key features exercised |
|-------|-----------|------|----------------------|
| **CodeAnalyzer** | JSON-RPC | File analysis, LOC counting | Streaming, artifacts, multi-part messages |
| **BuildMonitor** | REST | Cargo check/test runner | Streaming, cancellation, push notifications |
| **HealthMonitor** | JSON-RPC | Agent health checks | Push notifications, interceptors |
| **Coordinator** | REST | Orchestration, delegation | A2A client calls, task aggregation, metrics |
| **GrpcAnalyzer** | gRPC | Same as CodeAnalyzer | gRPC transport (`grpc` feature) |

## Running

```bash
# Base suite (81 tests):
cargo run -p agent-team

# All optional features (94+ tests):
cargo run -p agent-team --all-features

# Individual features:
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
| 1-10 | Basic | Sync/stream send, REST/JSON-RPC, get/list tasks, push config, multi-part |
| 11-20 | Lifecycle | Orchestration, metadata, cancel, agent cards, pagination |
| 21-30 | Edge cases | Cancel nonexistent, return_immediately, concurrent requests, metrics |
| 31-40 | Stress | High concurrency, mixed transport, large payloads, event ordering |
| 41-50 | Dogfood | Agent card correctness, push events, multiple artifacts, history |
| 51-58 | Transport | WebSocket, gRPC, multi-tenancy |
| 61-99 | Coverage gaps | Batch JSON-RPC, auth, dynamic cards, caching, backpressure, Axum, SQLite, signing |

For the full test matrix, see [Test Coverage Matrix](../deployment/dogfooding-tests.md).

## SDK features exercised

The suite exercises **40+ SDK features** including:

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
- Batch JSON-RPC (single, multi, empty, mixed, streaming rejection)
- Dynamic agent cards + HTTP caching (ETag + 304)
- `TenantAwareInMemoryTaskStore` isolation
- State transition validation + backpressure

For bugs discovered and fixed during dogfooding, see [Bugs Found & Fixed](../deployment/dogfooding-bugs.md).
