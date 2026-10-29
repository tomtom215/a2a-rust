<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Timeouts and Limits

The bounds that stop one slow executor, one greedy caller or one busy day from
taking the whole process with it. Each has a default; this page says what it is
and how to change it.

## Executor timeout

Prevent hung tasks from consuming resources forever. The default ceiling is
one hour (`DEFAULT_EXECUTOR_TIMEOUT`): an executor that never returns would
otherwise pin its task, queue and cancellation token for the life of the
process. Set one matched to your workload, or call `without_executor_timeout()`
if your executors are genuinely unbounded:

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
# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
use std::time::Duration;

RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_executor_timeout(Duration::from_secs(300))
    .build()
# }
```

## Concurrent stream limits

Concurrent streaming requests are capped at 1024 by default (each stream
allocates channels and spawns background tasks). Tune the ceiling to your
deployment; pass `usize::MAX` to effectively disable it:

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
# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_max_concurrent_streams(1000)
    .build()
# }
```

## Task store limits

Prevent unbounded memory growth:

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
# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
# use std::time::Duration;
use a2a_protocol_sdk::server::TaskStoreConfig;

RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_task_store_config(
        TaskStoreConfig::default()
            .with_max_capacity(Some(100_000))
            .with_task_ttl(Some(Duration::from_secs(3600)))
            .with_eviction_interval(64)
            .with_max_page_size(1000),
    )
    .build()
# }
```

Use `TaskStore::count()` for monitoring capacity utilization.

## Connections

`serve::Server` (and so `serve` and `serve_with_addr`, which run on it) accepts
any number of connections by default. `ServeConfig::with_max_connections`
holds a permit before each accept, so a burst over the ceiling waits in the
kernel's backlog rather than becoming tasks. Two timeouts are on by default:
a peer has 30 seconds to send its request headers
(`DEFAULT_HEADER_READ_TIMEOUT`), and a connection with no bytes moving in
either direction for 75 seconds is closed (`DEFAULT_IDLE_TIMEOUT`). Leave both
on; `None` turns one off.

```rust,no_run
use std::time::Duration;
use a2a_protocol_server::serve::ServeConfig;

let config = ServeConfig::new()
    .with_max_connections(1024)
    .with_header_read_timeout(Some(Duration::from_secs(30)))
    .with_idle_timeout(Some(Duration::from_secs(75)));
# let _ = config;
```

The idle timeout must stay well above the SSE keep-alive interval
(`DispatchConfig::sse_keep_alive_interval`, 30 seconds by default): those
keep-alive comments are what make a quiet stream look busy to it.

## Rate limiting

Protect public-facing agents from abuse:

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
# fn f(executor: MyAgent) -> ServerResult<RequestHandler> {
use a2a_protocol_sdk::server::{RateLimitInterceptor, RateLimitConfig};

RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_interceptor(
        RateLimitInterceptor::new(
            RateLimitConfig::default()
                .with_requests_per_window(100)
                .with_window_secs(60)
                // Set to the number of trusted reverse proxies so the client
                // IP is taken from X-Forwarded-For; 0 (default) ignores it.
                .with_trusted_proxy_hops(1),
        )
        .expect("valid rate limit config"),
    )
    .build()
# }
```

The window is fixed, so a caller can spend up to twice `requests_per_window`
across a window boundary. Register the limiter after the authentication
interceptor; registered first, it cannot key on the caller's identity and falls
back to the client address. It cannot slow credential guessing; use
`AuthFailureThrottle` for that
([Authentication](../../building-agents/authentication/throttling.md)).

For a limit shared across replicas, use `with_shared_counter` with
`PostgresRateLimitCounter` or your own `RateLimitCounter` — see
[A shared rate-limit counter](../horizontal-scaling/rate-limiting.md).
For sliding windows, use a reverse proxy or implement a custom
`ServerInterceptor`.

## Request size

Request bodies, query strings and SSE events have caps of their own; they are
listed under [Input handling](../security.md#input-handling).
