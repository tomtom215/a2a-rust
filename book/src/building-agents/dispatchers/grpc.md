<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# gRPC dispatcher

`GrpcDispatcher` serves the canonical `lf.a2a.v1.A2AService` over `tonic`. This
page covers setting it up, `GrpcConfig`, bounding idle and long-lived
connections, and mounting the service on a `tonic` server of your own.

Routes gRPC requests to the handler via `tonic`. Enable with the `grpc` feature flag
(plaintext listener) or `grpc-tls` to serve TLS on the listener itself with
`with_tls` — see [Transport Layers](../../concepts/transport-layers/grpc.md#serving-tls):

```toml
a2a-protocol-server = { version = "0.15", features = ["grpc"] }
```

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
use a2a_protocol_server::{GrpcDispatcher, GrpcConfig};
use std::sync::Arc;

let config = GrpcConfig::default()
    .with_max_message_size(8 * 1024 * 1024)
    .with_concurrency_limit(128);

// Blocking server (`serve` consumes the dispatcher)
GrpcDispatcher::new(Arc::clone(&handler), config.clone())
    .serve("0.0.0.0:50051")
    .await?;

// Non-blocking (returns bound address)
let addr = GrpcDispatcher::new(Arc::clone(&handler), config.clone())
    .serve_with_addr("127.0.0.1:0")
    .await?;
println!("gRPC listening on {addr}");

// Pre-bind pattern (when you need the address before building the handler)
let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr()?;
// ... build handler using addr for agent card URL ...
let dispatcher = GrpcDispatcher::new(handler, config);
let bound = dispatcher.serve_with_listener(listener)?;
# Ok(())
# }
```

## GrpcConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_message_size` | `usize` | 4 MiB | Maximum inbound/outbound message size |
| `concurrency_limit` | `usize` | 256 | Maximum concurrent gRPC requests per connection |
| `stream_channel_capacity` | `usize` | 64 | Bounded channel for streaming responses |
| `require_version_header` | `bool` | `true` | Refuse a request whose `a2a-version` metadata is absent or empty (`with_require_version_header(false)` admits it) |

## Bounding an idle gRPC connection

`GrpcConfig` bounds message size and per-connection concurrency. Neither bounds
a connection that simply exists: measured, 400 TCP connections opened and left
silent were all accepted, and the oldest was still alive twelve seconds later.

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::dispatch::grpc::{GrpcConfig, GrpcDispatcher};
# use std::sync::Arc;
# use std::time::Duration;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build().expect("handler"));
let dispatcher = GrpcDispatcher::new(handler, GrpcConfig::default())
    .with_http2_keepalive(Duration::from_secs(30), Duration::from_secs(10))
    .with_max_connection_age(Duration::from_secs(600))
    .with_max_connections(1024);
# }
```

All three are opt-in. `with_max_connections` is the ceiling the other two do
not provide — keepalive closes a peer that stops answering, and nothing bounded
how many peers could be answering at once. As on the WebSocket dispatcher, the
permit is taken before `accept()`, so load past the ceiling waits in the
kernel's listen backlog and is refused by the kernel when that fills, rather
than being accepted and then dropped. The other two: HTTP/2 keepalive PINGs an idle connection and closes it if the
peer does not answer — and a conformant client's HTTP/2 stack answers without
the application being involved, so this closes peers that are **unresponsive**,
not peers that are merely quiet. A streaming RPC waiting for its next event is
left alone. `with_max_connection_age` is the different question: it bounds a
peer that answers perfectly and never leaves, which is what makes a fleet
behind a load balancer drift into imbalance. tonic sends GOAWAY and drains
in-flight RPCs, so it is a reconnect rather than a failure.

## Protocol

All 11 A2A methods are served on the canonical `lf.a2a.v1.A2AService` with
fully-typed protobuf messages generated from the A2A specification's schema —
wire-compatible with the official Go, Python, and Java SDKs. Requests and
responses convert to and from the serde domain types through a fallible
`TryFrom` layer (ProtoJSON semantics; see ADR 0009).

Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) use gRPC server
streaming. The pre-0.7 JSON-in-`bytes` tunnel (`a2a.v1.A2aService`), served
alongside the canonical service through 0.7 behind the off-by-default
`grpc-legacy-json` feature, was **removed in 0.8**. The canonical service is
the only gRPC surface.

The agent card is served on the same port at `/.well-known/agent-card.json`
(spec §8.1), since 0.15. A card fetch is usually HTTP/1.1, so a dispatcher
with a card also accepts HTTP/1.1 connections; gRPC itself is HTTP/2 either
way. Over TLS tonic offers only `h2` in ALPN, so a fetch there needs a client
that speaks HTTP/2, as this SDK's does. `into_service()` below is the A2A
service alone, without the card.

## Custom server setup

For advanced scenarios, use `into_service()` to get a tonic service:

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
# let dispatcher = a2a_protocol_server::GrpcDispatcher::new(handler, a2a_protocol_server::GrpcConfig::default());
# let addr: std::net::SocketAddr = "127.0.0.1:50051".parse()?;
let svc = dispatcher.into_service();
tonic::transport::Server::builder()
    .add_service(svc)
    .serve(addr)
    .await?;
# Ok(())
# }
```

`into_service()` applies only `GrpcConfig`'s `max_message_size`,
`stream_channel_capacity` and `require_version_header`. `concurrency_limit`,
`with_tls`, `with_http2_keepalive`, `with_max_connection_age`,
`with_max_connections` and the agent card on the same port belong to the
`serve*` methods; on your own `tonic::transport::Server`, set the equivalents
(for example `.concurrency_limit_per_connection(256)`).
