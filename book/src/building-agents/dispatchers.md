# Dispatchers (JSON-RPC, REST, Axum & gRPC)

Dispatchers translate HTTP/gRPC requests into handler calls. a2a-rust provides five built-in dispatchers: `JsonRpcDispatcher`, `RestDispatcher`, `A2aRouter` (`axum` feature), `WebSocketDispatcher` (`websocket` feature), and `GrpcDispatcher` (`grpc` feature).

## JsonRpcDispatcher

Routes JSON-RPC 2.0 requests to the handler:

```rust,ignore
use a2a_protocol_sdk::server::JsonRpcDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
```

### Features

- **Single endpoint** — All methods go to `/` as POST requests
- **Agent card** — `GET /.well-known/agent-card.json` returns the agent card (same as REST)
- **Batch support** — Handles JSON-RPC batch arrays
- **ID preservation** — Echoes back the exact request ID (string, number, float, null). The client validates that response IDs match request IDs.
- **Streaming** — `SendStreamingMessage` and `SubscribeToTask` return SSE streams
- **CORS** — Configurable cross-origin headers
- **Content type** — Accepts `application/json` and `application/a2a+json`
- **Version validation** — Validates `A2A-Version` header if present; rejects incompatible major versions with `VersionNotSupported` (-32009)

### Batch Restrictions

Streaming methods cannot appear in batch requests:
- `SendStreamingMessage` in a batch → error response
- `SubscribeToTask` in a batch → error response

An empty batch `[]` returns a parse error.

Batch size is limited by `DispatchConfig::max_batch_size` (default 100). Batches exceeding this limit are rejected with a parse error before any individual request is dispatched.

### DispatchConfig

Both JSON-RPC and REST dispatchers share a `DispatchConfig` for transport-level limits:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_request_body_size` | `usize` | 4 MiB | Maximum request body size in bytes |
| `body_read_timeout` | `Duration` | 30 seconds | Timeout for reading the full request body |
| `max_query_string_length` | `usize` | 4096 | Maximum query string length (REST only) |
| `sse_keep_alive_interval` | `Duration` | 30 seconds | Periodic `: keep-alive` comment interval for SSE |
| `sse_channel_capacity` | `usize` | 64 | Backpressure channel between event reader and HTTP response |
| `max_batch_size` | `usize` | 100 | Maximum requests in a JSON-RPC batch |

## RestDispatcher

Routes RESTful HTTP requests to the handler:

```rust,ignore
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
| `GET` | `/.well-known/agent-card.json` | AgentCard |

### Multi-Tenancy

Tenant-scoped routes accept two forms — the canonical bare-segment form from
the spec proto's `google.api.http` additional bindings (what official-SDK
REST clients send), and this SDK's original explicit prefix:

```text
# Canonical form
GET  /acme-corp/tasks
POST /acme-corp/message:send

# Explicit form
GET  /tenants/acme-corp/tasks
POST /tenants/acme-corp/message:send
```

In the canonical form, literal route segments always win over the tenant
variable (a tenant named like a route head must use the explicit form).

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

```rust,ignore
use a2a_protocol_server::serve::{serve, serve_with_addr};

// Blocking — runs the accept loop on the current task
serve("127.0.0.1:3000", JsonRpcDispatcher::new(handler)).await?;

// Non-blocking — spawns the server and returns the bound address
let addr = serve_with_addr("127.0.0.1:0", dispatcher).await?;
println!("Listening on {addr}");
```

### Manual wiring (advanced)

Both dispatchers also expose a `dispatch` method for direct hyper integration:

```rust,ignore
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

## WebSocketDispatcher

Provides bidirectional A2A communication over WebSocket. Enable with the `websocket` feature flag:

```toml
a2a-protocol-server = { version = "0.8", features = ["websocket"] }
```

```rust,ignore
use a2a_protocol_server::WebSocketDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(WebSocketDispatcher::new(handler));

// Blocking server
dispatcher.clone().serve("0.0.0.0:3000").await?;

