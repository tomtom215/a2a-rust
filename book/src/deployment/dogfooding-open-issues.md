# Dogfooding: Open Issues & Future Work

Issues identified during dogfooding that have not yet been fixed. Organized by severity and category.

## Architecture Issues

### Push Delivery Broken for Streaming Mode

**Severity:** High | **Effort:** Large

`deliver_push()` only runs inside `collect_events()`, which only executes for synchronous, non-`return_immediately` requests. For streaming requests, events flow through `build_sse_response` which never calls `deliver_push`. This means:

- Push configs set **before** a streaming task starts: no delivery
- Push configs set **during** a streaming task: no delivery
- Push configs set inline via `SendMessageConfiguration.task_push_notification_config`: delivery only for sync mode

The agent-team test suite consistently shows "Push notifications received: 0" despite correct CRUD operations.

**Recommended fix:** Decouple push delivery from the response consumer. When an event is written to the queue, check for push configs asynchronously and deliver regardless of whether the consumer is SSE, sync polling, or `return_immediately`.

### `CallContext` Lacks HTTP Headers

**Severity:** Medium | **Effort:** Medium

`CallContext` is constructed without HTTP request headers. `ServerInterceptor::before()` always receives `caller: None`. This blocks any real authentication story — interceptors can't extract bearer tokens, API keys, or request IDs from headers.

**Recommended fix:** Add `headers: http::HeaderMap` field to `CallContext`. Populate it in both `JsonRpcDispatcher` and `RestDispatcher` before calling interceptors.

### `PartContent` Untagged Enum May Misparse

**Severity:** Low | **Effort:** Medium (breaking change)

`PartContent` uses `#[serde(untagged)]` which tries variants in order. A JSON object with both `text` and `url` fields matches `Text` first, silently dropping `url`. The A2A v1.0 spec uses a `type` discriminator field.

**Recommended fix:** Add `#[serde(tag = "type")]` with `kind` field matching the spec. This is a breaking change to the wire format.

## Ergonomics Issues

### `AgentExecutor` Boilerplate

**Severity:** Medium | **Effort:** Small

Every executor requires:
```rust
fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter)
    -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
    Box::pin(async move { ... })
}
```

This is the single most repeated pattern in any a2a-rust application. Options:
- Provide a proc macro `#[a2a_executor]` that generates the `Pin<Box>` wrapping
- Use RPITIT (Rust 1.75+) for `async fn` in traits — but this may sacrifice object safety
- Provide a helper function: `fn boxed_future<F: Future>(f: F) -> Pin<Box<dyn Future>>`

### `Arc<T: Metrics>` Doesn't Impl `Metrics`

**Severity:** Low | **Effort:** Trivial

Using shared metrics requires a `MetricsForward(Arc<TeamMetrics>)` wrapper because `RequestHandlerBuilder::with_metrics` takes `impl Metrics + Send + Sync + 'static`.

**Recommended fix:** Add blanket impl: `impl<T: Metrics> Metrics for Arc<T>`.

## Observability Gaps

### No `Metrics::on_latency`

**Severity:** Medium | **Effort:** Small

The `Metrics` trait has `on_request`, `on_response`, `on_error`, `on_queue_depth_change` but no `on_latency(method: &str, duration: Duration)`. Request latency is the most important metric for production monitoring.

### No `TaskStore::count()`

**Severity:** Low | **Effort:** Trivial

`InMemoryTaskStore` has no method to query current capacity utilization. Useful for metrics dashboards and capacity planning.

## Performance Issues

### Double Serialization in `EventQueueWriter::write()`

**Severity:** Low | **Effort:** Small

`InMemoryQueueWriter::write()` serializes the event to JSON just to check byte size against the limit, then broadcasts the unserialized `StreamResponse`. The serialized bytes are discarded.

**Recommended fix:** Either cache the serialized form or compute size without full serialization.

### `InMemoryTaskStore` Write Lock Contention

**Severity:** Low | **Effort:** Medium

`save()` acquires a write lock unconditionally, even when no eviction is needed. Under 20 parallel requests this works fine, but at higher concurrency it serializes all task saves.

**Recommended fix:** Read-check-then-write pattern, or sharded `DashMap`.

## Durability Gaps

### No Persistent Store Implementations

**Severity:** Medium | **Effort:** Large

Only `InMemoryTaskStore` and `InMemoryPushConfigStore` exist. All state is lost on restart. The traits are well-designed for custom backends, but no reference implementation exists for SQLite, Redis, or Postgres.

## Remaining Hardcoded Constants

These use sensible defaults but are not yet user-configurable via builder methods:

| Constant | Value | Location |
|---|---|---|
| `EVICTION_INTERVAL` | 64 writes between eviction sweeps | `InMemoryTaskStore` |
| `MAX_PAGE_SIZE` | 1000 tasks per page | `InMemoryTaskStore` |
| `MAX_PUSH_CONFIGS_PER_TASK` | 100 configs per task | `InMemoryPushConfigStore` |
| `DEFAULT_WRITE_TIMEOUT` | 5 seconds | SSE `SseBodyWriter` |
| `DEFAULT_KEEP_ALIVE` | 30 seconds | SSE keep-alive interval |

## Testing Gaps for Future Passes

| Area | What to test | Why it matters |
|---|---|---|
| **Executor timeout** | Slow executor exceeding `with_executor_timeout()` | Verify the task gets `Failed` state, not hung |
| **TLS/mTLS** | Client with `tls-rustls` feature connecting to TLS server | Verify cert validation, SNI, connection errors |
| **Batch JSON-RPC** | Multiple JSON-RPC requests in single HTTP body | Verify batch response assembly |
| **Graceful shutdown** | `handler.shutdown()` + `on_shutdown` hook | Verify in-flight tasks drain, tokens cleaned |
| **Real auth rejection** | Interceptor that actually returns error for bad tokens | Verify 401/403 propagation |
| **Backpressure** | Very slow consumer on broadcast channel | Verify `Lagged` handling, no OOM |
| **Memory under sustained load** | Hundreds of concurrent requests over minutes | Detect leaks in task store, event queues, cancel tokens |
| **Agent card caching** | ETag/Last-Modified/If-None-Match flow | Verify 304 responses, cache invalidation |
| **Multi-tenancy** | Populate `tenant` field, verify isolation | Verify tasks from tenant A invisible to tenant B |

## Priority Order for Future Sessions

1. **Push delivery architecture** — High severity, blocks a core protocol feature
2. **`CallContext` + HTTP headers** — Medium severity, blocks production auth
3. **`Metrics::on_latency`** — Medium severity, blocks production monitoring
4. **Persistent store reference impl** — Medium severity, blocks production deployments
5. **`AgentExecutor` ergonomics** — Medium severity, affects every SDK user
6. **Remaining hardcoded constants** — Low severity, sensible defaults exist
7. **`PartContent` tagged enum** — Low severity but breaking change, better to fix before 1.0
8. **Blanket `impl Metrics for Arc<T>`** — Trivial, do anytime
