# Interceptors & Middleware

Interceptors let you hook into the request/response pipeline on both the client and server side — for authentication, logging, metrics, rate limiting, or any cross-cutting concern.

## Server Interceptors

Server interceptors run before and after the handler processes a request:

```rust
use a2a_protocol_sdk::server::ServerInterceptor;

struct LoggingInterceptor;

impl ServerInterceptor for LoggingInterceptor {
    // Intercept incoming requests and outgoing responses
}
```

### Adding Interceptors

```rust
RequestHandlerBuilder::new(my_executor)
    .with_interceptor(AuthInterceptor::new(auth_config))
    .with_interceptor(LoggingInterceptor)
    .with_interceptor(MetricsInterceptor::new())
    .build()
```

Interceptors execute in the order they're added:

```
Request → Auth → Logging → Metrics → Handler → Metrics → Logging → Auth → Response
```

### Example: Authentication

```rust
struct BearerAuthInterceptor {
    valid_tokens: HashSet<String>,
}

impl ServerInterceptor for BearerAuthInterceptor {
    // Check Authorization header before passing to handler
    // Return 401 if token is missing or invalid
}
```

## Client Interceptors

Client interceptors modify outgoing requests and incoming responses:

```rust
use a2a_protocol_sdk::client::CallInterceptor;

struct RequestIdInterceptor;

impl CallInterceptor for RequestIdInterceptor {
    // Add X-Request-Id header to outgoing requests
    // Log the response status
}
```

### Adding Client Interceptors

```rust
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

```rust
struct LoggingInterceptor;
// Log: "SendMessage completed in 42ms"
// Log: "GetTask failed: task not found (15ms)"
```

### Metrics

Track request counts, latencies, error rates:

```rust
struct MetricsInterceptor {
    counter: Arc<AtomicU64>,
}
// Increment counter on each request
// Record latency histogram
```

### Rate Limiting

Reject requests that exceed a rate limit:

```rust
struct RateLimitInterceptor {
    limiter: Arc<RateLimiter>,
}
// Check rate limit before passing to handler
// Return 429 Too Many Requests if exceeded
```

## Interceptor Chain

Both client and server support ordered interceptor chains. The chain is built incrementally:

```rust
// Each .with_interceptor() call appends to the chain
builder
    .with_interceptor(first)    // Runs first on request, last on response
    .with_interceptor(second)   // Runs second on request, second-to-last on response
    .with_interceptor(third)    // Runs third on request, first on response
```

## Next Steps

- **[Task & Config Stores](./stores.md)** — Pluggable storage backends
- **[Production Hardening](../deployment/production.md)** — Security and reliability
