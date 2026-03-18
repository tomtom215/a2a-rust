# Dogfooding: The Agent Team Example

The best way to find bugs in an SDK is to use it yourself — under real conditions, with real complexity, exercising real interaction patterns. Unit tests verify individual functions. Integration tests verify pairwise contracts. But only dogfooding reveals the emergent issues that appear when all the pieces come together.

The `agent-team` example (`examples/agent-team/`) is a full-stack dogfood of every a2a-rust capability. It deploys 4 specialized agents that discover each other, delegate work, stream results, and report health — all via the A2A protocol. A comprehensive test suite of **71 E2E tests** (78 with optional WebSocket, gRPC, signing, and OTel features) runs in ~2.5 seconds.

## Why Dogfood?

Unit tests and integration tests are necessary but insufficient. **No single test type is enough — it requires an ensemble of all types to catch everything.** Here's what each layer catches and misses:

| Testing level | What it catches | What it misses |
|---|---|---|
| **Unit tests** | Logic errors in isolated functions | Interaction bugs, serialization mismatches |
| **Integration tests** | Pairwise contracts between components | Multi-hop communication, emergent behavior |
| **Property tests** | Edge cases in data handling | Protocol flow issues, lifecycle bugs |
| **Fuzz tests** | Malformed input handling | Semantic correctness of valid flows |
| **Dogfooding** | DX issues, multi-hop bugs, performance surprises, missing features | Weak assertions, dead code paths |
| **Mutation tests** | Weak/missing assertions, dead code paths, off-by-one errors, swapped operands | Protocol-level emergent behavior |

**The critical lesson:** After building 1,750+ tests across all of the above categories — unit, integration, property, fuzz, and 71 E2E dogfood tests that caught 36 real bugs — **the entire suite was green.** Every CI check passed. Then we ran mutation testing, and it found gaps in every crate. Tests that *looked* comprehensive were silently missing assertions on return values, boundary conditions, delegation correctness, and hash function specifics.

