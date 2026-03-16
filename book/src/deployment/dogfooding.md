# Dogfooding: The Agent Team Example

The best way to find bugs in an SDK is to use it yourself — under real conditions, with real complexity, exercising real interaction patterns. Unit tests verify individual functions. Integration tests verify pairwise contracts. But only dogfooding reveals the emergent issues that appear when all the pieces come together.

The `agent-team` example (`examples/agent-team/`) is a full-stack dogfood of every a2a-rust capability. It deploys 4 specialized agents that discover each other, delegate work, stream results, and report health — all via the A2A protocol. A comprehensive test suite of **50 E2E tests** runs in ~2 seconds.

## Why Dogfood?

Unit tests and integration tests are necessary but insufficient. Here's what they miss:

| Testing level | What it catches | What it misses |
|---|---|---|
| **Unit tests** | Logic errors in isolated functions | Interaction bugs, serialization mismatches |
| **Integration tests** | Pairwise contracts between components | Multi-hop communication, emergent behavior |
| **Property tests** | Edge cases in data handling | Protocol flow issues, lifecycle bugs |
| **Fuzz tests** | Malformed input handling | Semantic correctness of valid flows |
| **Dogfooding** | All of the above + DX issues, performance surprises, missing features | — |

Dogfooding operates at the highest level of the testing pyramid. It catches the class of bugs that live in the seams between components — bugs that only manifest when a real application exercises the full stack in realistic patterns.

## The Agent Team Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     E2E Test Harness                        │
│              (50 tests, ~2000ms total)                      │
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

The agent team exercises **32 distinct SDK features** in a single run:

- `AgentExecutor` trait (4 implementations)
- `RequestHandlerBuilder` (all options: timeout, queue capacity, max streams, metrics, interceptors)
- `JsonRpcDispatcher` and `RestDispatcher`
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

## Modular Example Structure

```
examples/agent-team/src/
├── main.rs                      # Thin orchestrator (~290 lines)
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
    └── dogfood.rs               # Tests 41-50: SDK gaps, regressions, edge cases
```

## Running the Agent Team

```bash
# Basic run (all output to stdout)
cargo run -p agent-team

# With structured logging
RUST_LOG=debug cargo run -p agent-team --features tracing
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

...50 tests...

║ Total: 50 | Passed: 50 | Failed: 0 | Time: ~2000ms
```

## Lessons for Your Own Agents

1. **Test both transports.** JSON-RPC and REST have different serialization paths. A bug in one may not exist in the other.
2. **Test multi-hop flows.** Agent A calling Agent B is different from a client calling Agent A. The interaction patterns surface different bugs.
3. **Test failure paths explicitly.** The agent team tests `TaskState::Failed` and `TaskState::Canceled` alongside `Completed`. Happy-path-only testing misses lifecycle bugs.
4. **Use real metrics and interceptors.** They exercise code paths that exist in the handler but are invisible to pure request/response tests.
5. **Deploy multiple agents simultaneously.** Concurrent servers with different configurations stress connection pooling, port binding, and resource cleanup in ways single-server tests cannot.
6. **Test `return_immediately` mode.** Client config must actually propagate to the server — this was a real bug caught only by dogfooding.

## Sub-pages

- **[Bugs Found & Fixed](./dogfooding-bugs.md)** — All 13 bugs discovered across four dogfooding passes
- **[Test Coverage Matrix](./dogfooding-tests.md)** — Complete 50-test E2E coverage map
- **[Open Issues & Roadmap](./dogfooding-open-issues.md)** — Known gaps, design debt, and future work

## See Also

- **[Testing Your Agent](./testing.md)** — Unit and integration testing patterns
- **[Production Hardening](./production.md)** — Preparing for deployment
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common mistakes
