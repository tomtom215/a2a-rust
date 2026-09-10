# Transport Layers

A2A supports four transport bindings: **JSON-RPC 2.0**, **REST**, **WebSocket** (`websocket` feature flag), and **gRPC** (`grpc` feature flag). All four are first-class citizens in a2a-rust — the server can serve multiple transports simultaneously, and the client auto-selects based on the agent card.

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
    "task": {
      "id": "task-abc",
      "contextId": "ctx-123",
      "status": { "state": "TASK_STATE_COMPLETED" },
      "artifacts": [...]
    }
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
| Subscribe | `GET\|POST` | `/tasks/{id}:subscribe` |
| Create push config | `POST` | `/tasks/{id}/pushNotificationConfigs` |
| Get push config | `GET` | `/tasks/{id}/pushNotificationConfigs/{configId}` |
| List push configs | `GET` | `/tasks/{id}/pushNotificationConfigs` |
| Delete push config | `DELETE` | `/tasks/{id}/pushNotificationConfigs/{configId}` |
| Extended card | `GET` | `/extendedAgentCard` |
| Agent card | `GET` | `/.well-known/agent-card.json` |

### Multi-Tenant Paths

The spec's proto binds every method twice: a primary pattern with no tenant in
the path (`/tasks/{id}`) and an `additional_bindings` pattern with the tenant
as the leading segment (`/{tenant}/tasks/{id}`). This SDK's client sends the
prefix form — the same one the official Python SDK's REST client sends, and
the only one the official Python server reads — with the segment
percent-encoded, and keeps the field in POST bodies as well.

The server accepts the tenant from every place the proto lets it arrive, in
this order of precedence:

1. the path prefix — `/{tenant}/…` (canonical) or this SDK's explicit
   `/tenants/{tenant-id}/…` form, percent-decoded;
2. the `?tenant=` query parameter on `GET` and `DELETE` (§11.5's transcoding
   of the primary pattern);
3. the `tenant` field of a `POST` body (`message:send`, `message:stream`,
   `tasks/{id}:cancel`, `tasks/{id}:subscribe`, push-config create).

A path tenant is injected into a POST body that omits it, so
`POST /acme/message:send` with no `tenant` in the JSON lands in `acme`. When
the path and the body disagree the path wins, as a path variable does under
`google.api.http`.

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
a2a-protocol-server = { version = "0.8", features = ["websocket"] }

# Client
a2a-protocol-client = { version = "0.8", features = ["websocket"] }
```

### Server

```rust,ignore
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
- The full A2A method surface is routed — the same method names (and v0.3 aliases) as the JSON-RPC HTTP binding
- For streaming methods (`SendStreamingMessage`, `SubscribeToTask`), the server sends multiple frames — one per event — followed by a `stream_complete` response
- Ping/pong frames are handled automatically
- Connection closes cleanly on WebSocket close frame
- The upgrade request's HTTP headers reach the handler for every request on the connection, so authentication and tenant resolution work exactly as over HTTP; supply credentials at connect time (`WebSocketTransport::connect_with_config`)

### Client

```rust,ignore
use std::time::Duration;
use a2a_protocol_client::{WebSocketTransport, WebSocketTransportConfig};

// Defaults are fine for a trusted agent on a local network:
let transport = WebSocketTransport::connect("ws://agent.example.com:3002").await?;

// Or set the bounds explicitly — see the table below for what each one covers:
let transport = WebSocketTransport::connect_with_config(
    "wss://agent.example.com:3002",
    WebSocketTransportConfig::default()
        .with_connect_timeout(Duration::from_secs(5))   // TCP + TLS + upgrade
        .with_request_timeout(Duration::from_secs(30))  // per-request response wait
        .with_max_message_size(4 * 1024 * 1024),        // incoming frame ceiling
)
.await?;

let client = ClientBuilder::new("wss://agent.example.com:3002")
    .with_custom_transport(transport)
    .build()?;
```

Because the transport is built before the client and handed to
`with_custom_transport`, it never sees the `ClientConfig` — so `with_timeout`,
`with_connection_timeout` and `with_max_response_size` on the builder do not
reach it. Its equivalents are the three above:

