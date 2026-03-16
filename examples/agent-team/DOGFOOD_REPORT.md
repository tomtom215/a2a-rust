# A2A Rust SDK — Dogfood Report

Comprehensive findings from building, running, and stress-testing the Agent Team
example against every SDK surface. Issues are categorized by severity.

## Critical Bugs Found & Fixed

### 1. JSON-RPC `ListTaskPushNotificationConfigs` param type mismatch

**File**: `crates/a2a-server/src/dispatch/jsonrpc.rs:318`

The JSON-RPC dispatcher parsed `ListTaskPushNotificationConfigs` params as
`TaskIdParams` (field `id`), but the client sends `ListPushConfigsParams`
(field `task_id`). This caused silent deserialization failure — the list
always returned an error that was swallowed by test code.

**Impact**: Push config listing via JSON-RPC was completely broken.
REST worked because it uses path-based routing.

**Fix**: Changed `parse_params::<TaskIdParams>` → `parse_params::<ListPushConfigsParams>`
and `p.id` → `p.task_id`.

### 2. Agent card URLs set to "http://placeholder"

**File**: `examples/agent-team/src/main.rs`

Agent cards were constructed with `code_analyzer_card("http://placeholder")`
*before* the server bound to a port. The actual address was only known after
`TcpListener::bind()`. This meant `/.well-known/agent.json` served a card
with a URL that didn't match the actual server address.

**Impact**: Any client using agent card discovery would get the wrong URL.
The `HealthMonitorExecutor` worked only because it used the URLs passed via
`TestContext`, not the card's URL.

**Fix**: Introduced `bind_listener()` that pre-binds TCP listeners to get
addresses *before* handler construction. Cards are now built with correct URLs.

### 3. Push notification event classification broken

**File**: `examples/agent-team/src/infrastructure.rs`

The webhook receiver classified events by checking `value.get("status")` and
`value.get("artifact")`, but `StreamResponse` serializes as
`{"statusUpdate": {...}}` / `{"artifactUpdate": {...}}` (camelCase variant names).
All push events were classified as "Unknown".

**Fix**: Check `statusUpdate`/`artifactUpdate`/`task` instead.

## Design Issues & Ergonomic Gaps

### 4. Executor event emission boilerplate

Every executor must repeat `task_id.clone()`, `ContextId::new(ctx.context_id.clone())`,
`metadata: None` for every single event. A 4-line status update becomes a 9-line
struct literal. This is the #1 source of boilerplate in user code.

**Recommendation**: The SDK should provide an `EventEmitter` (or similar) that
caches `task_id` + `context_id` from the `RequestContext`. We built one in
`helpers.rs` as a proof of concept. This should be upstreamed to
`a2a-protocol-server::executor_helpers`.

### 5. `Pin<Box<dyn Future>>` return type

`AgentExecutor::execute()` returns `Pin<Box<dyn Future<...> + Send + 'a>>`.
While `boxed_future()` exists, none of the examples used it. Every executor
had raw `Box::pin(async move { ... })`.

**Recommendation**: Use `boxed_future` in all examples. Consider whether
`async_trait` or a simpler pattern could be adopted now that Rust has
async-fn-in-trait (though object safety constraints may prevent this).

### 6. No server startup helper

Users must manually wire `TcpListener` → `hyper::service_fn` →
`hyper_util::server::conn::auto::Builder` for every agent. This is ~25 lines
of identical boilerplate per server.

**Recommendation**: The SDK should provide `a2a_protocol_server::serve(listener, handler)`
or a builder pattern like `ServerBuilder::new(handler).bind("127.0.0.1:0").serve()`.

### 7. `MetricsForward` wrapper needed for `Arc<T>`

Users who want to share metrics across handlers and test code must create
a forwarding wrapper struct because `RequestHandlerBuilder` takes
`Box<dyn Metrics>` ownership. The `Arc<T>` impl for `Metrics` exists but
doesn't help when the builder consumes it.

