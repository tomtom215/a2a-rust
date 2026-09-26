# Interceptors & Middleware

Interceptors let you hook into the request/response pipeline on both the client and server side — for authentication, logging, metrics, rate limiting, or any cross-cutting concern.

## Server Interceptors

Server interceptors run before and after the handler processes a request:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
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
            println!("Answered: {}", ctx.method());
            Ok(())
        })
    }
}
```

### Adding Interceptors

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
# use a2a_protocol_sdk::server::ServerInterceptor;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# struct LoggingInterceptor;
# impl ServerInterceptor for LoggingInterceptor {
#     fn before<'a>(&'a self, _: &'a CallContext) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
#     fn after<'a>(&'a self, _: &'a CallContext) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
# }
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let my_executor = MyAgent;
let handler = RequestHandlerBuilder::new(my_executor)
    .with_interceptor(BearerTokenAuthInterceptor::new(["s3cret-token"]))
    .with_interceptor(LoggingInterceptor)
    .with_interceptor(RateLimitInterceptor::new(RateLimitConfig::default())?)
    .build()?;
# Ok(())
# }
```

Interceptors execute in the order they're added:

```text
Request → Auth → Logging → RateLimit → Handler → RateLimit → Logging → Auth → Response
```

### Seeing every outcome: `on_complete`

`after` runs only when the call succeeds, and an error it returns replaces
the response. For work that must happen however the call ends — releasing
what `before` acquired, closing an audit record, counting failures by
caller — override `on_complete`. It is called once per call, in reverse
order, on every interceptor whose `before` ran, with how the call ended:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::server::{CallContext, CallOutcome, ServerInterceptor};

struct AuditInterceptor;

impl ServerInterceptor for AuditInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn on_complete<'a>(
        &'a self,
        ctx: &'a CallContext,
        outcome: CallOutcome<'a>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            match outcome {
                CallOutcome::Succeeded => println!("{}: ok", ctx.method()),
                CallOutcome::Failed(e) => println!("{}: {}", ctx.method(), e.metric_label()),
                CallOutcome::Cancelled => println!("{}: client went away", ctx.method()),
                _ => {}
            }
        })
    }
}
```

`Failed` carries the error the caller is sent, whether a `before` hook, the
handler or an `after` hook produced it. `Cancelled` means the call's future
was dropped unanswered — the client disconnected, or a timeout above the
handler gave up — and the hook then runs in a task of its own. It returns
nothing, so it cannot change the response. For `SendStreamingMessage` and
`SubscribeToTask` the call ends when the stream is established, not when it
closes. The trait method's documentation states the full contract.

### Example: Authentication

For a fixed set of API keys or bearer tokens, use the built-in
`ApiKeyAuthInterceptor` or `BearerTokenAuthInterceptor`: they compare
credentials in constant time, which a `HashSet` lookup does not. A custom
interceptor is for credentials you verify some other way — a session service,
say. It rejects in `before`, records who the caller is, and says that it
authenticates:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
# use a2a_protocol_sdk::server::ServerInterceptor;
# fn verify_session(_token: &str) -> Option<String> { None }
/// Accepts a request whose bearer token `verify_session` maps to a caller.
struct SessionAuthInterceptor;

impl ServerInterceptor for SessionAuthInterceptor {
    fn before<'a>(
        &'a self,
        ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let caller = ctx
                .http_headers()
                .get("authorization")
                .and_then(|h| h.strip_prefix("Bearer "))
                .and_then(verify_session)
                .ok_or_else(|| {
                    // 401 + WWW-Authenticate on HTTP, UNAUTHENTICATED on gRPC (ADR 0014).
                    A2aError::unauthenticated("authentication required", "Bearer realm=\"a2a\"")
                })?;
            // Rate limiting and executors (`ctx.caller_identity()`) key on this.
            ctx.set_caller_identity(caller);
            Ok(())
        })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Ok(()) })
    }

    // The extended agent card may be served only behind an authenticating
    // interceptor (spec §13.3); this is how the handler knows one is there.
    fn authenticates(&self) -> bool {
        true
    }
}
```

Refuse an authenticated caller who lacks permission with
`A2aError::permission_denied(message)` (`403` / `PERMISSION_DENIED`). Any other
error keeps its usual status, so a client never learns to refresh its token.

## Client Interceptors

Client interceptors modify outgoing requests and incoming responses:

```rust
# use std::future::Future;
# use a2a_protocol_sdk::client::{ClientRequest, ClientResponse, ClientResult};
use a2a_protocol_sdk::client::CallInterceptor;

struct RequestIdInterceptor;

impl CallInterceptor for RequestIdInterceptor {
    fn before<'a>(
        &'a self,
        req: &'a mut ClientRequest,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            // Add X-Request-Id header to outgoing requests
            req.extra_headers
                .insert("x-request-id".into(), uuid::Uuid::new_v4().to_string());
            Ok(())
        }
    }

    fn after<'a>(
        &'a self,
        resp: &'a ClientResponse,
    ) -> impl Future<Output = ClientResult<()>> + Send + 'a {
        async move {
            // Log the method that completed
            println!("{} completed", resp.method);
            Ok(())
        }
    }
}
```

