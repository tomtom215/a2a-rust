<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Interceptor patterns

Most interceptors do one of three jobs: log each call, count calls, or limit
how often a caller may call. This page sketches the first two and covers the
built-in `RateLimitInterceptor` in full.

## Logging

Log the method, the outcome and how long the call took from [`on_complete`](./server.md#seeing-every-outcome-on_complete)
(`CallContext::received_at()` is when the call arrived):

```rust,no_run
struct LoggingInterceptor;
// Log: "SendMessage completed in 42ms"
// Log: "GetTask failed: task not found (15ms)"
```

## Metrics

Count requests. A counter in `before` counts every call that reaches this
interceptor, whatever its outcome; to count successes and failures separately,
or to time calls, use
[`on_complete`](./server.md#seeing-every-outcome-on_complete):

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

## Rate limiting

The built-in `RateLimitInterceptor` provides per-caller fixed-window rate limiting:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
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
    .with_agent_card(card())
    .with_interceptor(limiter)
    .build()?;
# Ok(())
# }
```

Caller keys come from `CallContext::caller_identity()` when an authenticating
interceptor registered before the limiter set one. Otherwise they come from the
client address: the `x-forwarded-for` entry that `trusted_proxy_hops` trusted
reverse proxies appended, or, when `trusted_proxy_hops` is 0 (the default), the
connection's peer address, an IPv6 address counting as its /64. The header is
client-controlled, so it is ignored unless you set `trusted_proxy_hops` to the
number of trusted reverse proxies in front of the server. Only a call with no
identity and no usable address (a forwarded chain shorter than
`trusted_proxy_hops`, or a transport that records no peer) shares the `anon`
bucket.

`JwtAuthInterceptor` sets `jwt:<issuer>:<sub>` and nothing for a token without
`sub`. `BearerTokenAuthInterceptor::new` and `ApiKeyAuthInterceptor::new` name
nobody, so their callers are limited by address; use
`BearerTokenAuthInterceptor::with_labelled_tokens([(token, "caller-a"), …])` or
`ApiKeyAuthInterceptor::with_labelled_keys(…)` for per-caller limits (the key
is then `id:bearer:caller-a` or `id:api-key:caller-a`). The bucket map is
bounded by `max_buckets` (default 10,000).

> **Note:** `CallContext` fields are read-only (accessed via methods like
> `ctx.method()`, `ctx.caller_identity()`, `ctx.http_headers()`), with
> write-once exceptions: `set_caller_identity`, `set_auth_scheme` and
> `set_actors`, which an authenticating interceptor sets and nothing can then
> overwrite. This prevents interceptors from mutating security-critical context
> mid-request.

For limits shared across replicas, pass a `RateLimitCounter` to
`RateLimitInterceptor::with_shared_counter` (`PostgresRateLimitCounter` ships
under the `postgres` feature). For sliding windows, implement a custom
`ServerInterceptor` or use a reverse proxy.
