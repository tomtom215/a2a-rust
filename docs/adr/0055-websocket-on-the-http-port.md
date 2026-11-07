<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0055: The WebSocket binding is served on the HTTP port, by an upgrade route

**Date:** 2026-11-05
**Status:** Accepted (the maintainer's instruction of 2026-11-04: every open
plan item goes into 0.16.0 with the best engineering option, open to a veto in
review). Built in 0.16.0.
**Author:** Tom F.

---

## Context

Audit B-25. `WebSocketDispatcher` serves on a listener of its own, so an agent
offering JSON-RPC, HTTP+JSON and WebSocket needs two ports: two load-balancer
targets, two TLS configurations, two health checks, and a card with two
origins. Every proxy and load balancer an agent is deployed behind (nginx,
Envoy, the cloud L7 balancers) forwards an HTTP/1.1 `Upgrade: websocket` on the
port it already serves; a second port is the part that does not come free.

The WebSocket listener already has what a connection needs once it is
upgraded: the origin check against cross-site WebSocket hijacking, the
`A2A-Version` check, the idle bound with its Ping (ADR 0042), the write
timeout, per-connection request limits, the 4 MiB message cap, and a graceful
shutdown that sends a Close frame. What it does not have is a way in through
the HTTP server.

## Options

1. **An upgrade route on the HTTP dispatchers.** `CombinedDispatcher` and the
   axum `A2aRouter` take a `WebSocketDispatcher` and answer an HTTP/1.1
   `GET` with `Upgrade: websocket` at the JSON-RPC path (the card's base URL)
   with `101 Switching Protocols`, then hand the upgraded stream to the
   WebSocket listener's own connection loop. One port; the card names
   `ws://` or `wss://` at the same host and path, following the listener's
   TLS.
2. **Keep the separate port and document it** (a proxy rule that sends
   upgrades to the second port). Every deployment repeats the rule, and the
   card still names two origins.
3. **RFC 8441 (WebSocket over HTTP/2, extended `CONNECT`).** hyper's server
   supports it behind a setting, but the client libraries this SDK and its
   peers use (tungstenite, the browsers' fetch-less `WebSocket`) speak
   HTTP/1.1 upgrades; it would serve almost no client.

## Decision

Option 1, with these rules, each tested:

* **The interceptor chain admits the upgrade.** The upgrade request's headers
  (with the peer address and the client certificate, as every binding records
  them) go through the handler's interceptor chain before the `101`, as a
  call to the method `WebSocketUpgrade`; a refusal is answered with the status
  the HTTP+JSON binding gives it (`401` with its challenge, `403`), and nothing
  is upgraded. The headers are then the connection's for every request on it,
  as on the separate listener.
* **Cross-site WebSocket hijacking.** An upgrade that carries an `Origin` is
  refused (`403`) unless the origin is the request's own (its scheme-less
  authority equals the `Host` header) or is in the `WebSocketDispatcher`'s
  `with_allowed_origins` list (`allow_any_origin` accepts every one). An
  upgrade with no `Origin`, from a client that is not a browser, is accepted,
  unless `require_origin` is set. The same-origin rule is new and applies only
  here: on its own port the WebSocket listener has no page of its own.
* **The connection's limits are the listener's.** The connection keeps its
  HTTP server's `max_connections` permit until it closes; the
  `WebSocketDispatcher`'s `with_max_connections` caps upgraded connections
  too (`503` past it, before the `101`). The 4 MiB message and frame cap,
  the 64 requests per connection, the write timeout and the idle bound with
  its Ping are the same code as on the separate listener. While a connection
  is upgraded the HTTP server's byte-level idle timeout is suspended for it,
  as it is while a request is in flight: the WebSocket's own bound, which
  pings before it closes, governs instead (ADR 0042).
* **Shutdown.** `Server::serve_with_shutdown` closes upgraded connections in
  its drain, after the tasks in flight have ended, as it closes HTTP ones:
  each stops reading, lets the requests it read finish, and sends a Close
  frame, within the drain timeout. A new defaulted method on `Dispatcher`,
  `close_upgraded`, is the hook.
* **Where.** The JSON-RPC path, `/` by default, under `with_base_path` when
  one is set. `A2A-Version` is checked as on the separate listener. Only
  HTTP/1.1; an upgrade request over HTTP/2 is not one.

## Consequences

* New: `CombinedDispatcher::with_websocket(WebSocketDispatcher)` and
  `A2aRouter::with_websocket(WebSocketDispatcher)` (feature `websocket`);
  `Dispatcher::close_upgraded` (defaulted, so other dispatchers are
  unchanged); the HTTP server serves connections with upgrades enabled.
* The separate listener stays, for deployments that want the port apart.
* An application serving the axum router itself owns its shutdown:
  `A2aRouter::websocket_upgrades()` returns the handle whose `close` does what
  `Server` does in its drain.
* Not built: RFC 8441; a WebSocket route on `JsonRpcDispatcher` or
  `RestDispatcher` alone (the combined dispatcher is the one-port shape);
  rewriting the card, which stays the application's (the book shows the
  interface to declare).