`after` runs only when the call succeeds. To see failures, override
`on_error`, which has a no-op default: it gets the request as `before` left
it (its params have gone to the transport, so they read as `null`) and the
`ClientError`, runs in reverse registration order, and cannot change the
error the caller receives. `BearerAuthInterceptor` uses it to drop a token
the agent answered with `401`, so the next call fetches a new one.

### Adding Client Interceptors

Retries are a policy on the builder, not an interceptor:

```rust
# use std::future::Future;
# use a2a_protocol_sdk::client::{CallInterceptor, ClientRequest, ClientResponse, ClientResult};
# struct RequestIdInterceptor;
# impl CallInterceptor for RequestIdInterceptor {
#     fn before<'a>(&'a self, _: &'a mut ClientRequest) -> impl Future<Output = ClientResult<()>> + Send + 'a { async { Ok(()) } }
#     fn after<'a>(&'a self, _: &'a ClientResponse) -> impl Future<Output = ClientResult<()>> + Send + 'a { async { Ok(()) } }
# }
use a2a_protocol_sdk::client::{ClientBuilder, RetryPolicy};

let client = ClientBuilder::new("http://agent.example.com")
    .with_interceptor(RequestIdInterceptor)
    .with_retry_policy(RetryPolicy::default().with_max_retries(3))
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

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::server::CallContext;
# use a2a_protocol_sdk::server::ServerInterceptor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct MetricsInterceptor {
    requests: Arc<AtomicU64>,
}

impl ServerInterceptor for MetricsInterceptor {
    fn before<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        // Increment counter on each request
        self.requests.fetch_add(1, Ordering::Relaxed);
        Box::pin(async { Ok(()) })
    }

    fn after<'a>(
        &'a self,
        _ctx: &'a CallContext,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}
```

### Rate Limiting

The built-in `RateLimitInterceptor` provides per-caller fixed-window rate limiting:

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let my_executor = MyAgent;
use a2a_protocol_sdk::server::{RateLimitInterceptor, RateLimitConfig};

let limiter = RateLimitInterceptor::new(
    RateLimitConfig::default()
        .with_requests_per_window(100)
        .with_window_secs(60),
)?;

// Add to handler builder:
let handler = RequestHandlerBuilder::new(my_executor)
    .with_interceptor(limiter)
    .build()?;
# Ok(())
# }
```

Caller keys are derived from `CallContext::caller_identity()`, or
`"anonymous"`. `JwtAuthInterceptor` sets the identity from the token's `sub`.
`BearerTokenAuthInterceptor::new` and `ApiKeyAuthInterceptor::new` do not, so
every valid caller shares one bucket; use
`BearerTokenAuthInterceptor::with_labelled_tokens([(token, "caller-a"), …])` or
`ApiKeyAuthInterceptor::with_labelled_keys(…)` for per-caller limits. The
`X-Forwarded-For` header is only consulted
when `trusted_proxy_hops` is set to the number of trusted reverse proxies in
front of the server — the header is client-controlled, so it is ignored by
default. The bucket map is bounded by `max_buckets` (default 10,000).

> **Note:** `CallContext` fields are read-only (accessed via methods like
> `ctx.method()`, `ctx.caller_identity()`, `ctx.http_headers()`), with one
> write-once exception: `set_caller_identity`, which the first authenticating
> interceptor sets and nothing can then overwrite. This prevents interceptors
> from mutating security-critical context mid-request.

For limits shared across replicas, pass a `RateLimitCounter` to
`RateLimitInterceptor::with_shared_counter` (`PostgresRateLimitCounter` ships
under the `postgres` feature). For sliding windows, implement a custom
`ServerInterceptor` or use a reverse proxy.

## Interceptor Chain

Both client and server support ordered interceptor chains. The chain is built incrementally:

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
# let builder = RequestHandlerBuilder::new(MyAgent);
# let first = BearerTokenAuthInterceptor::new(["t"]);
# let second = ApiKeyAuthInterceptor::new(["k"]);
# let third = RateLimitInterceptor::new(RateLimitConfig::default()).unwrap();
// Each .with_interceptor() call appends to the chain
let builder = builder
    .with_interceptor(first)    // Runs first on request, last on response
    .with_interceptor(second)   // Runs second on request, second-to-last on response
    .with_interceptor(third);   // Runs third on request, first on response
# }
```

## Next Steps

- **[Task & Config Stores](./stores.md)** — Pluggable storage backends
- **[Production Hardening](../deployment/production.md)** — Security and reliability
