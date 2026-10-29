<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# WebSocket

The **WebSocket** transport (`websocket` feature flag) provides a persistent bidirectional channel over a single TCP connection. JSON-RPC 2.0 messages are exchanged as WebSocket text frames.

```toml
# Server
a2a-protocol-server = { version = "0.15", features = ["websocket"] }

# Client
a2a-protocol-client = { version = "0.15", features = ["websocket"] }
```

## Serving it

Serve the binding with `WebSocketDispatcher`; its setup, limits, TLS and
origin checks are in [Dispatchers](../../building-agents/dispatchers/websocket.md).

Declare the binding on the card you give the handler
(`AgentInterface::new("ws://agent.example.com:3002",
a2a_protocol_types::WEBSOCKET_BINDING_URI)`). The listener logs a warning at
start-up, with the `tracing` feature, when its binding is missing from the
card (spec §5.2). See
[Naming a protocol binding](../agent-cards/fields.md#naming-a-protocol-binding).

## Protocol

- Client sends JSON-RPC 2.0 requests as text frames
- Server responds with JSON-RPC 2.0 responses as text frames
- The v1.0 method names the JSON-RPC HTTP binding routes; the v0.3 `method/verb` aliases are refused with `MethodNotFound` (see the [binding specification](../../bindings/websocket.md))
- For streaming methods (`SendStreamingMessage`, `SubscribeToTask`), the server sends multiple frames — one per event — followed by a `stream_complete` response
- Ping/pong frames are handled automatically
- Connection closes cleanly on WebSocket close frame
- The upgrade request's HTTP headers reach the handler for every request on the connection, so authentication and tenant resolution work exactly as over HTTP; supply credentials at connect time (`WebSocketTransport::connect_with_config`)
- There is no resumption: the server drops a `Last-Event-ID` on the upgrade
  request, so a resubscribe starts from the task's snapshot
- Each call's span links to the trace the upgrade request carried rather than
  continuing it; a frame has no header to carry a per-call `traceparent`

The binding's full specification, including how it differs from JSON-RPC over
HTTP, is [WebSocket binding](../../bindings/websocket.md).

## Client

```rust,no_run
# use a2a_protocol_sdk::client::ClientBuilder;
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
# Ok(())
# }
```

Because the transport is built before the client and handed to
`with_custom_transport`, it never sees the `ClientConfig` — so `with_timeout`,
`with_connection_timeout` and `with_max_response_size` on the builder do not
reach it. Its equivalents are these:

| Knob | Default | Bounds |
|------|---------|--------|
| `connect_timeout` | 10s | the whole handshake — a server that accepts TCP and never upgrades |
| `request_timeout` | 30s | waiting for one response on an established connection |
| `max_message_size` | 32 MiB | an incoming frame, at the protocol level |
| `max_pending_requests` | 64 | requests awaiting a response on one connection; the next is refused with `ClientError::TooManyPendingRequests` (retryable) rather than queued |
| `with_tls_config` | Mozilla's roots, verified with `ring` | the certificate a `wss://` endpoint must present; `tls::tls_config_with_extra_roots` adds a private CA. Needs the `tls-rustls` feature (on by default); until 0.15 no `wss://` endpoint could be reached |

After the socket drops, the next call reconnects to the same endpoint with the
same configuration (one reconnect shared by concurrent callers, bounded by
`connect_timeout`). A stream keeps its connection open until you drop it, even
if the client is dropped first. A stream you read more slowly than the agent
writes ends with a `stream_lagged` error once 64 frames are waiting unread
(`ClientError::is_stream_lagged`); resubscribe to continue.

## When to use WebSocket

- **Long-lived connections** — Avoids TCP/TLS handshake overhead per request
- **Bidirectional streaming** — Server can push events without SSE
- **Low latency** — No HTTP framing overhead for small messages
