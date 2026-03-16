# Dispatchers (JSON-RPC & REST)

Dispatchers translate HTTP requests into handler calls. a2a-rust provides two built-in dispatchers matching the A2A spec's transport bindings.

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
| `GET` | `/tasks/{id}:subscribe` | SubscribeToTask |
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

## Wiring to Hyper

Both dispatchers expose a `dispatch` method that takes a `hyper::Request` and returns a `hyper::Response`:

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

## Running Both Transports

Serve JSON-RPC and REST on different ports with the same handler:

```rust
let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .with_agent_card(make_agent_card("http://localhost:3000", "http://localhost:3001"))
        .build()
        .unwrap(),
);

// JSON-RPC on port 3000
let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));
tokio::spawn(start_server(jsonrpc, "127.0.0.1:3000"));

// REST on port 3001
let rest = Arc::new(RestDispatcher::new(handler));
tokio::spawn(start_server(rest, "127.0.0.1:3001"));
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
