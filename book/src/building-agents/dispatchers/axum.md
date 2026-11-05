<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Axum router

`RestRouter` serves the REST binding as an `axum::Router`, so an existing Axum
application can mount the agent beside its own routes and middleware. This
page shows how, and where it differs from `RestDispatcher`.

For projects already using Axum, the `axum` feature provides `RestRouter` — an
idiomatic adapter that wraps `RequestHandler` as an `axum::Router`:

```toml
a2a-protocol-server = { version = "0.16", features = ["axum"] }
```

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::rest("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
use a2a_protocol_server::RestRouter;
use std::sync::Arc;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .with_agent_card(card)
        .build()
        .unwrap(),
);

// The router alone: `Server` serves it, with `ServeConfig`'s header-read and
// idle timeouts, connection cap, loopback `Host` check and production
// refusal, and a shutdown that ends the tasks in flight before it drains.
let report = a2a_protocol_server::serve::Server::bind("0.0.0.0:3000")
    .await?
    .serve_with_shutdown(RestRouter::new(handler), a2a_protocol_server::serve::shutdown_signal())
    .await;
if let Some(why) = report.refused {
    return Err(why.into());
}
# Ok(())
# }
```

`RestRouter` is a `Dispatcher` since 0.16.0 (ADR 0052). Until then it could
only be served with `axum::serve`, which has none of `Server`'s protections.
It is the same type as `A2aRouter`, under a name that says which binding it
serves (ADR 0053).

## Composability

`into_router()` returns the `axum::Router`, for an application that merges it
with its own routes and middleware and serves the result with `axum::serve`
(which then owns the timeouts and the shutdown):

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::rest("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# async fn custom_handler() -> &'static str { "ok" }
# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let handler = std::sync::Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build()?);
# use a2a_protocol_server::RestRouter;
let app = axum::Router::new()
    .merge(RestRouter::new(handler).into_router())
    .route("/custom", axum::routing::get(custom_handler));
# Ok(())
# }
```

## Routes

All 11 A2A REST methods are mapped, plus health check and agent card discovery.
Streaming methods return SSE responses. The router delegates entirely to
`RequestHandler` — zero business logic duplication.

Unlike `RestDispatcher`, the router has no `/{tenant}/…` or
`/tenants/{tenant}/…` routes: a tenant-prefixed path is a 404. Resolve
tenants with a `TenantResolver` (for example `HeaderTenantResolver`) plus
`require_resolved_tenant()`; with no resolver a GET or DELETE names its
tenant in `?tenant=` and a POST in the body's `tenant` field.
