<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-types

Pure A2A protocol v1.0 data types -- serde only, no I/O.

## Overview

- All A2A v1.0 wire types with zero I/O dependencies
- Just `serde` + `serde_json` for serialization
- Foundation crate used by `a2a-protocol-client` and `a2a-protocol-server`
- Use this crate when you need A2A types without the full HTTP stack

## Key Types

| Module | Key Types | Purpose |
|--------|-----------|---------|
| `agent_card` | `AgentCard`, `AgentCapabilities`, `AgentSkill` | Agent discovery (`/.well-known/agent-card.json`) |
| `task` | `Task`, `TaskStatus`, `TaskState`, `TaskId`, `ContextId` | Unit of work lifecycle |
| `message` | `Message`, `Part`, `MessageRole` | Communication with text/raw/url/data parts |
| `artifact` | `Artifact`, `ArtifactId` | Discrete agent outputs |
| `error` | `A2aError`, `ErrorCode`, `A2aResult` | Protocol errors (14 codes) |
| `jsonrpc` | `JsonRpcRequest`, `JsonRpcResponse` | JSON-RPC 2.0 envelope |
| `events` | `StreamResponse`, `TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent` | SSE streaming events |
| `security` | `SecurityScheme`, `SecurityRequirement` | API Key, OAuth 2.0, OpenID, mTLS |
| `params` | `MessageSendParams`, `TaskQueryParams`, `ListTasksParams` | Method parameters for all 11 A2A methods |
| `responses` | `SendMessageResponse`, `TaskListResponse` | Method result types |
| `push` | `TaskPushNotificationConfig` | Push notification config |
| `extensions` | `AgentExtension`, `AgentCardSignature` | Optional agent capabilities |
| `signing` | JWS signing functions | Agent card signing (feature-gated) |

## Features

| Feature | Default | Purpose |
|---------|---------|---------|
| `signing` | No | Agent card signing (JWS/ES256, RFC 8785 canonicalization). Adds `ring` and `base64`, and turns on serde_json's `float_roundtrip`. |
| `proto` | No | The canonical A2A protobuf messages (`lf.a2a.v1`, prost-generated) and lossless conversions to and from these types. |

## Usage

This block, like every Rust block in this README, is compiled and run as a
doctest of the crate.

```rust
use a2a_protocol_types::{AgentCard, Message, MessageRole, Part, StreamResponse};

// An agent card, as served at `/.well-known/agent-card.json`.
let json = r#"{
    "name": "Weather Agent",
    "description": "Provides weather forecasts",
    "version": "1.0.0",
    "supportedInterfaces": [{
        "url": "https://weather.example.com/rpc",
        "protocolBinding": "JSONRPC",
        "protocolVersion": "1.0"
    }],
    "defaultInputModes": ["text/plain"],
    "defaultOutputModes": ["text/plain"],
    "skills": [],
    "capabilities": {}
}"#;
let card: AgentCard = serde_json::from_str(json)?;
assert_eq!(card.name, "Weather Agent");

// A message to send.
let msg = Message::new("msg-001", MessageRole::User, vec![Part::text("Hello, agent!")]);
assert_eq!(msg.text(), Some("Hello, agent!"));

// Streaming events. `StreamResponse` is `#[non_exhaustive]`, so a match
// needs a wildcard arm for variants a later specification adds.
fn describe(event: &StreamResponse) -> &'static str {
    match event {
        StreamResponse::StatusUpdate(_) => "status",
        StreamResponse::ArtifactUpdate(_) => "artifact",
        StreamResponse::Task(_) => "task",
        StreamResponse::Message(_) => "message",
        _ => "newer event",
    }
}
# let _ = describe;
# Ok::<(), serde_json::Error>(())
```

## Protocol Constants

```rust
use a2a_protocol_types::{A2A_CONTENT_TYPE, A2A_VERSION, A2A_VERSION_HEADER};

assert_eq!(A2A_VERSION, "1.0");
assert_eq!(A2A_CONTENT_TYPE, "application/a2a+json");
assert_eq!(A2A_VERSION_HEADER, "A2A-Version");
```

## Design Notes

- **ID newtypes** for type safety: `TaskId`, `ContextId`, `MessageId`, `ArtifactId`
- **`#[non_exhaustive]`** on enums for forward-compatible additions
- **ProtoJSON naming**: `TaskState::Completed` serializes as `"TASK_STATE_COMPLETED"`
- **No unsafe library code**: `#![forbid(unsafe_code)]` covers `src/`. The build script is outside the attribute's reach and, with the `proto` feature, sets `PROTOC` through `std::env::set_var`, which edition 2024 makes `unsafe`
- **All types documented**: `#![deny(missing_docs)]`
- **Property-tested**: JSON ser/de round-trip verified via `proptest`

## License

Apache-2.0