// Non-blocking (returns bound address)
let addr = dispatcher.serve_with_addr("127.0.0.1:0").await?;
```

### Protocol

- Client sends JSON-RPC 2.0 requests as WebSocket text frames
- Server responds with JSON-RPC 2.0 responses as text frames
- Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) send one frame per event, followed by a final JSON-RPC success response
- The full A2A method surface is routed — the same method names (and v0.3
  `method/verb` aliases) as `JsonRpcDispatcher`, including the
  push-notification-config methods and `GetExtendedAgentCard`

### Authentication and tenancy

The HTTP headers of the upgrade request (lowercased, plus the request path
under `":path"`) are captured during the handshake and passed to the
handler for **every** request on the connection. Tenant resolvers, strict
multi-tenancy, and header-based authentication behave exactly as they do
over HTTP — credentials are presented once, at connect time, and apply to
the whole connection. An upgrade request whose `A2A-Version` header names
an unsupported major version is rejected during the handshake with
HTTP 400.

### Built-in Limits

| Limit | Value | Description |
|-------|-------|-------------|
| **Concurrent tasks per connection** | 64 | Per-connection `Semaphore(64)` prevents unbounded task spawning |
| **Incoming message size** | 4 MiB | Oversized WebSocket frames are rejected at the protocol level |
| **Handshake timeout** | 10 s (configurable via `with_handshake_timeout`) | A peer that never completes the upgrade is disconnected instead of pinning a connection |
| **Concurrent connections** | unbounded (`with_max_connections`) | The permit is taken before `accept()`, so load past the ceiling waits in the kernel's listen backlog rather than as spawned tasks |
| **Idle connection** | off (`with_idle_timeout`) | Closes a connection carrying no traffic in either direction for the given period |

Transient `accept()` errors (per-connection aborts, fd-table exhaustion)
never terminate the accept loop — it retries with the same backoff policy
as the HTTP serve path.

### Bounding a connection after the handshake

The handshake timeout only covers the part before the upgrade completes. A
peer that completes it and then goes silent held a task, a socket and a file
descriptor for the life of the process. Two knobs bound that, both opt-in:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::WebSocketDispatcher;
# use std::sync::Arc;
# use std::time::Duration;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).build().expect("handler"));
let dispatcher = Arc::new(
    WebSocketDispatcher::new(handler)
        .with_max_connections(1024)
        .with_idle_timeout(Duration::from_secs(75)),
);
# }
```

`with_idle_timeout` is **off by default**, unlike the equivalent on
`ServeConfig` for the HTTP bindings, and the difference is
deliberate. On HTTP, silence means nothing is happening. On a WebSocket it may
mean a subscription is waiting for its next event, which is a legitimate thing
to do for hours — a default that closed those would be a knob nobody enables.

What makes it safe to enable: at the halfway point of the budget the server
sends a WebSocket Ping. Every conformant client library answers it
automatically, and that Pong is traffic. So the timeout closes peers that are
**unresponsive**, not peers that are merely quiet. Outbound frames count too,
so a stream pushing events to a silent consumer keeps its own connection
alive.

## GrpcDispatcher

Routes gRPC requests to the handler via `tonic`. Enable with the `grpc` feature flag:

```toml
a2a-protocol-server = { version = "0.8", features = ["grpc"] }
```

```rust,ignore
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

### Custom Server Setup

For advanced scenarios, use `into_service()` to get a tonic service:

```rust,ignore
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
a2a-protocol-server = { version = "0.8", features = ["axum"] }
```

```rust,ignore
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

```rust,ignore
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

```rust,ignore
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

```rust,no_run
use a2a_protocol_sdk::server::CorsConfig;

// The dispatchers handle OPTIONS preflight automatically.
// CORS headers are included on all responses.
```

## Next Steps

- **[Push Notifications](./push-notifications.md)** — Webhook delivery
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Production Hardening](../deployment/production.md)** — Deployment best practices
