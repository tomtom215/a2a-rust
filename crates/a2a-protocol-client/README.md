<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-client

HTTP client for the A2A protocol v1.0 -- async, hyper-backed, with pluggable transports.

## Overview

- Full-featured async HTTP client for communicating with any A2A-compliant agent
- Built on hyper 1.x with tokio
- Pluggable transport bindings: JSON-RPC 2.0 (default), REST, WebSocket, gRPC
- Interceptor chain for auth, logging, custom middleware
- Automatic retry with jittered exponential backoff
- SSE streaming for real-time events

## Quick Start

Every Rust block in this README is compiled as a doctest of the crate; the
ones that need a live agent are compiled but not run.

```rust,no_run
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::{Message, MessageRole, MessageSendParams, Part};

# #[tokio::main]
# async fn main() -> Result<(), a2a_protocol_client::ClientError> {
let client = ClientBuilder::new("http://localhost:3000").build()?;
let message = Message::new("msg-1", MessageRole::User, vec![Part::text("Hello")]);
let response = client.send_message(MessageSendParams::new(message)).await?;
# let _ = response;
# Ok(())
# }
```

## Key Types

| Type | Purpose |
|------|---------|
| `A2aClient` | Main entry point -- all A2A methods |
| `ClientBuilder` | Fluent builder for client construction |
| `EventStream` | Async iterator over streaming events |
| `RetryPolicy` | Configurable jittered exponential backoff |
| `CallInterceptor` | Trait for request/response middleware |
| `AuthInterceptor` | Built-in auth header injection from a `CredentialsStore` |
| `BearerAuthInterceptor` | Bearer tokens from a `TokenProvider` (static, or OAuth2 client credentials) |
| `ClientError` | The error type; `#[non_exhaustive]`, so matches need a wildcard arm |

## A2A Methods

All 11 A2A v1.0 methods are methods of `A2aClient`:

- `send_message()` / `stream_message()` -- send, and stream the responses
- `get_task()` / `list_tasks()` / `cancel_task()` -- task management
- `subscribe_to_task()` / `subscribe_to_task_from()` -- real-time task updates, the second resuming after a known event
- `set_push_config()` / `get_push_config()` / `list_push_configs()` / `delete_push_config()` -- push notifications
- `get_extended_agent_card()` -- the authenticated extended agent card

## Features

| Feature | Default | Purpose |
|---------|---------|---------|
| `tls-rustls` | Yes | HTTPS via rustls (no OpenSSL) |
| `signing` | No | Forwards `a2a-protocol-types/signing`; the client signs and verifies nothing on its own |
| `tracing` | No | Structured logging (zero-cost when off) |
| `websocket` | No | WebSocket transport |
| `grpc` | No | gRPC transport via tonic (plaintext) |
| `grpc-tls` | No | gRPC over TLS (implies `grpc`; independent of `tls-rustls`); bundled roots or a pinned `ClientTlsConfig`, re-exported from `transport::grpc` |
| `testing` | No | A scripted hostile peer (`testing::ScriptedPeer`) that stalls, cuts off, mis-frames or refuses on each binding, for testing code that calls agents |

## Agent Discovery

```rust,no_run
use a2a_protocol_client::{A2aClient, ClientBuilder, resolve_agent_card};

# #[tokio::main]
# async fn main() -> Result<(), a2a_protocol_client::ClientError> {
let card = resolve_agent_card("http://localhost:3000").await?;
let client = ClientBuilder::from_card(&card)?.build()?;
// or, with no further configuration:
let client = A2aClient::from_card(&card)?;
# let _ = client;
# Ok(())
# }
```

`from_card` considers only interfaces for this SDK's protocol major (an agent
that also serves v0.3 is not connected to on that endpoint), in the order of
`ClientConfig::preferred_bindings` (`["JSONRPC"]` by default);
`ClientBuilder::from_card_preferring` takes an order of your own.

## Streaming

```rust,no_run
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::{Message, MessageRole, MessageSendParams, Part, StreamResponse};

# #[tokio::main]
# async fn main() -> Result<(), a2a_protocol_client::ClientError> {
# let client = ClientBuilder::new("http://localhost:3000").build()?;
# let params = MessageSendParams::new(Message::new("m", MessageRole::User, vec![Part::text("hi")]));
let mut stream = client.stream_message(params).await?;
while let Some(event) = stream.next().await {
    match event? {
        StreamResponse::StatusUpdate(ev) => println!("status: {:?}", ev.status.state),
        StreamResponse::ArtifactUpdate(ev) => println!("artifact: {}", ev.artifact.id),
        StreamResponse::Task(task) => println!("task: {}", task.id),
        StreamResponse::Message(msg) => println!("message: {:?}", msg.text()),
        // `StreamResponse` is `#[non_exhaustive]`.
        _ => {}
    }
}
# Ok(())
# }
```

## Interceptors & Auth

```rust
use std::sync::Arc;

use a2a_protocol_client::{
    AuthInterceptor, ClientBuilder, CredentialsStore, InMemoryCredentialsStore, RetryPolicy,
    SessionId,
};

let session = SessionId::new("session-1");
let store = Arc::new(InMemoryCredentialsStore::new());
store.set(session.clone(), "bearer", "my-token".into());

let client = ClientBuilder::new("http://localhost:3000")
    .with_interceptor(AuthInterceptor::new(store, session))
    .with_retry_policy(RetryPolicy::default())
    .build()?;
# let _ = client;
# Ok::<(), a2a_protocol_client::ClientError>(())
```

## Transport Selection

- `ClientBuilder::from_card` selects from the agent card's `supportedInterfaces`
- `ClientBuilder::with_protocol_binding` overrides it by name: `JSONRPC`,
  `HTTP+JSON` (the legacy `REST` is accepted) and, with the `grpc` feature,
  `GRPC`; names compare ignoring ASCII case
- WebSocket (feature `websocket`) is not chosen by name: connect a
  `transport::WebSocketTransport` and pass it to
  `ClientBuilder::with_custom_transport`, which takes any `Transport`
- JSON-RPC 2.0 is the default when nothing else is chosen

## License

Apache-2.0
