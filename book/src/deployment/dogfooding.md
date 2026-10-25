# Dogfooding: The Agent Team Example

The best way to find bugs in an SDK is to use it yourself — under real conditions, with real complexity, exercising real interaction patterns. Unit tests verify individual functions. Integration tests verify pairwise contracts. But only dogfooding reveals the emergent issues that appear when all the pieces come together.

The `agent-team` example (`examples/agent-team/`) is a full-stack dogfood of the SDK's protocol surface and its main server and client features. It deploys four specialized agents that discover each other, delegate work, stream results, and report health — all via the A2A protocol — plus a fifth, GrpcAnalyzer, that serves the CodeAnalyzer executor over gRPC, and a sixth that serves all four bindings for the surface sweep. The E2E suite is **100 tests** and runs in ~6 seconds; the run's total of **102** adds the two results of the method × binding surface sweep (`surface-counter` and `surface-sweep`). That is the figure for a plain `cargo run -p agent-team`, because the six optional features — WebSocket, gRPC, Axum, SQLite, signing and OTel — are the example's `default` set; `examples/agent-team/Cargo.toml` says why, and `--all-features` adds only `tracing`, which changes output volume rather than coverage, so it is 102 as well. Compile the six out with `--no-default-features` and **87** run, with no sweep (it needs `websocket` and `grpc` together). The 102 and the 87 are the `Total:` the binary prints; the 100 is the number of tests `main.rs` registers, 13 of them behind the six features.

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

**The critical lesson:** At v0.3.0 the suite held about 1,630 tests across the categories above — unit, integration, property and fuzz (about 1,850 with all feature flags) — and a 94-test E2E dogfood suite whose 13 documented passes found 68 real bugs — **the entire suite was green.** Every CI check passed. Mutation testing, adopted in that release, still found gaps in every crate. Tests that *looked* comprehensive were silently missing assertions on return values, boundary conditions, delegation correctness, and hash function specifics.

