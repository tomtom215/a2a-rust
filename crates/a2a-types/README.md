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

| Feature | Purpose |
|---------|---------|
| `signing` | Agent card signing (JWS/ES256, RFC 8785 canonicalization). Adds `ring` and `base64`. |

## Usage

```rust
use a2a_protocol_types::*;

// Agent card discovery
let card: AgentCard = serde_json::from_str(json)?;

// Create a message
let msg = Message {
    id: MessageId::new("msg-001"),
    role: MessageRole::User,
    parts: vec![Part::text("Hello, agent!")],
    metadata: None,
};

// Streaming events
match event {
    StreamResponse::StatusUpdate(ev) => { /* ... */ }
    StreamResponse::ArtifactUpdate(ev) => { /* ... */ }
    StreamResponse::Task(task) => { /* ... */ }
    StreamResponse::Message(msg) => { /* ... */ }
}
```

## Protocol Constants

- `A2A_VERSION` -- `"1.0.0"`
- `A2A_CONTENT_TYPE` -- `"application/a2a+json"`
- `A2A_VERSION_HEADER` -- `"A2A-Version"`

## Design Notes

- **ID newtypes** for type safety: `TaskId`, `ContextId`, `MessageId`, `ArtifactId`
- **`#[non_exhaustive]`** on enums for forward-compatible additions
- **ProtoJSON naming**: `TaskState::Completed` serializes as `"TASK_STATE_COMPLETED"`
- **No unsafe code**: `#![deny(unsafe_op_in_unsafe_fn)]`
- **All types documented**: `#![deny(missing_docs)]`
- **Property-tested**: JSON ser/de round-trip verified via `proptest`

## License

Apache-2.0
