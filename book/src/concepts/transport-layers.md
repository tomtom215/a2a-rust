# Transport Layers

A2A supports two transport bindings: **JSON-RPC 2.0** and **REST**. Both are first-class citizens in a2a-rust — the server can serve both simultaneously, and the client auto-selects based on the agent card.

## JSON-RPC 2.0

The JSON-RPC transport sends all requests to a single endpoint as POST requests with a JSON-RPC 2.0 envelope:

```json
{
  "jsonrpc": "2.0",
  "method": "SendMessage",
  "id": "req-1",
  "params": {
    "message": {
      "messageId": "msg-1",
      "role": "ROLE_USER",
      "parts": [{"text": "Hello, agent!"}]
    }
  }
}
```

Response:

```json
{
  "jsonrpc": "2.0",
  "id": "req-1",
  "result": {
    "id": "task-abc",
    "contextId": "ctx-123",
    "status": { "state": "TASK_STATE_COMPLETED" },
    "artifacts": [...]
  }
}
```

### Method Names

| A2A Operation | JSON-RPC Method |
|---------------|-----------------|
| Send message | `SendMessage` |
| Stream message | `SendStreamingMessage` |
| Get task | `GetTask` |
| List tasks | `ListTasks` |
| Cancel task | `CancelTask` |
| Subscribe to task | `SubscribeToTask` |
| Create push config | `CreateTaskPushNotificationConfig` |
| Get push config | `GetTaskPushNotificationConfig` |
| List push configs | `ListTaskPushNotificationConfigs` |
| Delete push config | `DeleteTaskPushNotificationConfig` |
| Extended card | `GetExtendedAgentCard` |

### Batching

JSON-RPC supports batch requests — multiple operations in a single HTTP request:

```json
[
  {"jsonrpc": "2.0", "method": "GetTask", "id": "1", "params": {"id": "task-a"}},
  {"jsonrpc": "2.0", "method": "GetTask", "id": "2", "params": {"id": "task-b"}}
]
```

> **Note:** Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) cannot be used in batch requests and will return an error.

### ID Handling

JSON-RPC request IDs can be strings, numbers (including 0 and floats), or `null`. The server preserves the exact ID type in the response.

## REST

The REST transport uses standard HTTP methods and URL paths:

| Operation | Method | Path |
|-----------|--------|------|
| Send message | `POST` | `/message:send` |
| Stream message | `POST` | `/message:stream` |
| Get task | `GET` | `/tasks/{id}` |
| List tasks | `GET` | `/tasks` |
| Cancel task | `POST` | `/tasks/{id}:cancel` |
| Subscribe | `GET` | `/tasks/{id}:subscribe` |
| Create push config | `POST` | `/tasks/{id}/pushNotificationConfigs` |
| Get push config | `GET` | `/tasks/{id}/pushNotificationConfigs/{configId}` |
| List push configs | `GET` | `/tasks/{id}/pushNotificationConfigs` |
| Delete push config | `DELETE` | `/tasks/{id}/pushNotificationConfigs/{configId}` |
| Agent card | `GET` | `/.well-known/agent.json` |

### Multi-Tenant Paths

With tenancy, paths are prefixed: `/tenants/{tenant-id}/tasks/{id}`

### Content Types

The REST dispatcher accepts both `application/json` and `application/a2a+json`.

### Security

The REST dispatcher includes built-in protections:

- **Path traversal rejection** — `..` in path segments (including percent-encoded `%2E%2E`) returns 400
- **Query string limits** — Query strings over 4 KiB return 414
- **Body size limits** — Request bodies over 4 MiB return 413

## WebSocket

The **WebSocket** transport (`websocket` feature flag) provides a persistent bidirectional channel over a single TCP connection. JSON-RPC 2.0 messages are exchanged as WebSocket text frames.

```toml
# Server
a2a-protocol-server = { version = "0.2", features = ["websocket"] }

# Client
a2a-protocol-client = { version = "0.2", features = ["websocket"] }
```

### Server

```rust
use a2a_protocol_server::{WebSocketDispatcher, RequestHandlerBuilder};
use std::sync::Arc;

let handler = Arc::new(RequestHandlerBuilder::new(my_executor).build().unwrap());
let dispatcher = Arc::new(WebSocketDispatcher::new(handler));

// Start accepting WebSocket connections
dispatcher.serve("0.0.0.0:3002").await?;
```

### Protocol

- Client sends JSON-RPC 2.0 requests as text frames
- Server responds with JSON-RPC 2.0 responses as text frames
- For streaming methods (`SendStreamingMessage`, `SubscribeToTask`), the server sends multiple frames — one per event — followed by a `stream_complete` response
- Ping/pong frames are handled automatically
- Connection closes cleanly on WebSocket close frame

### Client

```rust
use a2a_protocol_client::WebSocketTransport;

let transport = WebSocketTransport::connect("ws://agent.example.com:3002").await?;
let client = ClientBuilder::new("ws://agent.example.com:3002")
    .with_transport(transport)
    .build()?;
```

### When to Use WebSocket

- **Long-lived connections** — Avoids TCP/TLS handshake overhead per request
- **Bidirectional streaming** — Server can push events without SSE
- **Low latency** — No HTTP framing overhead for small messages

## Choosing a Transport

| Factor | JSON-RPC | REST | WebSocket |
|--------|----------|------|-----------|
| **Batch operations** | Supported | Not supported | Not supported |
| **Caching** | Limited (POST-only) | HTTP cache-friendly (GET) | Not applicable |
| **Tooling** | Needs JSON-RPC client | Standard HTTP tools work | WebSocket client needed |
| **URL structure** | Single endpoint | Resource-oriented | Single connection |
| **Streaming** | SSE via POST | SSE via POST/GET | Native text frames |
| **Connection reuse** | HTTP keep-alive | HTTP keep-alive | Persistent connection |

JSON-RPC and REST use SSE for streaming. WebSocket uses native text frames. The choice is mostly about ecosystem fit — JSON-RPC for agent-to-agent communication, REST for standard HTTP tooling, WebSocket for persistent low-latency connections.

## Running Both Transports

The server can serve both transports simultaneously on different ports:

```rust
use a2a_protocol_sdk::server::{JsonRpcDispatcher, RestDispatcher, RequestHandlerBuilder};
use std::sync::Arc;

let handler = Arc::new(
    RequestHandlerBuilder::new(my_executor).build().unwrap()
);

// JSON-RPC on port 3000
let jsonrpc = Arc::new(JsonRpcDispatcher::new(Arc::clone(&handler)));

// REST on port 3001
let rest = Arc::new(RestDispatcher::new(handler));
```

Both dispatchers share the same `RequestHandler`, which means they share the same task store, push config store, and executor.

## Next Steps

- **[Agent Cards & Discovery](./agent-cards.md)** — How transport URLs are advertised
- **[Streaming with SSE](./streaming.md)** — How real-time events work across transports
- **[Dispatchers](../building-agents/dispatchers.md)** — Server-side dispatcher configuration