| Knob | Default | Bounds |
|------|---------|--------|
| `connect_timeout` | 10s | the whole handshake — a server that accepts TCP and never upgrades |
| `request_timeout` | 30s | waiting for one response on an established connection |
| `max_message_size` | 32 MiB | an incoming frame, at the protocol level |
| `max_pending_requests` | 64 | requests awaiting a response on one connection; the next is refused with `ClientError::TooManyPendingRequests` (retryable) rather than queued |

### When to Use WebSocket

- **Long-lived connections** — Avoids TCP/TLS handshake overhead per request
- **Bidirectional streaming** — Server can push events without SSE
- **Low latency** — No HTTP framing overhead for small messages

## gRPC

The **gRPC** transport (`grpc` feature flag) provides high-performance RPC via protocol buffers and HTTP/2. As of 0.7 it is **protobuf-native**: the transport speaks the canonical `lf.a2a.v1.A2AService` service with fully-typed messages generated from the A2A specification's protobuf schema, making it wire-compatible with the official Go, Python, and Java SDKs. (Through 0.6 it tunneled JSON inside a protobuf `bytes` field on a non-standard service. The tunnel **client** was removed in 0.7; the tunnel **service** and the `grpc-legacy-json` feature that served it alongside the canonical one were removed in 0.8. A 0.6 client must now upgrade rather than be tunneled for.)

```toml
# Server
a2a-protocol-server = { version = "0.8", features = ["grpc"] }

# Client
a2a-protocol-client = { version = "0.8", features = ["grpc"] }
```

### Server

```rust,ignore
use a2a_protocol_server::{GrpcDispatcher, GrpcConfig};
use std::sync::Arc;

let handler = Arc::new(RequestHandlerBuilder::new(my_executor).build().unwrap());
let config = GrpcConfig::default()
    .with_max_message_size(8 * 1024 * 1024);
let dispatcher = GrpcDispatcher::new(handler, config);
dispatcher.serve("0.0.0.0:50051").await?;
```

> **Tip:** Use `serve_with_listener()` when you need to know the server address before constructing the handler (e.g., for agent cards with correct URLs). Pre-bind a `TcpListener`, extract the address, build your handler, then pass the listener.

### Client

An Agent Card's gRPC interface advertises a gRPC **target** — `host:port`,
per the proto's `AgentInterface.url` comment — not a URL, because gRPC names
carry no scheme. `GrpcTransport::connect` and `ClientBuilder::build_grpc`
accept that form, and decide how to dial it with a `GrpcBareAddressScheme`:

| Policy | Bare `host:port` is dialled… | When |
|---|---|---|
| `HttpsExceptLoopback` (default) | with TLS, except `localhost` / `127.0.0.0/8` / `::1` in plaintext | the spec requires TLS in production (§13); a loopback peer is this machine |
| `Https` | always with TLS | a loopback TLS terminator, or to remove the exception |
| `Http` | always in plaintext | a private network whose agents advertise `agent:50051` and terminate TLS in a mesh or sidecar |

An address that already carries `http://` or `https://` is used as-is.
`https://` — explicit or chosen by the policy — needs the `grpc-tls` feature;
without it the connect fails with a message naming the feature rather than
attempting a plaintext handshake against a TLS port.

```rust,ignore
use a2a_protocol_client::{ClientBuilder, GrpcBareAddressScheme};

// From a card whose gRPC interface says "grpc.example.com:443": TLS,
// verified against the bundled Mozilla roots (needs `grpc-tls`).
let client = ClientBuilder::from_card(&card)?.build_grpc().await?;

// A Compose network where the card says "analyzer:50051" and the mesh
// terminates TLS: plaintext, on purpose.
let client = ClientBuilder::from_card(&card)?
    .with_grpc_bare_address_scheme(GrpcBareAddressScheme::Http)
    .build_grpc()
    .await?;

// A private CA, pinned (needs `grpc-tls`). The TLS types are re-exported
// from `transport::grpc`, so no tonic dependency of your own.
use a2a_protocol_client::transport::grpc::{Certificate, ClientTlsConfig};

let tls = ClientTlsConfig::new()
    .ca_certificate(Certificate::from_pem(ca_pem))
    .domain_name("agent.internal");
let client = ClientBuilder::from_card(&card)?
    .with_grpc_tls_config(tls)
    .build_grpc()
    .await?;
```

