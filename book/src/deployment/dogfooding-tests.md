# Dogfooding: Test Coverage Matrix

The agent team runs **82 E2E tests** across 8 test modules (92 with optional WebSocket, gRPC, signing, and OTel features). All tests pass in ~3 seconds.

## Tests 1-10: Core Paths (`basic.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 1 | sync-jsonrpc-send | JSON-RPC | Synchronous `SendMessage`, artifact count |
| 2 | streaming-jsonrpc | JSON-RPC | SSE streaming, event ordering, artifact metadata |
| 3 | sync-rest-send | REST | REST synchronous path |
| 4 | streaming-rest | REST | REST SSE streaming |
| 5 | build-failure-path | REST | `TaskState::Failed` lifecycle |
| 6 | get-task | JSON-RPC | `GetTask` retrieval by ID |
| 7 | list-tasks | JSON-RPC | `ListTasks` with pagination token |
| 8 | push-config-crud | REST | Push config create/get/list/delete/verify lifecycle |
| 9 | multi-part-message | JSON-RPC | Text + data parts, HealthMonitor agent-to-agent |
| 10 | agent-to-agent | REST | Coordinator delegates to CodeAnalyzer |

## Tests 11-20: Lifecycle (`lifecycle.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 11 | full-orchestration | REST | Coordinator -> CodeAnalyzer + BuildMonitor fan-out |
| 12 | health-orchestration | REST | Coordinator -> HealthMonitor -> all agents (3-level) |
| 13 | message-metadata | JSON-RPC | Request metadata passthrough |
| 14 | cancel-task | REST | Mid-stream cancellation via `CancelTask` |
| 15 | get-nonexistent-task | JSON-RPC | Error: `TaskNotFound` (-32001) |
| 16 | pagination-walk | JSON-RPC | `ListTasks` page_size=1, no duplicates |
| 17 | agent-card-discovery | REST | `resolve_agent_card` on REST endpoint |
| 18 | agent-card-jsonrpc | JSON-RPC | `resolve_agent_card` on JSON-RPC endpoint |
| 19 | push-not-supported | JSON-RPC | Error: `NotSupported` (-32003) for no-push agent |
| 20 | cancel-completed | REST | Error: `InvalidState` for completed task cancel |

## Tests 21-30: Edge Cases (`edge_cases.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 21 | cancel-nonexistent | REST | Error: `TaskNotFound` for fake task ID |
| 22 | return-immediately | REST | `return_immediately` client config propagation |
| 23 | concurrent-requests | JSON-RPC | 5 parallel requests to same agent |
| 24 | empty-parts-rejected | JSON-RPC | Validation: empty message parts |
| 25 | get-task-rest | REST | `GetTask` via REST transport |
| 26 | list-tasks-rest | REST | `ListTasks` via REST transport |
| 27 | push-crud-jsonrpc | JSON-RPC | Push config CRUD via JSON-RPC (HealthMonitor) |
| 28 | resubscribe-rest | REST | `SubscribeToTask` concurrent resubscription |
| 29 | metrics-nonzero | — | All 4 agents have non-zero request counts |
| 30 | error-metrics-tracked | JSON-RPC | Error metric increments on invalid request |

## Tests 31-40: Stress & Durability (`stress.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 31 | high-concurrency | JSON-RPC | 20 parallel requests to same agent |
| 32 | mixed-transport | Both | REST + JSON-RPC simultaneously |
| 33 | context-continuation | JSON-RPC | Two messages with same `context_id` |
| 34 | large-payload | JSON-RPC | 64KB text payload processing |
| 35 | stream-with-get-task | REST | `GetTask` during active SSE stream |
| 36 | push-delivery-e2e | REST | Push config set during streaming task |
| 37 | list-status-filter | JSON-RPC | `ListTasks` with `TaskState::Completed` filter |
| 38 | store-durability | JSON-RPC | Create 5 tasks, retrieve all 5 |
| 39 | queue-depth-metrics | — | Cumulative metrics tracking (>20 total requests) |
| 40 | event-ordering | JSON-RPC | Working -> artifacts -> Completed sequence |

