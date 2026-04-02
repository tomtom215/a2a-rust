# Production Hardening

A checklist and guide for deploying a2a-rust agents in production.

## Security

### HTTPS

Always use HTTPS in production. a2a-rust doesn't bundle TLS — terminate TLS at your reverse proxy (nginx, Caddy, cloud load balancer) or use a TLS library:

```
Client ──HTTPS──→ [nginx/Caddy] ──HTTP──→ [a2a-rust agent]
```

### CORS

Configure CORS for browser-based clients:

```rust
use a2a_protocol_sdk::server::CorsConfig;

// The dispatchers include CORS handling.
// Configure allowed origins for production.
```

### Push Notification Security

The built-in `HttpPushSender` includes:

- **SSRF protection** — Rejects private/loopback IPs at config creation and delivery time (defense-in-depth)
- **Header injection prevention** — Validates credentials for `\r`/`\n` characters
- **HTTPS enforcement** — Optionally require HTTPS webhook URLs
- **Per-request timeout** — Each push delivery HTTP request is capped at 30 seconds

### Path Traversal Protection

The REST dispatcher automatically rejects:
- `..` in path segments
- Percent-encoded `%2E%2E` and `%2e%2e`
- Paths that escape the expected route hierarchy

### Body Size Limits

| Limit | Value | Transport |
|-------|-------|-----------|
| Request body | 4 MiB | REST |
| Query string | 4 KiB | REST |
| Event size | 16 MiB (configurable) | All SSE transports |

## Reliability

### Executor Timeout

Prevent hung tasks from consuming resources forever:

```rust
use std::time::Duration;

RequestHandlerBuilder::new(executor)
    .with_executor_timeout(Duration::from_secs(300))
    .build()
```

### Concurrent Stream Limits

Bound memory usage from SSE connections:

```rust
RequestHandlerBuilder::new(executor)
    .with_max_concurrent_streams(1000)
    .build()
```

### Task Store Limits

Prevent unbounded memory growth:

```rust
use a2a_protocol_sdk::server::TaskStoreConfig;

RequestHandlerBuilder::new(executor)
    .with_task_store_config(TaskStoreConfig {
        max_capacity: Some(100_000),
        task_ttl: Some(Duration::from_secs(3600)),
        eviction_interval: 64,
        max_page_size: 1000,
    })
    .build()
```

Use `TaskStore::count()` for monitoring capacity utilization.

### Graceful Shutdown

Implement `on_shutdown` in your executor for cleanup:

```rust
fn on_shutdown<'a>(&'a self) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
    Box::pin(async move {
        self.db_pool.close().await;
        self.cancel_token.cancel();
    })
}
```

### Rate Limiting

Protect public-facing agents from abuse:

```rust
use a2a_protocol_sdk::server::{RateLimitInterceptor, RateLimitConfig};

RequestHandlerBuilder::new(executor)
    .with_interceptor(RateLimitInterceptor::new(RateLimitConfig {
        requests_per_window: 100,
        window_secs: 60,
    }))
    .build()
```

For advanced rate limiting (sliding windows, distributed counters), use a
reverse proxy or implement a custom `ServerInterceptor`.

### Client Retry & Reuse

When calling remote agents, build clients once and reuse them. Connection reuse is critical for performance — creating a new client per request bypasses HTTP keep-alive and connection pooling, adding ~300-500us of overhead per call:

```rust
// Build once at startup
let client = ClientBuilder::new("http://agent.example.com")
    .with_retry_policy(RetryPolicy::default())
    .build()
    .unwrap();

// Reuse across all requests — client holds a connection pool
let result = client.send_message(params).await;
```

## Observability

### Structured Logging

Enable the `tracing` feature for structured logs:

```toml
[dependencies]
a2a-protocol-server = { version = "0.5", features = ["tracing"] }
tracing-subscriber = { version = "0.5", features = ["env-filter"] }
```

```rust
use tracing_subscriber::EnvFilter;

tracing_subscriber::fmt()
    .with_env_filter(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"))
    )
    .json()  // JSON output for log aggregation
    .init();
```

Set `RUST_LOG=debug` for verbose output, `RUST_LOG=a2a_protocol_server=debug` for server-specific logs.

### Health Checks

Add a health endpoint alongside your A2A dispatchers:

```rust
// Simple health check handler
async fn health_check(req: hyper::Request<impl hyper::body::Body>) -> hyper::Response<Full<Bytes>> {
    if req.uri().path() == "/health" {
        hyper::Response::new(Full::new(Bytes::from("ok")))
    } else {
        dispatcher.dispatch(req).await
    }
}
```

## Performance

### Connection Handling

Both dispatchers use hyper's HTTP/1.1 and HTTP/2 support via `hyper_util::server::conn::auto::Builder`. This automatically negotiates the best protocol version.

`TCP_NODELAY` is enabled on all server and client TCP sockets. This disables Nagle's algorithm, eliminating the ~40ms delayed-ACK latency that would otherwise penalize small SSE frames and JSON-RPC responses on loopback and low-latency networks.

### Task Store Performance

The `InMemoryTaskStore` uses a pre-allocated `HashMap` with secondary indexes for efficient queries:

- **O(1) amortized save/get/delete** — constant-time operations regardless of store size
- **No resize-induced latency spikes** — pre-allocation to the configured `max_capacity` eliminates the periodic full-rehash events that cause unpredictable 5-7× latency cliffs when the table outgrows its capacity
- **O(log n + page\_size) list queries** — a `BTreeSet<TaskId>` sorted index provides O(log n) cursor positioning via `range()`, and a `HashMap<String, BTreeSet<TaskId>>` context index enables O(log m + page\_size) filtered queries where m = matching tasks. This replaces the previous O(n log n) per-call sort that caused 20-70× regressions at 10K+ tasks.

### No Web Framework Overhead

a2a-rust works directly with hyper — no middleware framework overhead. Cross-cutting concerns (rate limiting, request logging, auth) are handled via the interceptor chain rather than a framework.

### Event Queue Sizing

Tune the event queue for your workload:

```rust
// High-throughput: larger queues for tasks producing >250 events/task
.with_event_queue_capacity(512)

// Memory-constrained: smaller queues
.with_event_queue_capacity(64)
```

> **Benchmark data:** Per-event cost inflects at the broadcast channel capacity
> boundary. With the default capacity of 256 (increased from 64), tasks producing
> >250 events see increased per-event overhead due to broadcast buffer pressure
> (~4µs/event below capacity, ~193µs/event above capacity). Set capacity to at
> least your expected peak event count per task. The `serde_helpers::SerBuffer`
> module can further reduce per-event serialization overhead via thread-local
> buffer reuse.

## Deployment Checklist

- [ ] HTTPS termination configured
- [ ] CORS origins restricted to known clients
- [ ] Executor timeout set
- [ ] Max concurrent streams limited
- [ ] Task store TTL and capacity configured
- [ ] Rate limiting enabled for public-facing agents
- [ ] A2A clients built once and reused (not per-request)
- [ ] Structured logging enabled
- [ ] Health check endpoint available
- [ ] Push notification URLs restricted to HTTPS
- [ ] Body/query size limits verified
- [ ] Graceful shutdown implemented

## Next Steps

- **[GitHub Pages & CI/CD](./cicd.md)** — Continuous deployment
- **[Configuration Reference](../reference/configuration.md)** — All configuration options
