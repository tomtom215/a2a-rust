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
By default every browser is refused, with 403 at the handshake (cross-site
WebSocket hijacking); `with_allowed_origins([...])` lets the pages on the
list in, and `allow_any_origin()` lets every page in. An upgrade with no
`Origin`, as a client that is not a browser sends, is accepted unless
`require_origin()` is set. See the
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

## On the HTTP port

Since 0.16.0 the binding can share the port of the HTTP bindings (audit B-25,
[ADR 0055](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0055-websocket-on-the-http-port.md)):
`CombinedDispatcher::with_websocket` upgrades an HTTP/1.1 `GET` with
`Upgrade: websocket` at the JSON-RPC path and serves the connection with the
`WebSocketDispatcher`'s settings. Declare it in the card at the same URL,
`ws://` or `wss://` as the listener's TLS is.

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("https://agent.example.com"))
#         .with_interface(AgentInterface::rest("https://agent.example.com"))
#         .with_interface(AgentInterface::new("wss://agent.example.com", a2a_protocol_types::WEBSOCKET_BINDING_URI))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
# }
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_server::{CombinedDispatcher, WebSocketDispatcher};
# use a2a_protocol_server::serve::Server;
# use std::sync::Arc;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# async fn run(shutdown: impl std::future::Future<Output = ()> + Send) -> std::io::Result<()> {
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card()).build().expect("handler"));
let dispatcher = CombinedDispatcher::new(Arc::clone(&handler)).with_websocket(
    WebSocketDispatcher::new(handler).with_allowed_origins(["https://app.example.com"]),
);
let report = Server::bind("0.0.0.0:8443")
    .await?
    .with_tls_pem(&std::fs::read("cert.pem")?, &std::fs::read("key.pem")?)?
    .serve_with_shutdown(dispatcher, shutdown)
    .await;
# let _ = report;
# Ok(())
# }
# fn main() {}
```

What differs from the separate listener:

- The interceptor chain runs on the upgrade request's headers before the
  `101`, as a call to `WebSocketUpgrade`: an upgrade without the credential
  is refused `401` and holds nothing.
- A page of the server's own origin is accepted besides the listed ones;
  any other origin is still `403`.
- The connection keeps the HTTP server's `max_connections` slot until it
  closes, and the dispatcher's `with_max_connections` caps upgrades (`503`
  past it). The HTTP idle timeout is suspended for it; the WebSocket idle
  bound and its Ping govern instead.
- `Server::serve_with_shutdown` closes it in the drain, after the tasks in
  flight have ended, with a Close frame. The `WebSocketDispatcher`'s TLS,
  handshake timeout and listener settings do not apply: the connection is the
  HTTP server's.

The axum router has the same route (`A2aRouter::with_websocket`, at `/`);
`axum::serve` does not know upgraded connections, so close them in the
application's shutdown with `A2aRouter::websocket_upgrades()`'s `close`.

## Built-in limits

| Limit | Value | Description |
|-------|-------|-------------|
| **Concurrent tasks per connection** | 64 | Per-connection `Semaphore(64)` prevents unbounded task spawning |
| **Incoming message size** | 4 MiB | Oversized WebSocket frames are rejected at the protocol level |
| **Handshake timeout** | 10 s (configurable via `with_handshake_timeout`) | A peer that never completes the upgrade is disconnected instead of pinning a connection |
| **Concurrent connections** | unbounded (`with_max_connections`) | The permit is taken before `accept()`, so load past the ceiling waits in the kernel's listen backlog rather than as spawned tasks |
| **Idle connection** | 75 s, pinging at half (`with_idle_timeout`, `without_idle_timeout`) | Closes a connection that answers nothing for the given period; off until 0.16.0 |

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
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::new("ws://localhost:3000", a2a_protocol_types::WEBSOCKET_BINDING_URI))
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

The idle bound is **on by default since 0.16.0**, at `DEFAULT_WS_IDLE_TIMEOUT`
(75 s), as `ServeConfig`'s is for the HTTP bindings; it was off until then
([ADR 0042](https://github.com/tomtom215/a2a-rust/blob/main/docs/adr/0042-listener-liveness-defaults.md)),
and `without_idle_timeout()` turns it off. On a WebSocket, silence may mean a
subscription is waiting for its next event, which is a legitimate thing to do
for hours, so a plain idle timeout would close healthy connections.

What makes it safe on by default: at the halfway point of the budget the server
sends a WebSocket Ping. Every conformant client library answers it
automatically, and that Pong is traffic. So the timeout closes peers that are
**unresponsive**, not peers that are merely quiet. Outbound frames count too,
so a stream pushing events to a silent consumer keeps its own connection
alive.
