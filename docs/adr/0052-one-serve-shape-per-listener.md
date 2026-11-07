<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0052: One serve shape per listener kind, and `ServeConfig` on every listener

**Date:** 2026-11-05
**Status:** Accepted (the maintainer's instruction of 2026-11-04: every open
plan item goes into 0.16.0 with the best engineering option, open to a veto in
review). Built in 0.16.0.
**Author:** Tom F.

---

## Context

Audit G-09 ("too many ways to serve"), re-read at the 0.16.0 integration head
(`558be5d3`). Ten functions start a listener, in three receiver shapes and
four return shapes, and only one of them takes a `ServeConfig`:

| # | Entry point | Receiver | Returns | Shutdown | Settings |
|---|---|---|---|---|---|
| 1 | `serve::serve(addr, dispatcher)` | free | `io::Result<()>`, never `Ok` | none: never returns | `ServeConfig::default()`, fixed |
| 2 | `serve::serve_with_addr(addr, dispatcher)` | free | `io::Result<SocketAddr>` | none: a detached task with no handle | `ServeConfig::default()`, fixed |
| 3 | `Server::serve_with_shutdown(dispatcher, signal)` | `self` | `ServeReport` | graceful | `ServeConfig` |
| 4 | `GrpcDispatcher::serve(addr)` | `self` | `io::Result<()>` | none | the dispatcher's own setters |
| 5 | `GrpcDispatcher::serve_with_addr(addr)` | `self` | `io::Result<SocketAddr>` | none: detached | the dispatcher's own setters |
| 6 | `GrpcDispatcher::serve_with_listener(listener)` | `self` | `io::Result<SocketAddr>` | none: detached | the dispatcher's own setters |
| 7 | `GrpcDispatcher::serve_with_shutdown(listener, signal)` | `self` | `io::Result<ServeReport>` | graceful | the dispatcher's own setters |
| 8 | `WebSocketDispatcher::serve(addr)` | `Arc<Self>` | `io::Result<()>` | none | the dispatcher's own setters |
| 9 | `WebSocketDispatcher::serve_with_addr(addr)` | `Arc<Self>` | `io::Result<SocketAddr>` | none: detached | the dispatcher's own setters |
| 10 | `WebSocketDispatcher::serve_with_shutdown(listener, signal)` | `Arc<Self>` | `ServeReport` | graceful | the dispatcher's own setters |

Two more hand the service to a server the caller runs and are not entry
points of this crate's: `GrpcDispatcher::into_service` (a tonic service) and
`A2aRouter::into_router` (an axum `Router`). `A2aRouter` did not implement
`Dispatcher`, so an Axum user could not run it on `Server` at all: the module
example bound `0.0.0.0:3000` under plain `axum::serve`, with none of
`Server`'s header-read and idle timeouts, connection cap, `Host` check,
production refusal or task-first drain.

The gRPC and WebSocket dispatchers each repeat four of `ServeConfig`'s fields
as setters of their own (`with_max_connections`, `with_completion_grace`,
`with_task_grace`, `with_drain_timeout`), and take none of the rest: no drain
delay, and no production refusal.

`STABILITY.md` §3: a signature change is a new name plus `#[deprecated]` on
the old one in 0.16, removed no earlier than 0.17.

## Options

1. **Additive only** (DECISIONS option A). `impl Dispatcher for A2aRouter`;
   `with_serve_config(ServeConfig)` on the gRPC and WebSocket dispatchers,
   their four repeated setters deprecated.
