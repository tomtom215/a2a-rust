# Dispatchers (JSON-RPC, REST, WebSocket, Axum & gRPC)

Dispatchers translate HTTP/gRPC requests into handler calls. a2a-rust provides five built-in dispatchers: `JsonRpcDispatcher`, `RestDispatcher`, `A2aRouter` (`axum` feature), `WebSocketDispatcher` (`websocket` feature), and `GrpcDispatcher` (`grpc` feature). `CombinedDispatcher` puts the first two on one listener ([JSON-RPC and REST on one port](./dispatchers/serving.md#json-rpc-and-rest-on-one-port)).

Every dispatcher feeds the same `RequestHandler`, so one handler can serve
several bindings at once and they share its tasks, push configs and executor.
What happens after the dispatcher hands a request on is on
[Request Handler & Builder](./handler.md#what-happens-to-a-request).

## Which dispatcher do I need

| Dispatcher | Binding | Feature | Page |
|---|---|---|---|
| `JsonRpcDispatcher` | JSON-RPC 2.0 over HTTP | none | [JSON-RPC dispatcher](./dispatchers/jsonrpc.md) |
| `RestDispatcher` | HTTP+JSON (REST) | none | [REST dispatcher](./dispatchers/rest.md) |
| `CombinedDispatcher` | JSON-RPC and REST on one port | none | [Serving and combining](./dispatchers/serving.md#json-rpc-and-rest-on-one-port) |
| `A2aRouter` | HTTP+JSON as an `axum::Router` | `axum` | [Axum router](./dispatchers/axum.md) |
| `WebSocketDispatcher` | JSON-RPC 2.0 over WebSocket | `websocket` | [Below](#websocketdispatcher), and [TLS and limits](./dispatchers/websocket.md) |
| `GrpcDispatcher` | gRPC, `lf.a2a.v1.A2AService` | `grpc`, `grpc-tls` | [gRPC dispatcher](./dispatchers/grpc.md) |

The rest of the chapter:

- [Serving and combining](./dispatchers/serving.md): the `serve` helpers,
  wiring into hyper by hand, the shared `DispatchConfig` limits, and several
  bindings from one handler.
- [CORS](./dispatchers/cors.md): letting a browser on another origin call the
  agent.
- [Calling the handler directly](./handler/direct-calls.md): skipping the
  dispatchers when you already own the HTTP surface.

## WebSocketDispatcher

Provides bidirectional A2A communication over WebSocket. Enable with the `websocket` feature flag:

```toml
a2a-protocol-server = { version = "0.16", features = ["websocket"] }
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
use a2a_protocol_server::WebSocketDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(WebSocketDispatcher::new(handler));

// Blocking server
dispatcher.clone().serve("0.0.0.0:3000").await?;

// Non-blocking (returns bound address)
let addr = dispatcher.serve_with_addr("127.0.0.1:0").await?;
# Ok(())
# }
```

### Protocol

- Client sends JSON-RPC 2.0 requests as WebSocket text frames
- Server responds with JSON-RPC 2.0 responses as text frames
- Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) send one frame per event, followed by a final JSON-RPC success response
- The full A2A method surface is routed — the same method names as
  `JsonRpcDispatcher`, including the push-notification-config methods and
  `GetExtendedAgentCard`. The v0.3 `method/verb` spellings are refused with
  `MethodNotFound` (`-32601`), as they are over HTTP
- The binding's full specification (service parameters, data types, stream
  ordering and termination, error mapping, security) is
  [its own page](../bindings/websocket.md)
- `GET /.well-known/agent-card.json` on the same port answers the agent card
  as plain HTTP/1.1, as the HTTP dispatchers do (spec §8.1), since 0.15; any
  other request goes to the WebSocket handshake

### Authentication and tenancy

The HTTP headers of the upgrade request (lowercased, plus the request path
under `":path"`) are captured during the handshake and passed to the
handler for **every** request on the connection. Tenant resolvers, strict
multi-tenancy, and header-based authentication behave exactly as they do
over HTTP — credentials are presented once, at connect time, and apply to
the whole connection — except that a refused credential is answered only in
the JSON-RPC body (-32600), since interceptors run per message after the
upgrade; see [Authentication](./authentication.md#error-mapping-important).
An upgrade request with no `A2A-Version` (header or `?A2A-Version=`
parameter), or one naming a version other than 1.0 (a patch number is
ignored), is rejected during the handshake with HTTP 400
(`accept_missing_version_header()` admits the former).

Serving `wss://`, refusing other sites' pages, and bounding connections are on
[WebSocket: TLS and limits](./dispatchers/websocket.md).

## Next steps

- **[Push Notifications](./push-notifications.md)** — Webhook delivery
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Production Hardening](../deployment/production.md)** — Deployment best practices
