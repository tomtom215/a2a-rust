<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# JSON-RPC dispatcher

`JsonRpcDispatcher` serves the JSON-RPC 2.0 binding: every method is a `POST`
to one path. This page shows how to build one, what it does with batches and
notifications, and which version and content types it accepts.

Routes JSON-RPC 2.0 requests to the handler:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let handler = std::sync::Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build()?);
use a2a_protocol_sdk::server::JsonRpcDispatcher;
use std::sync::Arc;

let dispatcher = Arc::new(JsonRpcDispatcher::new(handler));
# Ok(())
# }
```

## Features

- **Single endpoint** — All methods go to `/` as POST requests
- **Agent card** — `GET /.well-known/agent-card.json` returns the agent card (same as REST)
- **Batch support** — Handles JSON-RPC batch arrays
- **ID preservation** — Echoes back the exact request ID (string, number, float, null). The client validates that response IDs match request IDs.
- **Notifications** — A request with no `id` member is run and gets no reply: `204 No Content` over HTTP, no entry in a batch's array (`204` when every member is one), no frame over WebSocket. `"id": null` is a call and is answered; a streaming method sent as a notification is not run
- **Streaming** — `SendStreamingMessage` and `SubscribeToTask` return SSE streams
- **CORS** — Configurable cross-origin headers
- **Content type** — Accepts `application/json` and `application/a2a+json`
- **Version validation** — Requires the `A2A-Version` header (or the `A2A-Version` query parameter, §3.6.1) by default: a request without it (read as v0.3, spec §3.6.2) or with a version other than 1.0 (a patch number is ignored; `1.1` is refused) is refused with `VersionNotSupported` (-32009). `DispatchConfig::accept_missing_version_header()` admits header-less requests

## Batch restrictions

Streaming methods cannot appear in batch requests:
- `SendStreamingMessage` in a batch → error response
- `SubscribeToTask` in a batch → error response

An empty batch `[]` is answered with Invalid Request (-32600).

Batch size is limited by `DispatchConfig::max_batch_size` (default 100). Batches exceeding this limit are rejected with Invalid Request (-32600) before any individual request is dispatched.

To serve it, and to set its body-size, read-time and batch limits
(`DispatchConfig`), see [Serving and combining](./serving.md).
