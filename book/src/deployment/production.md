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
use a2a_sdk::server::CorsConfig;

// The dispatchers include CORS handling.
// Configure allowed origins for production.
```

### Push Notification Security

The built-in `HttpPushSender` includes:

- **SSRF protection** — Rejects private/loopback IPs after DNS resolution
- **Header injection prevention** — Validates credentials for `\r`/`\n` characters
- **HTTPS enforcement** — Optionally require HTTPS webhook URLs

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
| Event size | 16 MiB (configurable) | SSE streaming |

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
use a2a_sdk::server::TaskStoreConfig;

RequestHandlerBuilder::new(executor)
    .with_task_store_config(TaskStoreConfig {
        ttl: Some(Duration::from_secs(3600)),
        max_capacity: Some(100_000),
    })
    .build()
```

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

## Observability

### Structured Logging

Enable the `tracing` feature for structured logs:

```toml
[dependencies]
a2a-server = { version = "0.2", features = ["tracing"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
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

Set `RUST_LOG=debug` for verbose output, `RUST_LOG=a2a_server=debug` for server-specific logs.

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

### No Web Framework Overhead

a2a-rust works directly with hyper — no middleware framework overhead. If you need additional middleware (rate limiting, request logging), use interceptors rather than wrapping in a framework.

### Event Queue Sizing

Tune the event queue for your workload:

```rust
// High-throughput: larger queues
.with_event_queue_capacity(256)

// Memory-constrained: smaller queues
.with_event_queue_capacity(16)
```

## Deployment Checklist

- [ ] HTTPS termination configured
- [ ] CORS origins restricted to known clients
- [ ] Executor timeout set
- [ ] Max concurrent streams limited
- [ ] Task store TTL and capacity configured
- [ ] Structured logging enabled
- [ ] Health check endpoint available
- [ ] Push notification URLs restricted to HTTPS
- [ ] Body/query size limits verified
- [ ] Graceful shutdown implemented

## Next Steps

- **[GitHub Pages & CI/CD](./cicd.md)** — Continuous deployment
- **[Configuration Reference](../reference/configuration.md)** — All configuration options
