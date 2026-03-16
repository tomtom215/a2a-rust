# Dogfooding: The Agent Team Example

The best way to find bugs in an SDK is to use it yourself — under real conditions, with real complexity, exercising real interaction patterns. Unit tests verify individual functions. Integration tests verify pairwise contracts. But only dogfooding reveals the emergent issues that appear when all the pieces come together.

The `agent-team` example (`examples/agent-team/`) is a full-stack dogfood of every a2a-rust capability. It deploys 4 specialized agents that discover each other, delegate work, stream results, and report health — all via the A2A protocol.

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
│              (30 tests, ~1500ms total)                      │
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

The agent team exercises **27 distinct SDK features** in a single run:

- `AgentExecutor` trait (4 implementations)
- `RequestHandlerBuilder` (all options: timeout, queue capacity, max streams, metrics, interceptors)
- `JsonRpcDispatcher` and `RestDispatcher`
- `ClientBuilder` (both JSON-RPC and REST protocol bindings)
- Synchronous `SendMessage` and streaming `SendStreamingMessage`
- `EventStream` consumer (SSE event loop)
- `GetTask` and `ListTasks` with pagination
- `CancelTask` with custom executor override
- Push notification config CRUD (`set_push_config`, `list_push_configs`)
- `HttpPushSender` delivery with webhook receiver
- `ServerInterceptor` (audit logging + bearer token auth checking)
- Custom `Metrics` observer (request/response/error counting)
- `AgentCard` discovery
- Multi-part messages (`Part::text` + `Part::data`)
- Artifact streaming with `append` and `last_chunk` flags
- All `TaskState` transitions (Submitted, Working, Completed, Failed, Canceled)
- `CancellationToken` cooperative checking
- Request metadata passthrough

## What Dogfooding Found (and Fixed)

### First Dogfooding Pass

The first dogfooding pass uncovered **three real issues** that 500+ unit tests, integration tests, property tests, and fuzz tests did not catch. All three were fixed.

### Bug 1: REST Transport Strips Required Fields from Push Config Body (**Fixed**)

**Severity:** Medium — broke push notification config via REST transport.

The client's REST transport extracts path parameters from the serialized JSON body to interpolate URL templates. For `CreateTaskPushNotificationConfig`, the route is `/tasks/{taskId}/pushNotificationConfigs`, so the transport extracts `taskId` from the body and *removes it*. But the server-side handler deserializes the body as a full `TaskPushNotificationConfig` — which requires `taskId`. The request failed with HTTP 400:

```
missing field `taskId` at line 1 column 135
```

**Why unit tests missed it:** Unit tests test the client transport and server dispatch in isolation. The client correctly builds the URL. The server correctly parses valid bodies. The bug only appears when they interact — the client sends a body the server can't parse.

**Fix:** Server-side `handle_set_push_config` now injects `taskId` from the URL path parameter into the deserialized JSON body before parsing `TaskPushNotificationConfig`. The `inject_field_if_missing` helper is reusable for any REST endpoint where path params overlap with body fields. Test 8 now uses REST transport directly.

### Bug 2: `on_response` Metrics Hook Never Called (**Fixed**)

**Severity:** Low — metrics observers never saw successful response events.

The `Metrics::on_response` callback registered via `RequestHandlerBuilder::with_metrics()` showed 0 responses even after 17 successful requests across all agents. The `on_request` hook only fired for `SendMessage`/`SendStreamingMessage`, and `on_response` was never called in any handler method.

**Why unit tests missed it:** Metrics tests verify that the trait compiles and that `NoopMetrics` doesn't panic — but don't verify that the handler actually calls `on_response` at the right point in the request lifecycle.

**Fix:** Added `self.metrics.on_request()` and `self.metrics.on_response()` calls to all handler methods (`on_get_task`, `on_list_tasks`, `on_cancel_task`, `on_resubscribe`, `on_set_push_config`, `on_get_push_config`, `on_list_push_configs`, `on_delete_push_config`, `on_get_extended_agent_card`). The metrics summary now shows non-zero response counts.

### Finding 3: Protocol Binding Mismatch Produces Confusing Errors (**Fixed**)

**Severity:** Low — developer experience issue.

When the HealthMonitor (using a default JSON-RPC client) called `list_tasks` on the BuildMonitor (a REST-only server), the request failed with an opaque connection/parsing error rather than a clear "protocol binding mismatch" message. The health check reported "DEGRADED" instead of explaining that the client was speaking the wrong protocol.

**Why tests missed it:** Tests use matched client/server pairs. In a real multi-agent deployment, agents discover each other dynamically and may not know which transport to use without consulting the agent card first.

**Fix:** Three changes: (1) The HealthMonitor now fetches the agent card via `resolve_agent_card()` before health-checking, and uses the card's `protocol_binding` to build the correct client. All agents now report HEALTHY. (2) A new `ClientError::ProtocolBindingMismatch` variant provides a clear diagnostic when a JSON-RPC client receives a non-JSON-RPC response. (3) The JSON-RPC transport detects non-JSON-RPC responses and returns the new error variant with a hint to check the agent card.

### Second Dogfooding Pass

A comprehensive second audit uncovered **four more bugs** and several configuration gaps. All were fixed.

### Bug 4: `list_push_configs` REST Response Format Mismatch (**Fixed**)

**Severity:** Medium — broke client deserialization of push config lists.

Both REST and JSON-RPC dispatchers serialized `on_list_push_configs` results as a raw `Vec<TaskPushNotificationConfig>` (JSON array), but the client expected `ListPushConfigsResponse { configs, next_page_token }` (JSON object). Error: `invalid type: map, expected a sequence`.

