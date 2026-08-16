# Interceptors & Middleware

Interceptors let you hook into the request/response pipeline on both the client and server side — for authentication, logging, metrics, rate limiting, or any cross-cutting concern.

## Server Interceptors

Server interceptors run before and after the handler processes a request:

```rust,ignore
use a2a_protocol_sdk::server::ServerInterceptor;

struct LoggingInterceptor;

impl ServerInterceptor for LoggingInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("Request: {}", ctx.method());
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            println!("Response: {}", ctx.method());
            Ok(())
        })
    }
}
```

### Adding Interceptors

```rust,ignore
RequestHandlerBuilder::new(my_executor)
    .with_interceptor(AuthInterceptor::new(auth_config))
    .with_interceptor(LoggingInterceptor)
    .with_interceptor(MetricsInterceptor::new())
    .build()
```

Interceptors execute in the order they're added:

```text
Request → Auth → Logging → Metrics → Handler → Metrics → Logging → Auth → Response
```

### Example: Authentication

```rust,ignore
struct BearerAuthInterceptor {
    valid_tokens: HashSet<String>,
}

impl ServerInterceptor for BearerAuthInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Check Authorization header before passing to handler
            // Return error if token is missing or invalid
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }
}
```

## Client Interceptors

Client interceptors modify outgoing requests and incoming responses:

```rust,ignore
use a2a_protocol_sdk::client::CallInterceptor;

struct RequestIdInterceptor;

impl CallInterceptor for RequestIdInterceptor {
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            // Add X-Request-Id header to outgoing requests
            Ok(())
        }
    }

    fn after<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            // Log the response status
            Ok(())
        }
    }
}
```

### Adding Client Interceptors

```rust,ignore
use a2a_protocol_sdk::client::ClientBuilder;

let client = ClientBuilder::new("http://agent.example.com".into())
    .with_interceptor(RequestIdInterceptor)
    .with_interceptor(RetryInterceptor::new(3))
    .build()
    .unwrap();
```

## Common Patterns

### Logging

Log method names, durations, and errors:

```rust,no_run
struct LoggingInterceptor;
// Log: "SendMessage completed in 42ms"
// Log: "GetTask failed: task not found (15ms)"
```

### Metrics

Track request counts, latencies, error rates:

```rust,ignore
struct MetricsInterceptor {
    counter: Arc<AtomicU64>,
}
// Increment counter on each request
// Record latency histogram
```

### Rate Limiting

The built-in `RateLimitInterceptor` provides per-caller fixed-window rate limiting:

```rust,ignore
use a2a_protocol_sdk::server::{RateLimitInterceptor, RateLimitConfig};
use std::sync::Arc;

let limiter = Arc::new(
    RateLimitInterceptor::new(RateLimitConfig {
        requests_per_window: 100,
        window_secs: 60,
        ..RateLimitConfig::default()
    })
    .expect("valid rate limit config"),
);

// Add to handler builder:
RequestHandlerBuilder::new(my_executor)
    .with_interceptor(limiter)
    .build()
```

Caller keys are derived from `CallContext::caller_identity()` (set by auth
interceptors) or `"anonymous"`. The `X-Forwarded-For` header is only consulted
when `trusted_proxy_hops` is set to the number of trusted reverse proxies in
front of the server — the header is client-controlled, so it is ignored by
default. The bucket map is bounded by `max_buckets` (default 10,000).

> **Note:** `CallContext` fields are read-only (accessed via methods like
> `ctx.method()`, `ctx.caller_identity()`, `ctx.http_headers()`). This
> prevents interceptors from mutating security-critical context mid-request.

For advanced
use cases (sliding windows, distributed counters), implement a custom
`ServerInterceptor` or use a reverse proxy.

## Interceptor Chain

Both client and server support ordered interceptor chains. The chain is built incrementally:

```rust,ignore
// Each .with_interceptor() call appends to the chain
builder
    .with_interceptor(first)    // Runs first on request, last on response
    .with_interceptor(second)   // Runs second on request, second-to-last on response
    .with_interceptor(third)    // Runs third on request, first on response
```

## Next Steps

- **[Task & Config Stores](./stores.md)** — Pluggable storage backends
- **[Production Hardening](../deployment/production.md)** — Security and reliability
