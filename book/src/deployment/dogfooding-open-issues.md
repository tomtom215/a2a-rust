# Dogfooding: Open Issues & Future Work

Remaining gaps identified during dogfooding that have not yet been addressed. All architecture, ergonomics, observability, performance, and durability issues from earlier passes have been resolved — see [Bugs Found & Fixed](./dogfooding-bugs.md) for the full list.

## E2E Test Coverage Gaps

These features have dedicated unit/integration tests but are **not yet exercised** in the agent-team E2E suite:

| Area | Unit/Integration Coverage | E2E Status | Risk |
|---|---|---|---|
| **Agent card signing** | `signing` module tests (JWS/ES256, RFC 8785) | Requires JWS key setup in agent-team | Low |

## Recently Closed Coverage Gaps (Tests 61-71)

The following gaps were closed by adding E2E tests 61-71 in `coverage_gaps.rs`:

| Area | Test # | E2E Coverage |
|---|---|---|
| **Batch JSON-RPC** | 61-66 | Single, multi, empty, mixed, streaming/subscribe rejection |
| **Real auth rejection** | 67 | Interceptor rejects unauthenticated requests |
| **Extended agent card** | 68 | `GetExtendedAgentCard` via JSON-RPC |
| **Dynamic agent cards** | 69 | `DynamicAgentCardHandler` runtime card generation |
| **Agent card HTTP caching** | 70 | ETag, `If-None-Match`, 304 Not Modified |
| **Backpressure / `Lagged`** | 71 | Slow reader skips lagged events (capacity=2) |

## Potential Future Work

Features that are not part of the current A2A v1.0.0 spec but could add value:

| Feature | Description | Effort |
|---|---|---|
| **gRPC transport** | A2A spec lists gRPC as optional. Would require protobuf definitions, `tonic` crate, and `build.rs` proto compilation. | Large |
| **OpenTelemetry integration** | Native OTLP export for traces and metrics instead of the callback-based `Metrics` trait. | Medium |
| **Connection pooling metrics** | Expose hyper connection pool stats (active/idle connections) via the `Metrics` trait. | Small |
| **Hot-reload agent cards** | File-watch or signal-based agent card reload without server restart. | Small |
| **Store migration tooling** | Schema versioning and migration support for `SqliteTaskStore`. | Medium |

## Resolved in Previous Passes

All issues from dogfooding passes 1–4 have been resolved:

- Push delivery for streaming mode (background event processor)
- `CallContext` HTTP headers for interceptors
- `PartContent` tagged enum per A2A spec
- `AgentExecutor` boilerplate (`boxed_future`, `agent_executor!` macro)
- `Arc<T: Metrics>` blanket impl
- `Metrics::on_latency` callback
- `TaskStore::count()` method
- Double serialization in `EventQueueWriter::write()`
- `InMemoryTaskStore` write lock contention
- Persistent store implementations (SQLite)
- All hardcoded constants made configurable
- Server startup helpers (`serve()`, `serve_with_addr()`)
- Request ID / trace context propagation
- Client retry logic (`RetryPolicy`)
- Connection pooling in coordinator pattern
- Executor event emission boilerplate (`EventEmitter`)

See [Bugs Found & Fixed](./dogfooding-bugs.md) for details on each resolution.
