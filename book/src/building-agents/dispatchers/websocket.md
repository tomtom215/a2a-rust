<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# WebSocket: TLS and limits

The [`WebSocketDispatcher`](../dispatchers.md#websocketdispatcher) section
covers setup, the frame protocol and authentication. This page covers running
it in production: serving `wss://`, refusing other sites' pages, and bounding
how many connections it holds and for how long.

## TLS and origins

The listener is `ws://` by default. With the `tls-rustls` feature,
`with_tls(rustls::ServerConfig)` or `with_tls_pem(cert_chain, key)` makes it
`wss://` only, with the agent card served over the same TLS session.
`with_allowed_origins([...])` refuses, with 403 at the handshake, a browser
whose page is not on the list (cross-site WebSocket hijacking); an upgrade
with no `Origin` is still accepted unless `require_origin()` is set. See the
[binding's security considerations](../../bindings/websocket.md#7-security-considerations-134).

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::new("wss://agent.example.com", a2a_protocol_types::WEBSOCKET_BINDING_URI))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
# }
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::WebSocketDispatcher;
# use std::sync::Arc;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# async fn run() -> std::io::Result<()> {
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build().expect("handler"));
let dispatcher = Arc::new(
    WebSocketDispatcher::new(handler)
        .with_tls_pem(&std::fs::read("cert.pem")?, &std::fs::read("key.pem")?)?
        .with_allowed_origins(["https://app.example.com"]),
);
dispatcher.serve("0.0.0.0:443").await?;
# Ok(())
# }
# fn main() {}
```

## Built-in limits

| Limit | Value | Description |
|-------|-------|-------------|
| **Concurrent tasks per connection** | 64 | Per-connection `Semaphore(64)` prevents unbounded task spawning |
| **Incoming message size** | 4 MiB | Oversized WebSocket frames are rejected at the protocol level |
| **Handshake timeout** | 10 s (configurable via `with_handshake_timeout`) | A peer that never completes the upgrade is disconnected instead of pinning a connection |
| **Concurrent connections** | unbounded (`with_max_connections`) | The permit is taken before `accept()`, so load past the ceiling waits in the kernel's listen backlog rather than as spawned tasks |
| **Idle connection** | off (`with_idle_timeout`) | Closes a connection carrying no traffic in either direction for the given period |

Transient `accept()` errors (per-connection aborts, fd-table exhaustion)
never terminate the accept loop — it retries with the same backoff policy
as the HTTP serve path.

## Bounding a connection after the handshake

The handshake timeout only covers the part before the upgrade completes. A
peer that completes it and then goes silent held a task, a socket and a file
descriptor for the life of the process. Two knobs bound that, both opt-in:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::WebSocketDispatcher;
# use std::sync::Arc;
# use std::time::Duration;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# fn main() {
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build().expect("handler"));
let dispatcher = Arc::new(
    WebSocketDispatcher::new(handler)
        .with_max_connections(1024)
        .with_idle_timeout(Duration::from_secs(75)),
);
# }
```

`with_idle_timeout` is **off by default**, unlike the equivalent on
`ServeConfig` for the HTTP bindings, and the difference is
deliberate. On HTTP, silence means nothing is happening. On a WebSocket it may
mean a subscription is waiting for its next event, which is a legitimate thing
to do for hours — a default that closed those would be a knob nobody enables.

What makes it safe to enable: at the halfway point of the budget the server
sends a WebSocket Ping. Every conformant client library answers it
automatically, and that Pong is traffic. So the timeout closes peers that are
**unresponsive**, not peers that are merely quiet. Outbound frames count too,
so a stream pushing events to a silent consumer keeps its own connection
alive.