Mutation testing fills the gap between "tests pass" and "tests actually detect bugs." A test suite with 100% line coverage can still have a 0% mutation score if every assertion is trivial. Mutation testing is the only technique that directly measures *test effectiveness* rather than test *existence*. See **[Testing Your Agent — Mutation Testing](./testing.md#mutation-testing)** for setup and usage.

Dogfooding operates at the highest level of the testing pyramid. It catches the class of bugs that live in the seams between components — bugs that only manifest when a real application exercises the full stack in realistic patterns. But even dogfooding cannot verify that your *assertions* are strong enough to catch regressions — that's where mutation testing completes the picture.

## The Agent Team Architecture

```text
┌─────────────────────────────────────────────────────────────┐
│                     E2E Test Harness                        │
│    (100 tests + 2 sweep = 102 / 87 no-default, ~5500ms)     │
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

The agent team exercises **45+ distinct SDK features** in a single run:

- `AgentExecutor` trait (4 implementations)
- `RequestHandlerBuilder` (timeout, queue capacity, max streams, metrics, interceptors, push sender, extended card)
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
- gRPC transport (`SendMessage`, streaming, `GetTask`) — with `grpc` feature
- Batch JSON-RPC (single, multi, empty, mixed, streaming rejection)
- Real auth rejection (interceptor short-circuit)
- `GetExtendedAgentCard` via JSON-RPC
- `DynamicAgentCardHandler` (runtime-generated cards)
- Agent card HTTP caching (ETag + 304 Not Modified)
- Backpressure / lagged event queue (capacity=2)
- State transition validation (streaming)
- Executor error → Failed propagation
- Streaming event completeness verification
- Oversized metadata rejection
- Artifact content correctness
- Rapid sequential request throughput
- `A2aRouter` (Axum framework integration) — with `axum` feature
- `SqliteTaskStore` (persistent task store) — with `sqlite` feature
- `SqlitePushConfigStore` (persistent push config) — with `sqlite` feature
- Combined Axum + SQLite production stack — with `axum,sqlite` features
- `OtelMetrics` wired into a handler — with `otel` feature

## Modular Example Structure

```text
examples/agent-team/src/
├── main.rs                      # Thin orchestrator (~500 lines)
├── features.rs                  # Feature-claim table and its audit against the run
├── features/tests.rs            # Tests of the claim-table audit
├── surface.rs                   # Every method × every binding matrix
├── telemetry.rs                 # SDK telemetry (otel) and log output (tracing)
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
    ├── registration.rs          # Every test_* is registered and claimed
    └── coverage_gaps/           # Tests 61-98: modular test suite
        ├── mod.rs                   # Re-exports
        ├── batch_jsonrpc.rs         # Batch JSON-RPC tests
        ├── auth_and_cards.rs        # Auth rejection, extended/dynamic cards, caching
        ├── streaming_backpressure.rs # Backpressure tests
        ├── push_config.rs           # Push config edge cases
        ├── resilience.rs            # Resilience and recovery
        ├── deep_dogfood.rs          # Deep probes: state transitions, errors, scale
        ├── axum_sqlite.rs           # Axum + SQLite integration (feature-gated)
        └── feature_gated.rs         # Signing, OTel (feature-gated)
```

## Running the Agent Team

```bash
# The full suite — 100 tests, 102 results with the surface sweep. WebSocket,
# gRPC, Axum, SQLite, signing and OTel are the example's DEFAULT features, so
# this is not a reduced run.
cargo run -p agent-team

# Identical: --all-features adds only `tracing`, which changes output volume
# and not coverage. Still 100 tests, 102 results.
cargo run -p agent-team --all-features

# All six compiled out — 87 tests, no sweep. Exits 2, because a narrowed
# build leaves claimed feature areas unexercised, and that is reported rather
# than hidden.
cargo run -p agent-team --no-default-features

# One area at a time on top of that narrowed build:
cargo run -p agent-team --no-default-features --features axum,sqlite   # tests 93-98
cargo run -p agent-team --no-default-features --features websocket     # tests 51-52
cargo run -p agent-team --no-default-features --features grpc          # tests 56-58 (protoc is vendored; nothing to install)

# With structured logging. `tracing` is the only non-default feature, and the
# count is unchanged.
RUST_LOG=debug cargo run -p agent-team --features tracing
```

Expected output:

```text
╔══════════════════════════════════════════════════════════════╗
║     A2A Agent Team — Full SDK Dogfood & E2E Test Suite       ║
╚══════════════════════════════════════════════════════════════╝

Webhook receiver listening on http://127.0.0.1:XXXXX

Agent [CodeAnalyzer]  JSON-RPC on http://127.0.0.1:XXXXX
Agent [BuildMonitor]  REST     on http://127.0.0.1:XXXXX
Agent [HealthMonitor] JSON-RPC on http://127.0.0.1:XXXXX
Agent [Coordinator]   REST     on http://127.0.0.1:XXXXX
Agent [GrpcAnalyzer]  gRPC     on 127.0.0.1:XXXXX

...100 tests, then the surface sweep...

║ Total: 102 | Passed: 102 | Failed: 0 | Time: ~5500ms

...

All 102 tests passed in ~5500ms.
Every claimed SDK feature was exercised. SDK dogfood complete.
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

All architecture, ergonomics, observability, performance, and durability issues from the documented passes have been resolved (68 bugs across 13 documented passes, all fixed). Additional scale and security hardening items are documented in the [CHANGELOG](https://github.com/tomtom215/a2a-rust/blob/main/CHANGELOG.md) as known limitations for future releases. All proposed beyond-spec features have been implemented:

| Feature | Location |
|---|---|
| **OpenTelemetry integration** | `crates/a2a-protocol-server/src/otel/` — `OtelMetrics`, `OtelMetricsBuilder` (`otel` feature), `init_otlp_pipeline` (`otlp` feature) |
| **Connection pooling metrics** | `crates/a2a-protocol-server/src/metrics/mod.rs` — `ConnectionPoolStats`, `on_connection_pool_stats` |
| **Hot-reload agent cards** | `crates/a2a-protocol-server/src/agent_card/hot_reload.rs` — file polling + SIGHUP reload |
| **Store migration tooling** | `crates/a2a-protocol-server/src/store/migration/` — `MigrationRunner`, V1–V8 migrations |
| **Per-tenant configuration** | `crates/a2a-protocol-server/src/tenant_config/mod.rs` — `PerTenantConfig`, `TenantLimits` |
| **TenantResolver trait** | `crates/a2a-protocol-server/src/tenant_resolver.rs` — header, bearer token, path segment strategies |
| **Agent card signing E2E** | `examples/agent-team/src/tests/coverage_gaps/feature_gated.rs` — test 79 (`signing` feature) |

## Sub-pages

- **[Bugs Found & Fixed](./dogfooding-bugs.md)** — All 68 bugs discovered across thirteen dogfooding passes
- **[Test Coverage Matrix](./dogfooding-tests.md)** — Complete E2E coverage map — 100 tests plus the surface sweep (102 results) on the default feature set, 87 with `--no-default-features`

## See Also

- **[Testing Your Agent](./testing.md)** — Unit and integration testing patterns
- **[Production Hardening](./production.md)** — Preparing for deployment
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common mistakes
