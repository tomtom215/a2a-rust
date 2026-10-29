<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Serving and combining

A dispatcher on its own answers requests; something has to accept the
connections. This page covers the `serve` helpers, wiring a dispatcher into
hyper yourself, the transport limits every HTTP dispatcher shares
(`DispatchConfig`), and serving several bindings from one handler, on separate
ports or on one.

## Using `serve()` (recommended)

Both dispatchers implement the `Dispatcher` trait, so you can use the `serve()` helper to eliminate hyper boilerplate:

```rust,no_run
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
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let handler = std::sync::Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build()?);
use a2a_protocol_server::serve::{serve, serve_with_addr};

// Blocking — runs the accept loop on the current task
serve("127.0.0.1:3000", JsonRpcDispatcher::new(handler.clone())).await?;

// Non-blocking — spawns the server and returns the bound address
let addr = serve_with_addr("127.0.0.1:0", RestDispatcher::new(handler)).await?;
println!("Listening on {addr}");
# Ok(())
# }
```

`serve` and `serve_with_addr` run on `Server` with `ServeConfig::default()`
(header-read and idle timeouts on, connections unbounded). For TLS, a
connection ceiling or a graceful shutdown, use `Server`
([Production Hardening](../../deployment/production.md)).

## Manual wiring (advanced)

Both dispatchers also expose a `dispatch` method for direct hyper integration (this uses `hyper-util` with its `server-auto` feature):

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
use std::sync::Arc;

async fn start_server(
    dispatcher: Arc<JsonRpcDispatcher>,
    addr: &str,
) {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind");

    loop {
        let (stream, _) = listener.accept().await.expect("accept");
        let io = hyper_util::rt::TokioIo::new(stream);
        let dispatcher = Arc::clone(&dispatcher);

        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let d = Arc::clone(&dispatcher);
                async move {
                    Ok::<_, std::convert::Infallible>(d.dispatch(req).await)
                }
            });

            let _ = hyper_util::server::conn::auto::Builder::new(
                hyper_util::rt::TokioExecutor::new(),
            )
            .serve_connection(io, service)
            .await;
        });
    }
}
```

No web framework required — the dispatchers work directly with hyper's service layer.

## Transport limits: DispatchConfig

Both JSON-RPC and REST dispatchers share a `DispatchConfig` for transport-level limits:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_request_body_size` | `usize` | 4 MiB | Maximum request body size in bytes |
| `body_read_timeout` | `Duration` | 30 seconds | Timeout for reading the full request body |
| `max_query_string_length` | `usize` | 4096 | Maximum query string length (`RestDispatcher` only; `A2aRouter` leaves query parsing to axum) |
| `sse_keep_alive_interval` | `Duration` | 30 seconds | Periodic `: keep-alive` comment interval for SSE |
| `sse_channel_capacity` | `usize` | 64 | Backpressure channel between event reader and HTTP response |
| `max_batch_size` | `usize` | 100 | Maximum requests in a JSON-RPC batch |
| `require_version_header` | `bool` | `true` | Refuse requests without an `A2A-Version` header (`accept_missing_version_header()` turns it off) |
| `agent_card_max_age` | `u32` | 3600 | `Cache-Control: max-age`, in seconds, of the public agent card (spec §8.6.1) |

`DispatchConfig` is `#[non_exhaustive]`: build one with
`DispatchConfig::default()` and its `with_*` methods, and pass it to
`JsonRpcDispatcher::with_config(handler, config)`,
`RestDispatcher::with_config`, `A2aRouter::with_config` or
`CombinedDispatcher::with_config`.

## Running multiple transports

Serve JSON-RPC and REST on different ports with the same handler:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use std::sync::Arc;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# fn make_agent_card(jsonrpc_url: &str, rest_url: &str) -> AgentCard {
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc(jsonrpc_url))
#         .with_description("An example agent")
#         .with_interface(AgentInterface::rest(rest_url))
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
# }
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::serve::serve_with_addr;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .with_agent_card(make_agent_card("http://localhost:3000", "http://localhost:3001"))
        .build()
        .unwrap(),
);

// JSON-RPC on port 3000
let jsonrpc_addr = serve_with_addr("127.0.0.1:3000", JsonRpcDispatcher::new(Arc::clone(&handler))).await?;

// REST on port 3001
let rest_addr = serve_with_addr("127.0.0.1:3001", RestDispatcher::new(handler)).await?;
# Ok(())
# }
```

### JSON-RPC and REST on one port

`CombinedDispatcher` serves both bindings on one listener. A `POST` to the
JSON-RPC path (`/` unless `with_jsonrpc_path` names another single segment,
such as `/a2a`), and its CORS preflight, go to JSON-RPC. Every other request
goes to REST, including the agent card, the health probes and every
tenant-prefixed route. Declare both interfaces on the card at that one URL:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use std::sync::Arc;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::dispatch::CombinedDispatcher;
use a2a_protocol_server::serve::serve_with_addr;

let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000/a2a"))
    .with_interface(AgentInterface::rest("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let handler = Arc::new(RequestHandlerBuilder::new(MyExecutor).with_agent_card(card).build()?);

let dispatcher = CombinedDispatcher::new(handler).with_jsonrpc_path("/a2a")?;
serve_with_addr("127.0.0.1:3000", dispatcher).await?;
# Ok(())
# }
```

Like the single-binding dispatchers it works with `Server`, so TLS,
connection limits and a graceful shutdown apply to it too. A JSON-RPC path
with a `/` inside it, a `:`, or one of the REST binding's own first segments
(`/tasks`, `/extendedAgentCard`, `/.well-known`, `/health`, `/ready`) is
refused, because REST routes it or could.