2. **Option 1, and deprecate the entry points that have no shutdown** (option
   B): #1, #2, #4, #5, #6, #8 and #9, leaving one `serve_with_shutdown` per
   listener kind (#3, #7, #10).
3. **Document the matrix** in the book and change nothing (option C).

## Decision

Option 2.

### Per entry point

| # | Decision | Replacement |
|---|---|---|
| 1 | Deprecated in 0.16.0, removed no earlier than 0.17.0 | `Server::bind(addr).await?.serve_with_shutdown(dispatcher, shutdown_signal())`, checking `ServeReport::refused` |
| 2 | Deprecated, same | `Server::bind(addr)`, `local_addr()`, then `serve_with_shutdown` on a task the caller keeps |
| 3 | Kept: the HTTP shape | — |
| 4, 5, 6 | Deprecated, same | `GrpcDispatcher::serve_with_shutdown(listener, signal)` |
| 7 | Kept: the gRPC shape | — |
| 8, 9 | Deprecated, same | `Arc::new(dispatcher).serve_with_shutdown(listener, signal)` |
| 10 | Kept: the WebSocket shape | — |

The deprecated ones are the entry points with no way to stop: a process that
uses them cannot drain, and the task-first shutdown (ADR 0054's hand-off
included) never runs. Each still works exactly as before.

The three that stay keep their return types and receivers. The differences
are not accidents to unify through new names:

* gRPC's `io::Result<ServeReport>` carries errors the other two cannot have:
  a TLS configuration tonic rejects when the router is built, and the
  transport error a tonic server stops with. Folding them into
  `ServeReport::refused` would lose the second, which happens after serving
  started. A new name for the same shape is not worth another entry point.
* The WebSocket dispatcher's `self: Arc<Self>` is the shape its accept loop
  needs (every connection task holds the dispatcher). A `self` variant would
  be `Arc::new(self)` and nothing else, and a second name for it would be the
  kind of growth this ADR exists to stop.

### `ServeConfig` on every listener

`GrpcDispatcher::with_serve_config` and `WebSocketDispatcher::with_serve_config`
take the same struct `Server::with_config` takes. Their own
`with_max_connections`, `with_completion_grace`, `with_task_grace` and
`with_drain_timeout` are deprecated in 0.16.0 and now write the stored
`ServeConfig`, so a mixture of old setters and `with_serve_config` keeps
meaning "the later call wins", field by field for the old setters.

Each listener applies the fields that mean the same thing on it, and says in
`with_serve_config`'s documentation which it does not:

| Field | `Server` | gRPC | WebSocket |
|---|---|---|---|
| `max_connections` (0 refused) | yes | yes | yes |
| `completion_grace`, `task_grace`, `drain_timeout` | yes | yes | yes |
| `drain_delay` (readiness turns "draining" at the signal, accepting goes on for the delay) | yes | yes (new) | yes (new) |
| `production` with `tls_terminated_in_front` (refuse plaintext beyond loopback and an undeclared binding) | yes | yes (new) | yes (new) |
| `idle_timeout` | yes | no: HTTP/2 keepalive (`with_http2_keepalive`, on by default) closes a peer that stops answering, and a server stream waiting for its next event is quiet and healthy | yes: the dispatcher's idle bound, which Pings at half the budget (ADR 0042); `with_idle_timeout` remains for it |
| `header_read_timeout` | yes | no: an HTTP/2 connection has no header phase to bound; the TLS handshake is bounded (10 s) | no: `with_handshake_timeout` (10 s by default) bounds the whole upgrade, TLS included, and is tighter than the 30 s header default |
| `allowed_hosts`, `host_check` | yes | no | no: the `Origin` check (`with_allowed_origins`) is what refuses a browser, which is what DNS rebinding needs |

### `A2aRouter` as a `Dispatcher`

`impl Dispatcher for A2aRouter` builds the axum `Router` once, on the first
request (`OnceLock`), and calls it as a tower service per request. Its
response body is mapped into the dispatcher response type; an error from the
body ends the body there (the client sees a truncated response, as it would
from a connection reset), and is logged. It reports its handler
(`request_handler`), so `Server`'s task-first shutdown and load stamping
apply; its binding (`protocol_binding`, `"HTTP+JSON"`, or none when
`with_websocket` makes it serve two); and it closes the connections it
upgraded to WebSocket in `Server`'s drain (`close_upgraded`), as
`CombinedDispatcher` does.

`into_router` stays: an application that composes the router with its own
routes and layers still serves it with `axum::serve`.

## Consequences

* An Axum user runs `A2aRouter` (or its `RestRouter` alias, ADR 0053) on
  `Server` and gets every protection `Server` has.
* Seven entry points and eight setters are deprecated in 0.16.0, removed no
  earlier than 0.17.0. The workspace's examples, book, READMEs, benches and
  tests move to the kept shapes; each deprecated item keeps one test under
  `#[allow(deprecated)]` showing it still behaves as before.
* New behaviour only for a caller that asks for it: `drain_delay` and
  `production` on gRPC and WebSocket apply only through `with_serve_config`.
* Not built: unified return types and receivers (above); `idle_timeout` and
  `header_read_timeout` on gRPC; the `Host` check on gRPC and WebSocket.
