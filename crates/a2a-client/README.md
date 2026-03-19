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

```rust
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::params::MessageSendParams;

let client = ClientBuilder::new("http://localhost:3000").build()?;
let response = client.send_message(params).await?;
```

## Key Types

| Type | Purpose |
|------|---------|
| `A2aClient` | Main entry point -- all A2A methods |
| `ClientBuilder` | Fluent builder for client construction |
| `EventStream` | Async iterator over SSE streaming events |
| `RetryPolicy` | Configurable jittered exponential backoff |
| `CallInterceptor` | Trait for request/response middleware |
| `AuthInterceptor` | Built-in auth header injection |
| `ClientError` | Rich error type with 10 variants |

## A2A Methods

All 11 A2A v1.0 methods are supported:

- `send_message()` / `stream_message()` -- Send and stream responses
- `get_task()` / `list_tasks()` / `cancel_task()` -- Task management
- `subscribe_to_task()` / `resubscribe()` -- Real-time task updates
- `set_push_config()` / `get_push_config()` / `list_push_configs()` / `delete_push_config()` -- Push notifications
- `get_authenticated_extended_card()` -- Extended agent info

## Features

| Feature | Default | Purpose |
|---------|---------|---------|
| `tls-rustls` | Yes | HTTPS via rustls (no OpenSSL) |
| `signing` | No | Agent card signature verification |
| `tracing` | No | Structured logging (zero-cost when off) |
| `websocket` | No | WebSocket transport |
| `grpc` | No | gRPC transport via tonic |

## Agent Discovery

```rust
use a2a_protocol_client::resolve_agent_card;

let card = resolve_agent_card("http://localhost:3000").await?;
let client = ClientBuilder::from_card(&card)?.build()?;
// or shorthand:
let client = A2aClient::from_card(&card)?;
```

HTTP caching supported via ETag / If-None-Match / If-Modified-Since.

## Streaming

```rust
let mut stream = client.stream_message(params).await?;
while let Some(event) = stream.next().await {
    match event? {
        StreamResponse::StatusUpdate(ev) => { /* ... */ }
        StreamResponse::ArtifactUpdate(ev) => { /* ... */ }
        StreamResponse::Task(task) => { /* ... */ }
        StreamResponse::Message(msg) => { /* ... */ }
    }
}
```

## Interceptors & Auth

```rust
use a2a_protocol_client::{AuthInterceptor, InMemoryCredentialsStore, SessionId};

let store = Arc::new(InMemoryCredentialsStore::new());
store.set(session.clone(), "bearer", "my-token".into());

let client = ClientBuilder::new("http://localhost:3000")
    .with_interceptor(AuthInterceptor::new(store, session))
    .with_retry_policy(RetryPolicy::default())
    .build()?;
```

## Transport Selection

- Auto-selects from agent card's `supported_interfaces`
- Override via `ClientBuilder::with_transport()`
- JSON-RPC 2.0 is default (most widely supported)

## License

Apache-2.0
