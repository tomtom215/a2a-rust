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

## Resolved in Pass 5 (Hardening)

The following issues were found and fixed in the fifth dogfood pass:

| Issue | Category | Resolution |
|---|---|---|
| **Lock poisoning in `InMemoryCredentialsStore`** | Durability | Changed from silent `.ok()?` to `.expect()` (fail-fast). Poisoned locks now surface immediately instead of masking failures. |
| **Rate limiter TOCTOU race on window reset** | Correctness | Replaced non-atomic window-start + count reset with a CAS loop. Two concurrent threads can no longer both reset the counter to 1 on window advance. |
| **Rate limiter unbounded bucket growth** | Scalability | Added amortized stale-bucket cleanup (every 256 checks). Buckets from departed callers are evicted when their window is >1 window old. |
| **Missing protocol version validation** | Compatibility | `ClientBuilder::from_card()` now warns (via `tracing`) when the agent advertises a major version different from the client's supported version. |
| **Clippy `is_multiple_of` lint** | Code quality | Replaced manual `count % N == 0` with `count.is_multiple_of(N)`. |

### New Tests Added (Pass 5)

| Crate | Test | Description |
|---|---|---|
| `a2a-client` | `credentials_store_multiple_sessions` | Multi-session isolation |
| `a2a-client` | `credentials_store_multiple_schemes` | Multiple auth schemes per session |
| `a2a-client` | `credentials_store_overwrite` | Credential replacement semantics |
| `a2a-client` | `credentials_store_debug_hides_values` | Debug output security |
| `a2a-client` | `auth_interceptor_basic_scheme` | Basic auth header format |
| `a2a-client` | `auth_interceptor_custom_scheme` | Custom scheme raw credential |
| `a2a-client` | `session_id_display` | Display impl |
| `a2a-client` | `session_id_from_string` | From conversions |
| `a2a-client` | `builder_zero_request_timeout_errors` | Validation |
| `a2a-client` | `builder_zero_stream_timeout_errors` | Validation |
| `a2a-client` | `builder_unknown_binding_errors` | Validation |
| `a2a-client` | `builder_from_card_empty_interfaces` | Edge case |
| `a2a-client` | `builder_with_return_immediately` | Config propagation |
| `a2a-client` | `builder_with_history_length` | Config propagation |
| `a2a-server` | `builder_defaults_build_ok` | Default build |
| `a2a-server` | `builder_zero_executor_timeout_errors` | Validation |
| `a2a-server` | `builder_empty_agent_card_interfaces_errors` | Validation |
| `a2a-server` | `builder_with_all_options` | Full builder chain |
| `a2a-server` | `builder_debug_does_not_panic` | Debug safety |
| `a2a-server` | `concurrent_rate_limit_checks` | Concurrent correctness |
| `a2a-server` | `stale_bucket_cleanup` | Eviction correctness |

## Potential Future Work

Features that are not part of the current A2A v1.0.0 spec but could add value:

| Feature | Description | Effort |
|---|---|---|
| **OpenTelemetry integration** | Native OTLP export for traces and metrics instead of the callback-based `Metrics` trait. | Medium |
| **Connection pooling metrics** | Expose hyper connection pool stats (active/idle connections) via the `Metrics` trait. | Small |
| **Hot-reload agent cards** | File-watch or signal-based agent card reload without server restart. | Small |
| **Store migration tooling** | Schema versioning and migration support for `SqliteTaskStore`. | Medium |

## Resolved in Previous Passes

All issues from dogfooding passes 1–5 have been resolved:

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
- Lock poisoning fail-fast in `InMemoryCredentialsStore`
- Rate limiter TOCTOU race condition (CAS loop fix)
- Rate limiter stale-bucket cleanup (unbounded growth prevention)
- Client protocol version compatibility warning

See [Bugs Found & Fixed](./dogfooding-bugs.md) for details on each resolution.
