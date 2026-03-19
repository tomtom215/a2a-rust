<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-sdk

Umbrella re-export crate for the A2A protocol v1.0 Rust SDK.

## Overview

- Re-exports `a2a-protocol-types`, `a2a-protocol-client`, and `a2a-protocol-server`
- Provides a `prelude` module with the most commonly used types
- Single dependency for applications that need the full SDK

## Quick Start

```rust
use a2a_protocol_sdk::prelude::*;
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

All features are pass-through to the constituent crates:

| Feature | Source Crate |
|---------|-------------|
| `signing` | `a2a-protocol-types` |
| `tracing` | `a2a-protocol-client`, `a2a-protocol-server` |
| `tls-rustls` | `a2a-protocol-client` |
| `grpc` | `a2a-protocol-client`, `a2a-protocol-server` |
| `otel` | `a2a-protocol-server` |
| `websocket` | `a2a-protocol-client`, `a2a-protocol-server` |
| `sqlite` | `a2a-protocol-server` |
| `postgres` | `a2a-protocol-server` |
| `axum` | `a2a-protocol-server` |

## When to Use

- **Use `a2a-protocol-sdk`** when building a full application (agent + client)
- **Use individual crates** when you need only types, or only client, or only server

## License

Apache-2.0
