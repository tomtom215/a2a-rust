<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-rig

Serve a [rig](https://github.com/0xPlaygrounds/rig) agent over the
[Agent2Agent (A2A) protocol](https://a2a-protocol.org/).

`RigExecutor` wraps any `rig_core::completion::CompletionModel` as an
`a2a_protocol_server::AgentExecutor`, so a rig model answers A2A `message/send`
and `message/stream` calls over JSON-RPC, REST, WebSocket or gRPC without the
caller writing protocol code.

```toml
[dependencies]
a2a-rig = "0.1"
a2a-protocol-server = "0.12"
rig-core = "0.42"
```

```rust,no_run
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::dispatch::JsonRpcDispatcher;
use a2a_rig::{RigExecutor, agent_card};
use rig_core::client::CompletionClient;
use rig_core::providers::openai;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let client = openai::CompletionsClient::builder()
    .api_key(&std::env::var("OPENAI_API_KEY")?)
    .build()?;

let handler = RequestHandlerBuilder::new(
    RigExecutor::new(client.completion_model("gpt-4o-mini"))
        .with_preamble("You are a helpful assistant."),
)
.with_agent_card(agent_card("https://agent.example.com", "gpt-4o-mini").build())
.build()?;

let dispatcher = JsonRpcDispatcher::new(std::sync::Arc::new(handler));
# let _ = dispatcher;
# Ok(())
# }
```

## What it does, and what it does not

One A2A message becomes one rig completion request. The message's first text part
is the user turn; the model's text blocks are concatenated into a single artifact.

- **No tool loop.** A tool call in the response is skipped, because a single-turn
  bridge has no loop to hand it to.
- **No conversation history.** A2A `contextId` is not mapped onto rig chat
  history. Multi-turn state is the caller's to hold.
- **No token-level streaming into artifacts.** The answer is emitted as one
  artifact when the completion returns. A2A streaming still works — clients
  subscribe and receive `Working` then the artifact then `Completed` — but the
  artifact does not arrive in chunks.

These are rig's agent-layer concerns, and a bridge claiming to cover them would
be claiming fidelity it does not have.

**Cancellation is real.** `execute` races the completion against the A2A
cancellation token, so `tasks/cancel` stops waiting on the provider rather than
running to completion and discarding the answer. The task then ends `Canceled`,
and a cancel that lands while the artifact is being written does not get
overwritten by `Completed`.

## Versioning

`a2a-protocol-server`, `a2a-protocol-types` and `rig-core` are all **public**
dependencies: `RigExecutor` implements a trait from the first, the card builder
returns a type from the second, and the executor is generic over a trait from the
third. Requirements are therefore tight (`0.12`, `0.42`) rather than ranges — a
range lets cargo link two copies and hands the caller
`expected AgentExecutor, found AgentExecutor`. Every minor bump in any of the
three means a release here.

This crate is versioned and released independently of the `a2a-protocol-*` SDK,
and is not a member of its workspace.

## Status

Built against `rig-core` 0.42. The MSRV is declared as 1.88, which is what the
A2A SDK crates require; `rig-core` declares no `rust-version`, so the rig half of
that floor is inherited from its dependency tree rather than tested.

Related: [rig#391](https://github.com/0xPlaygrounds/rig/issues/391) asks for A2A
support in rig itself. This crate is the out-of-tree form of that, which is the
route rig's `CONTRIBUTING.md` suggests for companion crates.

## Licence

Apache-2.0.