## Tests 41-50: SDK Dogfood Regressions (`dogfood.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 41 | card-url-correct | JSON-RPC | Agent card URL matches actual bound address |
| 42 | card-skills-valid | Both | All 4 agent cards have name, description, version, skills, interface |
| 43 | push-list-regression | JSON-RPC | `ListTaskPushNotificationConfigs` via JSON-RPC (regression for bug 11) |
| 44 | push-event-classify | REST | Webhook event classifier uses correct field names |
| 45 | resubscribe-jsonrpc | JSON-RPC | `SubscribeToTask` via JSON-RPC transport |
| 46 | multiple-artifacts | JSON-RPC | Multiple artifacts per task (>=2) |
| 47 | concurrent-streams | REST | 5 parallel SSE streams on same agent |
| 48 | list-context-filter | JSON-RPC | `ListTasks` with `context_id` filter |
| 49 | file-parts | JSON-RPC | `Part::file_bytes` with base64 content |
| 50 | history-length | JSON-RPC | `history_length` configuration via builder |

## Tests 51-58: WebSocket, Multi-Tenancy & gRPC (`transport.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 51 | ws-send-message | WebSocket | JSON-RPC `SendMessage` over WebSocket (feature-gated) |
| 52 | ws-streaming | WebSocket | `SendStreamingMessage` with multi-frame streaming (feature-gated) |
| 53 | tenant-isolation | JSON-RPC | Different tenants cannot see each other's tasks |
| 54 | tenant-id-independence | Direct store | Same task ID in different tenants doesn't collide |
| 55 | tenant-count | Direct store | `TenantAwareInMemoryTaskStore::tenant_count()` tracking |
| 56 | grpc-send-message | gRPC | JSON-RPC `SendMessage` over gRPC transport (feature-gated) |
| 57 | grpc-streaming | gRPC | `SendStreamingMessage` over gRPC transport (feature-gated) |
| 58 | grpc-get-task | gRPC | `GetTask` after `SendMessage` over gRPC (feature-gated) |

> **Note:** Tests 51-52 require the `websocket` feature flag: `cargo run -p agent-team --features websocket`
> Tests 56-58 require the `grpc` feature flag: `cargo run -p agent-team --features grpc`

## Tests 61-79: E2E Coverage Gaps (`coverage_gaps.rs`)