**Recommendation**: Accept `Arc<dyn Metrics>` directly, or provide
`.with_metrics_arc()`.

### 8. Agent card URL immutable after handler construction

`RequestHandler.agent_card` is `Option<AgentCard>` (not behind a lock).
Once built, the card cannot be updated. The dispatcher creates a
`StaticAgentCardHandler` at construction time.

For dynamic environments (containers, auto-scaling), users need to either:
- Pre-bind listeners (our fix)
- Use `DynamicAgentCardHandler`

**Recommendation**: Make pre-bind the documented pattern. Consider adding
`RequestHandler::set_agent_card()` behind an `RwLock`.

### 9. No request ID / trace context propagation

`CallContext` has `http_headers` but no structured trace ID field.
Interceptors can extract `X-Request-ID` manually, but there's no
first-class support for distributed tracing across agent-to-agent calls.

**Recommendation**: Add `request_id: Option<String>` to `CallContext` and
propagate it through `RequestContext` so executors can include it in
outbound A2A calls.

### 10. Client has no retry logic

`a2a-protocol-client` is single-attempt. Any transient network error
causes immediate failure. Users must implement retry/backoff manually.

**Recommendation**: Add `ClientBuilder::with_retry_policy(RetryPolicy)` with
configurable max attempts and backoff strategy.

## Test Gaps Found

### 11. Push notification final drain was always 0

`main.rs` called `webhook_receiver.drain()` in the final report, but Test 36
already drained the same receiver. Result: final report always showed 0 events.

**Fix**: Changed to `snapshot()` (non-destructive read).

### 12. No test for `GetExtendedAgentCard`

The `GetExtendedAgentCard` JSON-RPC method is implemented but never exercised
in the example. No test verifies it works end-to-end.

### 13. No test for JSON-RPC batch requests

The JSON-RPC dispatcher supports batch requests (array of requests), but
no test exercises this. Batch mode has different error handling paths.

### 14. No test for body size limit enforcement

`DispatchConfig::max_request_body_size` defaults to 4 MiB, but no test
sends a payload exceeding this to verify the 413 response.

### 15. No test for executor timeout

`with_executor_timeout()` is configured but never triggered. No test
verifies that a timed-out executor produces a Failed status.

### 16. No test for CORS configuration

Both dispatchers support `.with_cors()` but it's not exercised.

## Architectural Observations

### What works well

- **Dual transport parity**: JSON-RPC and REST are fully interchangeable
- **Streaming model**: broadcast-based fan-out is elegant and handles
  concurrent subscribers well
- **Cancellation**: cooperative cancellation via `CancellationToken` is clean
- **Store abstraction**: `TaskStore` trait allows swapping in-memory for SQL
- **Type safety**: Wire types are well-modeled with correct serde annotations
- **Zero unsafe**: Entire SDK has no unsafe code
- **SSRF protection**: `HttpPushSender` blocks private IPs by default

### Potential scaling concerns

- **In-memory task store**: Default store keeps all tasks in a `RwLock<HashMap>`.
  Under high concurrency, the write lock becomes a bottleneck. The amortized
  eviction (every 64 writes) helps but doesn't eliminate contention.

- **Broadcast channel lag**: If an SSE consumer falls behind, events are silently
  dropped (`RecvError::Lagged`). No mechanism to detect or recover from this.

- **No connection pooling in coordinator**: Each `delegate_*` call in the
  coordinator creates a new `ClientBuilder::new(url).build()`. The hyper
  client inside does pool connections, but the client/builder overhead is
  repeated unnecessarily.

- **Cancellation token map**: Stored in `RwLock<HashMap>` with sweep-on-overflow.
  Under sustained high-throughput with many concurrent tasks, the sweep
  operation (which holds a write lock) could cause latency spikes.

## Summary

| Category | Count |
|----------|-------|
| Critical bugs fixed | 3 |
| Design issues identified | 7 |
| Test gaps found | 6 |
| New tests added | 10 |
| Total tests | 50 |
