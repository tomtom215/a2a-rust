# Transport Layers

A2A defines three transport bindings, **JSON-RPC 2.0**, **REST** (`HTTP+JSON`) and **gRPC** (`grpc` feature flag). a2a-rust adds a fourth, **WebSocket** (`websocket` feature flag), as a custom binding under spec §12; a fifth, [SLIMRPC](../bindings/slimrpc.md), is a separate crate. All four are first-class citizens in a2a-rust — the server can serve multiple transports simultaneously, and `ClientBuilder::from_card` selects an interface from the agent card (`build()` constructs JSON-RPC or REST; gRPC needs `build_grpc`, and WebSocket a transport passed to `with_custom_transport`).

Every binding carries the same eleven operations and the same data model; they
differ in how a request is framed, how a stream is delivered, and where the
per-request metadata (credentials, tenant, `A2A-Version`, `traceparent`)
travels. This page compares them; each binding's wire details have a page of
their own. Setting a binding up is covered elsewhere: the server side in
[Dispatchers](../building-agents/dispatchers.md), the client side in
[Building a Client](../client/builder.md).

| You want | Read |
|---|---|
| The JSON-RPC envelope, method names, batches and notifications | [JSON-RPC 2.0](./transport-layers/jsonrpc.md) |
| The REST route table, tenant paths and content negotiation | [HTTP+JSON (REST)](./transport-layers/rest.md) |
| WebSocket framing, connection-level headers and the client transport's limits | [WebSocket](./transport-layers/websocket.md) |
| gRPC targets, client TLS and serving TLS | [gRPC](./transport-layers/grpc.md) |
| The SLIM fabric | [SLIMRPC](../bindings/slimrpc.md) |

## Bindings at a glance

| Binding | Streams | Per-request metadata | Resume a dropped stream |
|---|---|---|---|
| JSON-RPC 2.0 | SSE on the POST response | HTTP headers, per request | `Last-Event-ID` |
| HTTP+JSON (REST) | SSE on `POST` or `GET` | HTTP headers, per request | `Last-Event-ID` |
| gRPC | gRPC server streaming over HTTP/2 | gRPC metadata, per call | No event ids on the stream; resubscribe |
| WebSocket | Text frames, then `stream_complete` | Upgrade headers, once per connection | None; resubscribe |
| SLIMRPC | SLIMRPC response stream | Session metadata, per call | No event ids on the stream; resubscribe |

The metadata column is the one that changes behaviour. JSON-RPC, REST, gRPC
and SLIMRPC carry headers on every call, so a token can rotate between calls
and each call's `traceparent` makes its server span a child of the caller's.
A WebSocket frame has no header channel. The upgrade request's HTTP headers
reach the handler for every request on the connection,
so authentication and tenant resolution work exactly as over HTTP,
but credentials are fixed at connect time, and a per-call header an interceptor adds is not sent. W3C trace
context cannot be fixed at connect time, because every request needs its own
parent id, so it is not propagated at all: each call's server span links to
the trace of the upgrade request instead. If a call must be traced end to end,
send it over another binding.

## Choosing a Transport

| Factor | JSON-RPC | REST | WebSocket | gRPC |
|--------|----------|------|-----------|------|
| **Batch operations** | Supported | Not supported | Not supported | Not supported |
| **Caching** | Limited (POST-only) | HTTP cache-friendly (GET) | Not applicable | Not applicable |
| **Tooling** | Needs JSON-RPC client | Standard HTTP tools work | WebSocket client needed | gRPC client needed |
| **URL structure** | Single endpoint | Resource-oriented | Single connection | Single connection |
| **Streaming** | SSE via POST | SSE via POST/GET | Native text frames | Native server streaming |
| **Connection reuse** | HTTP keep-alive | HTTP keep-alive | Persistent connection | HTTP/2 multiplexing |

JSON-RPC and REST use SSE for streaming. WebSocket uses native text frames. gRPC uses native server streaming over HTTP/2. The choice is mostly about ecosystem fit — JSON-RPC for agent-to-agent communication, REST for standard HTTP tooling, WebSocket for persistent low-latency connections, gRPC for service mesh and cross-language interop.

## Running several transports

The server can serve several bindings at once: each dispatcher on its own
port, or JSON-RPC and REST on one port with `CombinedDispatcher`. All
dispatchers share the same `RequestHandler`, which means they share the same
task store, push config store, and executor. The setup is in
[Serving and combining](../building-agents/dispatchers/serving.md).

## Next Steps

- **[Agent Cards & Discovery](./agent-cards.md)** — How transport URLs are advertised
- **[Streaming with SSE](./streaming.md)** — How real-time events work across transports
- **[Dispatchers](../building-agents/dispatchers.md)** — Server-side dispatcher configuration