**Fix:** Both dispatchers now wrap the result in `ListPushConfigsResponse`.

### Bug 5: Push Notification Test Task ID Mismatch (**Fixed**)

**Severity:** Medium — Test 8 always reported "Push notifications received: 0".

Push config was registered on Task A, but the test's second `send_message` created Task B with a new UUID. `deliver_push` looks up configs by task_id — no config existed for Task B.

**Fix:** Restructured Test 8 as a push config CRUD lifecycle test (create→get→list→delete→verify) via REST transport.

### Bug 6: `on_error` Metrics Hook Never Fired (**Fixed**)

**Severity:** Low — error metrics were always zero.

`on_error` was defined on the `Metrics` trait but never called. All handler error paths used `?` to propagate without invoking the metrics hook.

**Fix:** Restructured all 10 handler methods to wrap the body in an async block, then call `on_response` or `on_error` based on the outcome.

### Bug 7: `on_queue_depth_change` Metrics Hook Never Fired (**Fixed**)

**Severity:** Low — queue depth metrics were always zero.

`EventQueueManager` had no access to the `Metrics` object.

**Fix:** Added `Arc<dyn Metrics>` to `EventQueueManager` (passed from the builder). Calls `on_queue_depth_change` in `get_or_create` and `destroy`.

### Bug 8: `JsonRpcDispatcher` Does Not Serve Agent Cards (**Fixed**)

**Severity:** Medium — agent card discovery only worked on REST endpoints.

`RestDispatcher` serves `GET /.well-known/agent.json` but `JsonRpcDispatcher` did not, so `resolve_agent_card()` failed for JSON-RPC agents. The spec requires agent card discovery regardless of transport.

**Fix:** Added `StaticAgentCardHandler` to `JsonRpcDispatcher`. GET requests to `/.well-known/agent.json` are now handled before falling through to JSON-RPC body parsing. Test 18 verifies agent card discovery on JSON-RPC endpoints.

### Bug 9: `SubscribeToTask` Fails When Another SSE Stream Is Active (**Fixed**)

**Severity:** High — resubscription was fundamentally broken.

`EventQueueManager` used `mpsc` channels which allow only a single reader. Once `stream_message` took the reader, `subscribe_to_task` for the same task returned "no active event queue for task".

**Fix:** Redesigned `EventQueueManager` from `mpsc` to `tokio::sync::broadcast` channels. `subscribe()` creates additional readers from the same sender. Slow readers get `Lagged` notifications instead of blocking the writer. Test 28 verifies concurrent resubscription.

### Configuration Hardening

Extracted all hardcoded constants into configurable structs:

- **`DispatchConfig`**: request body size, read timeout, query string length
- **`PushRetryPolicy`**: max attempts, backoff schedule
- **`HandlerLimits`**: ID length, metadata size, cancellation token limits

Aligned client `DEFAULT_MAX_EVENT_SIZE` from 4 MiB to 16 MiB to match the server default.

### Modular Example Structure

The agent-team example follows best-practice Rust module organization (all files under 500 lines):

```
examples/agent-team/src/
├── main.rs                      # Thin orchestrator (~280 lines)
├── executors/
│   ├── mod.rs                   # Re-exports
│   ├── code_analyzer.rs         # CodeAnalyzer executor
│   ├── build_monitor.rs         # BuildMonitor executor
│   ├── health_monitor.rs        # HealthMonitor executor
│   └── coordinator.rs           # Coordinator executor (A2A client calls)
├── cards.rs                     # Agent card builders
├── helpers.rs                   # Shared helpers (make_send_params)
├── infrastructure.rs            # Metrics, interceptors, webhook, server setup
└── tests/
    ├── mod.rs                   # TestResult, TestContext
    ├── basic.rs                 # Tests 1-10: core send/stream paths
    ├── lifecycle.rs             # Tests 11-20: orchestration, cancel, agent cards
    └── edge_cases.rs            # Tests 21-30: errors, concurrency, metrics
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
║     A2A Agent Team — Full SDK Dogfood & E2E Test Suite       ║
╚══════════════════════════════════════════════════════════════╝

Agent [CodeAnalyzer]  JSON-RPC on http://127.0.0.1:XXXXX
Agent [BuildMonitor]  REST     on http://127.0.0.1:XXXXX
Agent [HealthMonitor] JSON-RPC on http://127.0.0.1:XXXXX
Agent [Coordinator]   REST     on http://127.0.0.1:XXXXX

...30 tests...

║ Total: 30 | Passed: 30 | Failed: 0 | Time: ~1500ms
```

## Lessons for Your Own Agents

1. **Test both transports.** JSON-RPC and REST have different serialization paths. A bug in one may not exist in the other.
2. **Test multi-hop flows.** Agent A calling Agent B is different from a client calling Agent A. The interaction patterns surface different bugs.
3. **Test failure paths explicitly.** The agent team tests `TaskState::Failed` and `TaskState::Canceled` alongside `Completed`. Happy-path-only testing misses lifecycle bugs.
4. **Use real metrics and interceptors.** They exercise code paths that exist in the handler but are invisible to pure request/response tests.
5. **Deploy multiple agents simultaneously.** Concurrent servers with different configurations stress connection pooling, port binding, and resource cleanup in ways single-server tests cannot.

## Next Steps

- **[Testing Your Agent](./testing.md)** — Unit and integration testing patterns
- **[Production Hardening](./production.md)** — Preparing for deployment
- **[Pitfalls & Lessons Learned](../reference/pitfalls.md)** — Common mistakes
