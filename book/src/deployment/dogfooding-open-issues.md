# Dogfooding: Open Issues & Future Work

Issues identified during dogfooding that have not yet been fixed. Organized by severity and category.

## Architecture Issues

### ~~Push Delivery Broken for Streaming Mode~~ ✅ RESOLVED

~~**Severity:** High | **Effort:** Large~~

Fixed by spawning a background event processor (`spawn_background_event_processor`) that subscribes independently to the broadcast channel. Push notifications now fire for every event regardless of consumer mode (streaming, sync, or `return_immediately`). Stores were also changed from `Box<dyn T>` to `Arc<dyn T>` to enable cloning into the spawned task.

### ~~`CallContext` Lacks HTTP Headers~~ ✅ RESOLVED

~~**Severity:** Medium | **Effort:** Medium~~

Fixed by adding `http_headers: HashMap<String, String>` to `CallContext`. Both `JsonRpcDispatcher` and `RestDispatcher` now extract headers from the incoming `hyper::HeaderMap` before consuming the request body and pass them through all handler methods. Interceptors can now inspect `Authorization`, `X-Request-Id`, and other headers.

### ~~`PartContent` Untagged Enum May Misparse~~ ✅ RESOLVED

~~**Severity:** Low | **Effort:** Medium (breaking change)~~

**Breaking change:** `PartContent` now uses `#[serde(tag = "type")]` with variant renames (`"text"`, `"file"`, `"data"`) per A2A spec. The old `Raw` and `Url` variants were merged into `File` with a new `FileContent` struct containing `name`, `mime_type`, `bytes`, and `uri` fields. Backward-compatible `Part::raw()` and `Part::url()` constructors delegate to the new `Part::file_bytes()` and `Part::file_uri()` methods.

## Ergonomics Issues

### ~~`AgentExecutor` Boilerplate~~ ✅ RESOLVED

~~**Severity:** Medium | **Effort:** Small~~

Fixed by adding two ergonomic helpers in `executor_helpers`:
- `boxed_future(async move { ... })` — wraps an async block into `Pin<Box<dyn Future>>`, eliminating the `Box::pin()` wrapper.
- `agent_executor!(MyAgent, |ctx, queue| async { ... })` — macro that generates the full `AgentExecutor` impl from a closure-like syntax. Supports both execute-only and execute+cancel forms.

### ~~`Arc<T: Metrics>` Doesn't Impl `Metrics`~~ ✅ RESOLVED

~~**Severity:** Low | **Effort:** Trivial~~

Fixed by adding blanket impl: `impl<T: Metrics + ?Sized> Metrics for Arc<T>`. The `MetricsForward` wrapper is no longer needed.

## Observability Gaps

### ~~No `Metrics::on_latency`~~ ✅ RESOLVED

~~**Severity:** Medium | **Effort:** Small~~

Fixed by adding `on_latency(&self, method: &str, duration: Duration)` to the `Metrics` trait with a default no-op implementation. All handler methods now measure elapsed time via `Instant::now()` and report it through this callback.

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

### ~~No Persistent Store Implementations~~ ✅ RESOLVED

~~**Severity:** Medium | **Effort:** Large~~

Fixed by adding `SqliteTaskStore` and `SqlitePushConfigStore` behind the `sqlite` feature flag. Uses `sqlx` for async SQLite access with schema auto-creation, cursor-based pagination, upsert support, and 12 integration tests using in-memory SQLite.

## Remaining Hardcoded Constants

These use sensible defaults but are not yet user-configurable via builder methods:

| Constant | Value | Location |
|---|---|---|
| `EVICTION_INTERVAL` | 64 writes between eviction sweeps | `InMemoryTaskStore` |
| `MAX_PAGE_SIZE` | 1000 tasks per page | `InMemoryTaskStore` |
| `MAX_PUSH_CONFIGS_PER_TASK` | 100 configs per task | `InMemoryPushConfigStore` |
| `DEFAULT_WRITE_TIMEOUT` | 5 seconds | SSE `SseBodyWriter` |
| `DEFAULT_KEEP_ALIVE` | 30 seconds | SSE keep-alive interval |

## Design Issues (Pass 4)

### No Server Startup Helper

**Severity:** Medium | **Effort:** Small

Users must manually wire `TcpListener` → `hyper::service_fn` → `hyper_util::server::conn::auto::Builder` for every agent. This is ~25 lines of identical boilerplate per server.

**Recommendation:** Provide `a2a_protocol_server::serve(listener, handler)` or a builder pattern like `ServerBuilder::new(handler).bind("127.0.0.1:0").serve()`.

### No Request ID / Trace Context Propagation

**Severity:** Medium | **Effort:** Medium

`CallContext` has `http_headers` but no structured trace ID field. Interceptors can extract `X-Request-ID` manually, but there's no first-class support for distributed tracing across agent-to-agent calls.

**Recommendation:** Add `request_id: Option<String>` to `CallContext` and propagate it through `RequestContext` so executors can include it in outbound A2A calls.

### Client Has No Retry Logic

**Severity:** Medium | **Effort:** Small

`a2a-protocol-client` is single-attempt. Any transient network error causes immediate failure.

**Recommendation:** Add `ClientBuilder::with_retry_policy(RetryPolicy)` with configurable max attempts and backoff strategy.

### No Connection Pooling in Coordinator Pattern

**Severity:** Low | **Effort:** Small

Each `delegate_*` call in orchestrator agents creates a new `ClientBuilder::new(url).build()`. The hyper client inside does pool connections, but the client/builder overhead is repeated unnecessarily.

**Recommendation:** Document the pattern of creating clients once and reusing them. Consider adding `A2aClient::clone()` support.

### Executor Event Emission Boilerplate

**Severity:** Low | **Effort:** Small

Every executor must repeat `task_id.clone()`, `ContextId::new(ctx.context_id.clone())`, `metadata: None` for every event. A 4-line status update becomes a 9-line struct literal.

**Recommendation:** Upstream the `EventEmitter` pattern from the agent-team example into `a2a_protocol_server::executor_helpers`.

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

1. ~~**Push delivery architecture**~~ ✅ Done
2. ~~**`CallContext` + HTTP headers**~~ ✅ Done
3. ~~**`Metrics::on_latency`**~~ ✅ Done
4. ~~**Persistent store reference impl**~~ ✅ Done
5. ~~**`AgentExecutor` ergonomics**~~ ✅ Done
6. **Remaining hardcoded constants** — Low severity, sensible defaults exist
7. ~~**`PartContent` tagged enum**~~ ✅ Done
8. ~~**Blanket `impl Metrics for Arc<T>`**~~ ✅ Done
