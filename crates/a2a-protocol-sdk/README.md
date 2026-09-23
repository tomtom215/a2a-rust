<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-sdk

Umbrella re-export crate for the A2A protocol v1.0 Rust SDK.

## Overview

- Re-exports `a2a-protocol-types`, `a2a-protocol-client`, and `a2a-protocol-server`
- Provides a `prelude` module with the most commonly used types
- Single dependency for applications that need the full SDK

## Quick Start

Every Rust block in this README is compiled as a doctest of the crate.

```rust
use a2a_protocol_sdk::prelude::*;

struct Echo;

agent_executor!(Echo, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Completed).await
});

let card = AgentCard::new("echo", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"));
let handler = RequestHandlerBuilder::new(Echo).with_agent_card(card).build()?;
let _dispatcher = JsonRpcDispatcher::new(std::sync::Arc::new(handler));

let _client = ClientBuilder::new("http://localhost:3000").build()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Modules

| Module | Re-exports |
|--------|------------|
| `types` | All A2A wire types (`a2a-protocol-types`) |
| `client` | HTTP client (`a2a-protocol-client`) |
| `server` | Server framework (`a2a-protocol-server`) |
| `prelude` | Most common types from all three |

## Prelude Contents

- **Wire types**: `Task`, `TaskState`, `Message`, `Part`, `AgentCard`, `StreamResponse`
- **ID types**: `TaskId`, `ContextId`, `MessageId`, `ArtifactId`
- **Client**: `A2aClient`, `ClientBuilder`, `EventStream`, `RetryPolicy`
- **Server**: `AgentExecutor`, `RequestHandler`, `RequestHandlerBuilder`, `EventEmitter`, `JsonRpcDispatcher`, `RestDispatcher`
- **Errors**: `A2aError`, `ClientError`, `ServerError`

## Features

Each feature forwards to the constituent crates named; `tls-rustls` and
`tracing` are the defaults. The SDK takes those crates without their own
defaults, so `default-features = false` here removes both.

| Feature | Default | Forwards to |
|---------|---------|-------------|
| `tls-rustls` | Yes | `a2a-protocol-client`, `a2a-protocol-server` |
| `signing` | No | `a2a-protocol-types`, `a2a-protocol-client`, `a2a-protocol-server` |
| `tracing` | Yes | `a2a-protocol-client`, `a2a-protocol-server` |
| `grpc` | No | `a2a-protocol-client`, `a2a-protocol-server` |
| `grpc-tls` | No | `a2a-protocol-client`, `a2a-protocol-server` (and turns on `grpc`, `tls-rustls`) |
| `otel` | No | `a2a-protocol-server` |
| `websocket` | No | `a2a-protocol-client`, `a2a-protocol-server` |
| `sqlite` | No | `a2a-protocol-server` |
| `postgres` | No | `a2a-protocol-server` |
| `axum` | No | `a2a-protocol-server` |
| `auth-jwt` | No | `a2a-protocol-server` |

The server's `conformance` feature and the types crate's `proto` feature
are not forwarded; depend on those crates directly to enable them.

## When to Use

- **Use `a2a-protocol-sdk`** when building a full application (agent + client)
- **Use individual crates** when you need only types, or only client, or only server

## License

Apache-2.0
