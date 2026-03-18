# Dispatchers (JSON-RPC, REST, Axum & gRPC)

Dispatchers translate HTTP/gRPC requests into handler calls. a2a-rust provides five built-in dispatchers: `JsonRpcDispatcher`, `RestDispatcher`, `A2aRouter` (`axum` feature), `WebSocketDispatcher` (`websocket` feature), and `GrpcDispatcher` (`grpc` feature).

## JsonRpcDispatcher

Routes JSON-RPC 2.0 requests to the handler:

```rust
use a2a_protocol_sdk::server::JsonRpcDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
```

### Features

- **Single endpoint** — All methods go to `/` as POST requests
- **Agent card** — `GET /.well-known/agent.json` returns the agent card (same as REST)
- **Batch support** — Handles JSON-RPC batch arrays
- **ID preservation** — Echoes back the exact request ID (string, number, float, null)
- **Streaming** — `SendStreamingMessage` and `SubscribeToTask` return SSE streams
- **CORS** — Configurable cross-origin headers
- **Content type** — Accepts `application/json`

### Batch Restrictions

Streaming methods cannot appear in batch requests:
- `SendStreamingMessage` in a batch → error response
- `SubscribeToTask` in a batch → error response

An empty batch `[]` returns a parse error.

## RestDispatcher

Routes RESTful HTTP requests to the handler:

```rust
use a2a_protocol_sdk::server::RestDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(RestDispatcher::new(handler));
```

### Route Table

| Method | Path | Handler |
|--------|------|---------|
| `POST` | `/message:send` | SendMessage |
| `POST` | `/message:stream` | SendStreamingMessage |
| `GET` | `/tasks` | ListTasks |
| `GET` | `/tasks/{id}` | GetTask |
| `POST` | `/tasks/{id}:cancel` | CancelTask |
| `GET\|POST` | `/tasks/{id}:subscribe` | SubscribeToTask |
| `GET` | `/extendedAgentCard` | GetExtendedAgentCard |
| `POST` | `/tasks/{id}/pushNotificationConfigs` | CreatePushConfig |
| `GET` | `/tasks/{id}/pushNotificationConfigs` | ListPushConfigs |
| `GET` | `/tasks/{id}/pushNotificationConfigs/{cfgId}` | GetPushConfig |
| `DELETE` | `/tasks/{id}/pushNotificationConfigs/{cfgId}` | DeletePushConfig |
| `GET` | `/.well-known/agent.json` | AgentCard |

### Multi-Tenancy

Tenant routes are prefixed with `/tenants/{tenant-id}/`:

```
GET /tenants/acme-corp/tasks
GET /tenants/acme-corp/tasks/{id}
POST /tenants/acme-corp/message:send
```

### Built-in Security

The REST dispatcher includes automatic protections:

| Protection | Behavior |
|-----------|----------|
| **Path traversal** | `..` in path segments (including `%2E%2E`, `%2e%2e`) → 400 |
| **Query string size** | Over 4 KiB → 414 |
| **Body size** | Over 4 MiB → 413 |
| **Content type** | Accepts `application/json` and `application/a2a+json` |

## Server Startup

### Using `serve()` (recommended)

Both dispatchers implement the `Dispatcher` trait, so you can use the `serve()` helper to eliminate hyper boilerplate:

```rust
use a2a_protocol_server::serve::{serve, serve_with_addr};

// Blocking — runs the accept loop on the current task
serve("127.0.0.1:3000", JsonRpcDispatcher::new(handler)).await?;

// Non-blocking — spawns the server and returns the bound address
let addr = serve_with_addr("127.0.0.1:0", dispatcher).await?;
println!("Listening on {addr}");
```

### Manual wiring (advanced)

Both dispatchers also expose a `dispatch` method for direct hyper integration:

```rust
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

## GrpcDispatcher

Routes gRPC requests to the handler via `tonic`. Enable with the `grpc` feature flag:

```toml
a2a-protocol-server = { version = "0.2", features = ["grpc"] }
```

```rust
use a2a_protocol_server::{GrpcDispatcher, GrpcConfig};
use std::sync::Arc;

let config = GrpcConfig::default()
    .with_max_message_size(8 * 1024 * 1024)
    .with_concurrency_limit(128);

let dispatcher = GrpcDispatcher::new(handler, config);

// Blocking server
dispatcher.serve("0.0.0.0:50051").await?;

// Non-blocking (returns bound address)
let addr = dispatcher.serve_with_addr("127.0.0.1:0").await?;
println!("gRPC listening on {addr}");

// Pre-bind pattern (when you need the address before building the handler)
let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr()?;
// ... build handler using addr for agent card URL ...
let dispatcher = GrpcDispatcher::new(handler, config);
let bound = dispatcher.serve_with_listener(listener)?;
```

### GrpcConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_message_size` | `usize` | 4 MiB | Maximum inbound/outbound message size |
| `concurrency_limit` | `usize` | 256 | Maximum concurrent gRPC requests per connection |
| `stream_channel_capacity` | `usize` | 64 | Bounded channel for streaming responses |

### Protocol

All 11 A2A methods are mapped to gRPC RPCs. JSON payloads are carried inside protobuf `bytes` fields, reusing the same serde types as JSON-RPC and REST — no duplicate protobuf definitions needed.

Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) use gRPC server streaming.

### Custom Server Setup

For advanced scenarios, use `into_service()` to get a tonic service:

```rust
let svc = dispatcher.into_service();
tonic::transport::Server::builder()
    .add_service(svc)
    .serve(addr)
    .await?;
```

## A2aRouter (Axum)

For projects already using Axum, the `axum` feature provides `A2aRouter` — an
idiomatic adapter that wraps `RequestHandler` as an `axum::Router`:

```toml
a2a-protocol-server = { version = "0.2", features = ["axum"] }
```

```rust
use a2a_protocol_server::A2aRouter;
use std::sync::Arc;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .with_agent_card(card)
        .build()
        .unwrap(),
);

let app = A2aRouter::new(handler).into_router();

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, app).await?;
```

### Composability

The returned `Router` can be merged with other Axum routes and middleware:

```rust
let app = axum::Router::new()
    .merge(A2aRouter::new(handler).into_router())
    .route("/custom", axum::routing::get(custom_handler));
```

### Routes

All 11 A2A REST methods are mapped, plus health check and agent card discovery.
Streaming methods return SSE responses. The router delegates entirely to
`RequestHandler` — zero business logic duplication.

## Running Multiple Transports

Serve JSON-RPC and REST on different ports with the same handler:

```rust
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
```

## CORS Configuration

Both dispatchers support CORS for browser-based clients:

```rust
use a2a_protocol_sdk::server::CorsConfig;

// The dispatchers handle OPTIONS preflight automatically.
// CORS headers are included on all responses.
```

## Next Steps

- **[Push Notifications](./push-notifications.md)** — Webhook delivery
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Production Hardening](../deployment/production.md)** — Deployment best practices