#### Serving TLS

The server's gRPC listener is plaintext with the `grpc` feature — the shape
the official SDKs' gRPC servers expect, with TLS terminated in a proxy or
mesh. With `grpc-tls` on `a2a-protocol-server` it can serve TLS itself:
`GrpcDispatcher::with_tls` takes a `ServerTlsConfig` carrying the server
certificate and key and, for mutual TLS, the CA that client certificates
must chain to. The types are re-exported from `dispatch::grpc`, so no tonic
dependency of your own. The listener then speaks TLS only; a plaintext
client is refused at the handshake rather than served.

```rust
# use std::sync::Arc;
# use a2a_protocol_server::RequestHandler;
use a2a_protocol_server::dispatch::grpc::{
    Certificate, GrpcConfig, GrpcDispatcher, Identity, ServerTlsConfig,
};

# async fn example(
#     handler: Arc<RequestHandler>,
#     cert_pem: &str,
#     key_pem: &str,
#     client_ca_pem: &str,
# ) -> std::io::Result<()> {
let tls = ServerTlsConfig::new()
    .identity(Identity::from_pem(cert_pem, key_pem))
    // Mutual TLS: clients must present a certificate this CA signed.
    // Add `.client_auth_optional(true)` to admit clients that present none.
    .client_ca_root(Certificate::from_pem(client_ca_pem));

GrpcDispatcher::new(handler, GrpcConfig::default())
    .with_tls(tls)
    .serve("0.0.0.0:50051")
    .await?;
# Ok(())
# }
```

A configuration tonic rejects — a key that does not match its certificate —
comes back from the `serve*` call as an `std::io::Error`, not a panic. tonic
builds its acceptor from the process-level rustls crypto provider; when none
is installed and more than one is linked, the dispatcher installs `ring`
rather than letting rustls panic, and never overrides a provider the
application installed first.

### Protocol

- All 11 A2A methods are mapped to gRPC RPCs on the canonical `lf.a2a.v1.A2AService`
- Streaming methods (`SendStreamingMessage`, `SubscribeToTask`) use gRPC server streaming
- Messages are the fully-typed `lf.a2a.v1` protobuf types, converted to/from the serde domain types via a bidirectional `TryFrom` layer (ProtoJSON semantics)
- The canonical schema lives at `proto/a2a_v1/a2a.proto`, kept byte-identical to the specification copy

### When to Use gRPC

- **Service mesh integration** — gRPC is native to Kubernetes, Istio, Envoy
- **Language interop** — gRPC has code generation for 10+ languages
- **HTTP/2 multiplexing** — Multiple RPCs over a single connection
- **Streaming** — Native server streaming without SSE

## Choosing a Transport

| Factor | JSON-RPC | REST | WebSocket | gRPC |
|--------|----------|------|-----------|------|
| **Batch operations** | Supported | Not supported | Not supported | Not supported |
| **Caching** | Limited (POST-only) | HTTP cache-friendly (GET) | Not applicable | Not applicable |
| **Tooling** | Needs JSON-RPC client | Standard HTTP tools work | WebSocket client needed | gRPC client needed |
| **URL structure** | Single endpoint | Resource-oriented | Single connection | Single connection |
| **Streaming** | SSE via POST | SSE via POST/GET | Native text frames | Native server streaming |
| **Connection reuse** | HTTP keep-alive | HTTP keep-alive | Persistent connection | HTTP/2 multiplexing |

JSON-RPC and REST use SSE for streaming. WebSocket uses native text frames. gRPC uses native server streaming over HTTP/2. The choice is mostly about ecosystem fit — JSON-RPC for agent-to-agent communication, REST for standard HTTP tooling, WebSocket for persistent low-latency connections, gRPC for service mesh and cross-language interop.

## Running Both Transports

The server can serve both transports simultaneously on different ports:

```rust,ignore
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

All dispatchers share the same `RequestHandler`, which means they share the same task store, push config store, and executor.

## Next Steps

- **[Agent Cards & Discovery](./agent-cards.md)** — How transport URLs are advertised
- **[Streaming with SSE](./streaming.md)** — How real-time events work across transports
- **[Dispatchers](../building-agents/dispatchers.md)** — Server-side dispatcher configuration
