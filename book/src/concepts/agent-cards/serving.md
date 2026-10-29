<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Serving a card

A server publishes its card at `/.well-known/agent-card.json`. This page
covers the three handlers that serve it: static, dynamic and hot-reload.
Their HTTP caching headers are on [HTTP caching](./caching.md).

## Static handler

For agent cards that don't change at runtime:

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"])) }
# fn main() {
# let my_executor = MyAgent;
use a2a_protocol_sdk::server::RequestHandlerBuilder;

let handler = RequestHandlerBuilder::new(my_executor)
    .with_agent_card(make_agent_card())
    .build()
    .unwrap();
# }
```

When you give the handler a card with `with_agent_card`, every built-in
dispatcher serves it through `StaticAgentCardHandler` (also usable directly:
`StaticAgentCardHandler::new(&card)?`), which provides:
- **ETag** headers for cache validation
- **Last-Modified** timestamps
- **Cache-Control** directives
- **304 Not Modified** responses for conditional requests

## Dynamic handler

For agent cards that change (e.g., based on feature flags, load, or authentication):

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::{A2aResult, AgentInterface, AgentSkill};
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"])) }
use a2a_protocol_sdk::server::{AgentCardProducer, DynamicAgentCardHandler};
use a2a_protocol_sdk::types::agent_card::AgentCard;

struct MyCardProducer;

impl AgentCardProducer for MyCardProducer {
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
        Box::pin(async move {
            // Generate card dynamically
            Ok(make_agent_card())
        })
    }
}
```

The dynamic handler calls the producer on every request, computes a fresh ETag, and handles conditional caching.

`DynamicAgentCardHandler::new(MyCardProducer).handle(&req).await` answers a
`GET` or `HEAD` for the card; mount it on your own route. The built-in dispatchers
always serve, at that path, the static card given to `with_agent_card`.
`without_agent_card()` is the way to build a handler whose card is published
elsewhere, and it also turns off the checks the card drives (the
[capability flags and input modes](./fields.md#capabilities)). A produced card that fails
`AgentCard::validate` or `validate_security_schemes` is answered `500`.

## Hot-reload handler

For agent cards loaded from a JSON file that may change at runtime:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"])) }
# #[tokio::main]
# async fn main() {
# let initial_card = make_agent_card();
use a2a_protocol_sdk::server::HotReloadAgentCardHandler;
use std::path::Path;
use std::time::Duration;

let handler = HotReloadAgentCardHandler::new(initial_card);

// Cross-platform: poll the file every 30 seconds
let watcher = handler.spawn_poll_watcher(
    Path::new("/etc/a2a/agent.json"),
    Duration::from_secs(30),
);

// Unix only: reload on SIGHUP
#[cfg(unix)]
let signal_watcher = handler.spawn_signal_watcher(
    Path::new("/etc/a2a/agent.json"),
);
# }
```

`HotReloadAgentCardHandler` implements `AgentCardProducer`, so it plugs directly into `DynamicAgentCardHandler` for full HTTP caching support. The internal `Arc<RwLock<AgentCard>>` ensures updates are atomic with low contention for concurrent readers.

`reload_from_file`, `reload_from_json`, the poll watcher and the SIGHUP watcher
refuse a card that does not parse or is not valid, and keep serving the
current one. To replace the card in code, use `try_update`, which validates;
`update` is deprecated since 0.15.0.
