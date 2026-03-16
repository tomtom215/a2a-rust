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

### ~~No `TaskStore::count()`~~ ✅ RESOLVED

~~**Severity:** Low | **Effort:** Trivial~~

Added `TaskStore::count()` with default implementation returning `0`. Implemented for both `InMemoryTaskStore` (read lock + `HashMap::len()`) and `SqliteTaskStore` (`SELECT COUNT(*)`).

## Performance Issues

### ~~Double Serialization in `EventQueueWriter::write()`~~ ✅ RESOLVED

~~**Severity:** Low | **Effort:** Small~~

Fixed by replacing `serde_json::to_string()` (allocates a `String`) with a zero-allocation `CountingWriter` that counts serialized bytes via `serde_json::to_writer()` without allocating.

### ~~`InMemoryTaskStore` Write Lock Contention~~ ✅ RESOLVED

~~**Severity:** Low | **Effort:** Medium~~

Fixed by decoupling the O(n) eviction sweep from the `save()` write lock. The insert now releases the write lock immediately; eviction runs in a separate lock acquisition with an `AtomicBool` guard to prevent concurrent sweeps.

## Durability Gaps

### ~~No Persistent Store Implementations~~ ✅ RESOLVED

~~**Severity:** Medium | **Effort:** Large~~

Fixed by adding `SqliteTaskStore` and `SqlitePushConfigStore` behind the `sqlite` feature flag. Uses `sqlx` for async SQLite access with schema auto-creation, cursor-based pagination, upsert support, and 12 integration tests using in-memory SQLite.

## ~~Remaining Hardcoded Constants~~ ✅ RESOLVED

All 5 constants are now configurable via builder methods:

| Constant | Default | How to configure |
|---|---|---|
| `EVICTION_INTERVAL` | 64 writes | `TaskStoreConfig { eviction_interval: N, .. }` → `builder.with_task_store_config(config)` |
| `MAX_PAGE_SIZE` | 1000 tasks | `TaskStoreConfig { max_page_size: N, .. }` → `builder.with_task_store_config(config)` |
| `MAX_PUSH_CONFIGS_PER_TASK` | 100 configs | `InMemoryPushConfigStore::with_max_configs_per_task(N)` → `builder.with_push_config_store(store)` |
| `DEFAULT_WRITE_TIMEOUT` | 5 seconds | `builder.with_event_queue_write_timeout(Duration::from_secs(N))` |
| `DEFAULT_KEEP_ALIVE` | 30 seconds | `DispatchConfig::default().with_sse_keep_alive_interval(Duration::from_secs(N))` |

## Design Issues (Pass 4)

### ~~No Server Startup Helper~~ ✅ Done

Added `serve()` and `serve_with_addr()` in `a2a_protocol_server::serve`. Both `JsonRpcDispatcher` and `RestDispatcher` implement the `Dispatcher` trait.

### ~~No Request ID / Trace Context Propagation~~ ✅ Done

Added `CallContext::request_id: Option<String>` — automatically populated from the `X-Request-ID` HTTP header. Also settable via `with_request_id()`.

### ~~Client Has No Retry Logic~~ ✅ Done

Added `RetryPolicy` and `ClientBuilder::with_retry_policy()` with configurable exponential backoff. `ClientError::is_retryable()` classifies transient vs permanent errors.

### No Connection Pooling in Coordinator Pattern

**Severity:** Low | **Effort:** Small

Each `delegate_*` call in orchestrator agents creates a new `ClientBuilder::new(url).build()`. The hyper client inside does pool connections, but the client/builder overhead is repeated unnecessarily.

**Recommendation:** Document the pattern of creating clients once and reusing them. Consider adding `A2aClient::clone()` support.

### ~~Executor Event Emission Boilerplate~~ ✅ Done

Upstreamed `EventEmitter` from the agent-team example to `a2a_protocol_server::executor_helpers`. Reduces 9-line struct literals to one-liners.

## Testing Gaps for Future Passes

| Area | Status | What to test | Why it matters |
|---|---|---|---|
| **Executor timeout** | ✅ Covered | `audit_tests::executor_timeout_causes_failure`, `edge_case_tests::executor_timeout` | Verify the task gets `Failed` state |
| **TLS/mTLS** | ⏳ Open | Client with `tls-rustls` feature connecting to TLS server | Verify cert validation, SNI |
| **Batch JSON-RPC** | ✅ Covered | `jsonrpc_edge_tests` — empty, mixed, single, streaming-in-batch | Verify batch response assembly |
| **Graceful shutdown** | ✅ Covered | `audit_tests` — SlowExecutor with cancellation_token | Verify in-flight tasks drain |
| **Auth rejection** | ✅ Covered | `interceptor_tests::RejectingInterceptor`, `handler_tests` | Verify error propagation |
| **Backpressure** | ✅ Covered | `event_queue_tests::slow_reader_skips_lagged_events` | Verify `Lagged` handling |
| **Memory under sustained load** | ⏳ Open | Hundreds of concurrent requests over minutes | Detect leaks |
| **Agent card caching** | ✅ Covered | `caching.rs` — 13 unit tests (ETag, 304, If-None-Match) | Verify cache behavior |
| **Multi-tenancy** | ✅ Covered | `store_tests::multi_tenant_context_isolation` | Verify tenant data isolation |
| **TaskStore::count()** | ✅ Covered | `store_tests` — 3 new count tests + insert_if_absent | Verify count accuracy |
| **Rate limiting** | ✅ Covered | `rate_limit::tests` — 5 unit tests | Verify per-caller limiting |

## Priority Order for Future Sessions

1. ~~**Push delivery architecture**~~ ✅ Done
2. ~~**`CallContext` + HTTP headers**~~ ✅ Done
3. ~~**`Metrics::on_latency`**~~ ✅ Done
4. ~~**Persistent store reference impl**~~ ✅ Done
5. ~~**`AgentExecutor` ergonomics**~~ ✅ Done
6. **Remaining hardcoded constants** — Low severity, sensible defaults exist
7. ~~**`PartContent` tagged enum**~~ ✅ Done
8. ~~**Blanket `impl Metrics for Arc<T>`**~~ ✅ Done