Mutation testing fills the gap between "tests pass" and "tests actually detect bugs." A test suite with 100% line coverage can still have a 0% mutation score if every assertion is trivial. Mutation testing is the only technique that directly measures *test effectiveness* rather than test *existence*. See **[Testing Your Agent — Mutation Testing](./testing.md#mutation-testing)** for setup and usage.

Dogfooding operates at the highest level of the testing pyramid. It catches the class of bugs that live in the seams between components — bugs that only manifest when a real application exercises the full stack in realistic patterns. But even dogfooding cannot verify that your *assertions* are strong enough to catch regressions — that's where mutation testing completes the picture.

## The Agent Team Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     E2E Test Harness                        │
│              (71 tests, ~2500ms total)                      │
└─────┬───────────┬───────────┬───────────┬───────────────────┘
      │           │           │           │
      ▼           ▼           ▼           ▼
┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐
│  Code    │ │  Build   │ │  Health  │ │ Coordin- │
│ Analyzer │ │ Monitor  │ │ Monitor  │ │   ator   │
│ JSON-RPC │ │  REST    │ │ JSON-RPC │ │  REST    │
└──────────┘ └──────────┘ └──────────┘ └─────┬────┘
                                             │ A2A
                          ┌──────────────────┼──────────────┐
                          │                  │              │
                          ▼                  ▼              ▼
                     CodeAnalyzer      BuildMonitor   HealthMonitor
                     (send_message)    (send_message) (send_message)
                                                           │
                                                      ┌────┴────┐
                                                      ▼         ▼
                                                 list_tasks  list_tasks
                                                (all agents) (all agents)
```

Each agent exercises different SDK capabilities:

| Agent | Transport | Capabilities Exercised |
|---|---|---|
| **CodeAnalyzer** | JSON-RPC | Streaming artifacts with append mode, multi-part output (text + JSON data), cancellation token checking |
| **BuildMonitor** | REST | Full task lifecycle (Completed/Failed/Canceled), streaming phase output, cancel executor override, push notification support |
| **HealthMonitor** | JSON-RPC | Multi-part input (text + data), agent-to-agent discovery via `list_tasks`, push notification support |
| **Coordinator** | REST | A2A client calls to other agents, result aggregation, multi-level orchestration |

## SDK Features Exercised

The agent team exercises **35+ distinct SDK features** in a single run:

- `AgentExecutor` trait (4 implementations)
- `RequestHandlerBuilder` (all options: timeout, queue capacity, max streams, metrics, interceptors)
- `JsonRpcDispatcher` and `RestDispatcher`
- `WebSocketDispatcher` (with `websocket` feature flag)
- `ClientBuilder` (both JSON-RPC and REST protocol bindings)
- Synchronous `SendMessage` and streaming `SendStreamingMessage`
- `EventStream` consumer (SSE event loop)
- `GetTask` and `ListTasks` with pagination and status filtering
- `CancelTask` with custom executor override
- Push notification config CRUD (`set_push_config`, `list_push_configs`)
- `HttpPushSender` delivery with webhook receiver
- `ServerInterceptor` (audit logging + bearer token auth checking)
- Custom `Metrics` observer (request/response/error counting)
- `AgentCard` discovery (both transports)
- Multi-part messages (`Part::text` + `Part::data`)
- Artifact streaming with `append` and `last_chunk` flags
- All `TaskState` transitions (Submitted, Working, Completed, Failed, Canceled)
- `CancellationToken` cooperative checking
- `return_immediately` mode
- Request metadata passthrough
- Context ID continuation across messages
- Concurrent GetTask during active streams
- `SubscribeToTask` resubscribe (both REST and JSON-RPC)
- `boxed_future` and `EventEmitter` helpers
- Concurrent streams on same agent
- `history_length` configuration
- `Part::file_bytes` (binary file content)
- `TenantAwareInMemoryTaskStore` isolation
- `TenantContext::scope` task-local threading
- WebSocket transport (`SendMessage` + streaming) — with `websocket` feature
- Batch JSON-RPC (single, multi, empty, mixed, streaming rejection)
- Real auth rejection (interceptor short-circuit)
- `GetExtendedAgentCard` via JSON-RPC
- `DynamicAgentCardHandler` (runtime-generated cards)
- Agent card HTTP caching (ETag + 304 Not Modified)
- Backpressure / lagged event queue (capacity=2)

## Modular Example Structure

```
examples/agent-team/src/
├── main.rs                      # Thin orchestrator (~400 lines)
├── executors/
│   ├── mod.rs                   # Re-exports
│   ├── code_analyzer.rs         # CodeAnalyzer executor
│   ├── build_monitor.rs         # BuildMonitor executor
│   ├── health_monitor.rs        # HealthMonitor executor
│   └── coordinator.rs           # Coordinator executor (A2A client calls)
├── cards.rs                     # Agent card builders
├── helpers.rs                   # Shared helpers (make_send_params, EventEmitter)
├── infrastructure.rs            # Metrics, interceptors, webhook, server setup
└── tests/
    ├── mod.rs                   # TestResult, TestContext
    ├── basic.rs                 # Tests 1-10: core send/stream paths
    ├── lifecycle.rs             # Tests 11-20: orchestration, cancel, agent cards
    ├── edge_cases.rs            # Tests 21-30: errors, concurrency, metrics
    ├── stress.rs                # Tests 31-40: stress, durability, event ordering
    ├── dogfood.rs               # Tests 41-50: SDK gaps, regressions, edge cases
    ├── transport.rs             # Tests 51-58: WebSocket, gRPC, multi-tenancy
    └── coverage_gaps/           # Tests 61-79: modular test suite
        ├── mod.rs                   # Re-exports
        ├── batch_jsonrpc.rs         # Batch JSON-RPC tests
        ├── auth_and_cards.rs        # Auth rejection, extended/dynamic cards, caching
        ├── streaming_backpressure.rs # Backpressure tests
        ├── push_config.rs           # Push config edge cases
        ├── resilience.rs            # Resilience and recovery
        └── feature_gated.rs         # Signing, OTel (feature-gated)
```

## Running the Agent Team

```bash
# Basic run (all output to stdout)
cargo run -p agent-team

# With WebSocket tests (tests 51-52)
cargo run -p agent-team --features websocket

# With structured logging
RUST_LOG=debug cargo run -p agent-team --features tracing

# With all optional features
cargo run -p agent-team --features "websocket,tracing"
```

Expected output:

```
╔══════════════════════════════════════════════════════════════╗
║     A2A Agent Team — Full SDK Dogfood & E2E Test Suite     ║
╚══════════════════════════════════════════════════════════════╝

Agent [CodeAnalyzer]  JSON-RPC on http://127.0.0.1:XXXXX
Agent [BuildMonitor]  REST     on http://127.0.0.1:XXXXX
Agent [HealthMonitor] JSON-RPC on http://127.0.0.1:XXXXX
Agent [Coordinator]   REST     on http://127.0.0.1:XXXXX

...71 tests...

║ Total: 72 | Passed: 72 | Failed: 0 | Time: ~2500ms
```

## Lessons for Your Own Agents

1. **Test all four transports.** JSON-RPC, REST, WebSocket, and gRPC have different serialization and framing paths. A bug in one may not exist in the others.
2. **Test multi-hop flows.** Agent A calling Agent B is different from a client calling Agent A. The interaction patterns surface different bugs.
3. **Test failure paths explicitly.** The agent team tests `TaskState::Failed` and `TaskState::Canceled` alongside `Completed`. Happy-path-only testing misses lifecycle bugs.
4. **Use real metrics and interceptors.** They exercise code paths that exist in the handler but are invisible to pure request/response tests.
5. **Deploy multiple agents simultaneously.** Concurrent servers with different configurations stress connection pooling, port binding, and resource cleanup in ways single-server tests cannot.
6. **Test `return_immediately` mode.** Client config must actually propagate to the server — this was a real bug caught only by dogfooding.
7. **Test tenant isolation.** Multi-tenancy bugs are subtle — same task IDs in different tenants should not collide.

## Open Issues & Future Work — All Resolved

All architecture, ergonomics, observability, performance, and durability issues from passes 1–8 have been resolved (36 bugs across 8 passes). All proposed beyond-spec features have been implemented:

| Feature | Location |
|---|---|
| **OpenTelemetry integration** | `crates/a2a-server/src/otel/` — `OtelMetrics`, `OtelMetricsBuilder`, `init_otlp_pipeline` (`otel` feature) |
| **Connection pooling metrics** | `crates/a2a-server/src/metrics.rs` — `ConnectionPoolStats`, `on_connection_pool_stats` |
| **Hot-reload agent cards** | `crates/a2a-server/src/agent_card/hot_reload.rs` — file polling + SIGHUP reload |
| **Store migration tooling** | `crates/a2a-server/src/store/migration.rs` — `MigrationRunner`, V1–V3 migrations |
| **Per-tenant configuration** | `crates/a2a-server/src/tenant_config.rs` — `PerTenantConfig`, `TenantLimits` |
| **TenantResolver trait** | `crates/a2a-server/src/tenant_resolver.rs` — header, bearer token, path segment strategies |
| **Agent card signing E2E** | `examples/agent-team/src/tests/coverage_gaps.rs` — test 79 (`signing` feature) |

## Sub-pages

- **[Bugs Found & Fixed](./dogfooding-bugs.md)** — All 36 bugs discovered across eight dogfooding passes
- **[Test Coverage Matrix](./dogfooding-tests.md)** — Complete 71-test E2E coverage map (78 with optional transports, signing, and OTel)

## See Also

- **[Testing Your Agent](./testing.md)** — Unit and integration testing patterns
- **[Production Hardening](./production.md)** — Preparing for deployment
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common mistakes