| # | Test | Category | What it exercises |
|---|------|----------|-------------------|
| 61 | batch-single-element | Batch JSON-RPC | Single-element batch `[{...}]` with `SendMessage` |
| 62 | batch-multi-request | Batch JSON-RPC | Multi-request batch: `SendMessage` + `GetTask` |
| 63 | batch-empty | Batch JSON-RPC | Empty batch `[]` returns parse error |
| 64 | batch-mixed | Batch JSON-RPC | Mixed valid/invalid requests in batch |
| 65 | batch-streaming-rejected | Batch JSON-RPC | `SendStreamingMessage` in batch returns error |
| 66 | batch-subscribe-rejected | Batch JSON-RPC | `SubscribeToTask` in batch returns error |
| 67 | real-auth-rejection | Auth | Interceptor rejects unauthenticated requests |
| 68 | extended-agent-card | Cards | `GetExtendedAgentCard` via JSON-RPC |
| 69 | dynamic-agent-card | Cards | `DynamicAgentCardHandler` runtime card generation |
| 70 | agent-card-caching | Caching | ETag, `If-None-Match`, 304 Not Modified |
| 71 | backpressure-lagged | Streaming | Slow reader skips lagged events (capacity=2) |
| 72 | push-global-limit | Push config | Global push config limit enforcement (DoS prevention) |
| 73 | webhook-url-scheme | Push config | Rejects non-HTTP webhook URL schemes (ftp://, file://) |
| 74 | combined-filter | ListTasks | Combined status + context_id filtering |
| 75 | latency-metrics | Metrics | Verifies `on_request()` callback fires |
| 76 | timeout-retryable | Retry | Timeout errors are classified as retryable |
| 77 | concurrent-cancels | Stress | 10 parallel cancel requests on same task |
| 78 | stale-page-token | Pagination | Graceful handling of invalid page tokens |
| 79 | agent-card-signing | Signing | ES256 key generation, JWS sign/verify, tamper detection (`signing` feature) |

## Coverage by SDK Feature

| SDK Feature | Tests exercising it |
|---|---|
| `AgentExecutor` trait | 1-5, 9-14, 22, 31-36, 38, 40, 46, 49, 51-52 |
| JSON-RPC dispatch | 1-2, 6-7, 9, 13, 15-16, 18-19, 23-24, 27, 29-34, 37-43, 45-46, 48-50, 53 |
| REST dispatch | 3-5, 8, 10-12, 14, 17, 20-22, 25-26, 28, 32, 35-36, 44, 47 |
| WebSocket dispatch | 51, 52 |
| SSE streaming | 2, 4, 14, 28, 35-36, 40, 44, 45, 47 |
| WebSocket streaming | 52 |
| `GetTask` | 6, 14, 25, 35 |
| `ListTasks` + pagination | 7, 16, 26, 37, 48, 50 |
| `CancelTask` | 14, 20, 21 |
| Push config CRUD | 8, 19, 27, 36, 43 |
| Agent card discovery | 17, 18, 41, 42 |
| `ServerInterceptor` | All tests (audit interceptor on every agent) |
| `Metrics` hooks | 29, 30, 39 |
| `return_immediately` | 22 |
| `CancellationToken` | 14 |
| Error handling | 15, 19, 20, 21, 24, 30 |
| Multi-agent orchestration | 10, 11, 12 |
| Concurrent requests | 23, 31, 32, 47 |
| `SubscribeToTask` resubscribe | 28, 45 |
| Multiple artifacts | 46 |
| File parts (binary) | 49 |
| History length config | 50 |
| Context ID filtering | 33, 48 |
| Multi-tenancy | 53, 54, 55 |
| `TenantAwareInMemoryTaskStore` | 53, 54, 55 |
| `TenantContext::scope` | 54, 55 |
| Batch JSON-RPC | 61, 62, 63, 64, 65, 66 |
| Auth rejection (interceptor) | 67 |
| `GetExtendedAgentCard` | 68 |
| `DynamicAgentCardHandler` | 69 |
| Agent card HTTP caching (ETag/304) | 70 |
| Agent card signing (JWS/ES256) | 79 |
| Backpressure / `Lagged` events | 71 |
| Push config global limit | 72 |
| Webhook URL scheme validation | 73 |
| Combined status+context filter | 74 |
| `Metrics` callbacks | 29, 30, 39, 75 |
| Timeout retryability (Bug #32) | 76 |
| Concurrent cancel stress | 77 |
| Stale page token handling | 78 |
| Agent card signing (JWS/ES256) | 79 |

## Tests 81-90: Deep Dogfood Probes (`deep_dogfood.rs`)

| # | Test | Transport | What it exercises |
|---|------|-----------|-------------------|
| 81 | state-transition-order | JSON-RPC | Validates streaming state transitions are monotonically forward |
| 82 | executor-error-failed | REST | Executor error produces `Failed` state with error metadata |
| 83 | stream-completeness | JSON-RPC | Working→Artifact→Completed event sequence verified completely |
| 84 | oversized-metadata | JSON-RPC | Rejects messages with metadata exceeding 1 MiB limit |
| 85 | artifact-content | JSON-RPC | Artifact text contains actual analysis, not just present |
| 86 | get-task-history | JSON-RPC | GetTask with `history_length` returns history data |
| 87 | rapid-sequential | JSON-RPC | 30 sequential requests without degradation |
| 88 | cancel-already-failed | REST | Cancel on terminal-state task handled gracefully |
| 89 | card-semantic-valid | JSON-RPC | Agent card fields (name, version, skills, interfaces) validated |
| 90 | get-after-stream | JSON-RPC | GetTask reflects streaming state (documents Bug #38 race) |

## Dedicated Integration Tests (Outside Agent-Team)

In addition to the 82 agent-team E2E tests (92 with optional features), the SDK includes dedicated integration test suites:

| Suite | Location | Tests | What it covers |
|---|---|---|---|
| **TLS/mTLS** | `crates/a2a-client/tests/tls_integration_tests.rs` | 7 | Client cert validation, SNI hostname verification, unknown CA rejection, mutual TLS |
| **WebSocket server** | `crates/a2a-server/tests/websocket_tests.rs` | 7 | Send/stream, error handling, ping/pong, connection reuse, close frames |
| **Memory & load stress** | `crates/a2a-server/tests/stress_tests.rs` | 5 | 200 concurrent requests, sustained load (500 requests/10 waves), eviction under load, multi-tenant isolation (10×50), rapid connect/disconnect |

## Features NOT Covered by E2E Tests

All previously uncovered features now have E2E test coverage. Agent card signing is covered by test 79 (`#[cfg(feature = "signing")]`).
