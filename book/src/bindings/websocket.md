# WebSocket binding specification

The WebSocket binding carries the JSON-RPC binding's messages over one
WebSocket connection. It is a custom protocol binding in the sense of A2A §12:
the server is `WebSocketDispatcher` (feature `websocket`), the client is
`WebSocketTransport`. This page is its specification; the
[dispatcher guide](../building-agents/dispatchers.md#websocketdispatcher)
covers setup.

```text
protocolBinding: https://a2a-rust.com/bindings/websocket/v1
url:             ws://host:port/   (wss:// behind a TLS terminator)
constant:        a2a_protocol_types::WEBSOCKET_BINDING_URI
```

## 1. Connection

A client opens the connection with an HTTP/1.1 upgrade request (RFC 6455).
The server answers `101 Switching Protocols`, or refuses the upgrade:

| Condition | Answer |
|---|---|
| `A2A-Version` absent or empty (read as 0.3, §3.6.2) | `400`, unless the server was built with `accept_missing_version_header()` |
| `A2A-Version` names a version other than 1.0 (a patch number is ignored) | `400` |
| Handshake not finished within the handshake timeout (default 10 s) | connection closed |

A refused upgrade carries the HTTP+JSON binding's error body (§11.6):
`{"error":{"code":400,"status":"FAILED_PRECONDITION","message":…,"details":[ErrorInfo]}}`.

## 2. Service parameters (§3.2.6, §12.3)

**Transmission.** Service parameters are the HTTP headers of the upgrade
request. They are captured once, at the handshake, and apply to every request
on the connection.

**Fallback.** A browser cannot set headers on a WebSocket upgrade, so the
version may instead be sent as the `A2A-Version` query parameter of the
upgrade URL (§3.6.1), for example `ws://host/?A2A-Version=1.0`. A header wins
over the query parameter. No other parameter has a query-string fallback.

**Value constraints.** Header names are lowercased. A header whose value is
not valid UTF-8 is dropped, as on the HTTP bindings.

**Reserved names.** The server adds one entry of its own, `:path`, holding the
upgrade request's path (read by `PathSegmentTenantResolver`). A colon cannot
start an HTTP/1 header name, so no client header can collide with it.

**Consequence.** A parameter cannot vary per request. A client that needs a
different `A2A-Extensions` value or credential for one call opens another
connection.

## 3. Messages

Every message is one JSON-RPC 2.0 object in one **text** frame, in both
directions. Method names, params and results are those of the JSON-RPC binding
(§9): the eleven v1.0 `PascalCase` methods. The v0.3 `method/verb` names are
answered `-32601` (MethodNotFound), as over HTTP. JSON-RPC batches are not
supported: an array is answered `-32600`.

```json
{"jsonrpc":"2.0","id":"1","method":"GetTask","params":{"id":"task-1"}}
```

```json
{"jsonrpc":"2.0","id":"1","result":{"id":"task-1","contextId":"ctx-1","status":{"state":"TASK_STATE_WORKING","timestamp":"2026-10-12T08:00:00Z"}}}
```

A connection may have up to 64 requests in progress at once. Their responses
can arrive in any order; a client correlates them by `id`.

A request with no `id` member is a notification (JSON-RPC 2.0 §4.1): the
server runs it and sends no frame for it, whether it succeeds or fails. A
streaming method (`SendStreamingMessage`, `SubscribeToTask`) sent as a
notification is not run: a stream is its only answer.

### Data types (§12.2)

The payloads are the JSON-RPC binding's ProtoJSON (§9), unchanged:

| Proto type | On the wire |
|---|---|
| Field names | lowerCamelCase (`contextId`, `messageId`) |
| Enumerations | the proto enum name as a string (`TASK_STATE_WORKING`, `ROLE_USER`) |
| `google.protobuf.Timestamp` | RFC 3339 string in UTC (`2026-10-12T08:00:00Z`); the server converts an executor's other offsets to UTC |
| `bytes` (a part's `raw`) | base64 string |
| `google.protobuf.Struct` (`metadata`, a part's `data`) | JSON object |

Binary WebSocket frames are not part of the binding. The server answers one
with `-32700` and `"id": null`.

## 4. Streaming (§12.5)

**Mechanism.** `SendStreamingMessage` and `SubscribeToTask` answer with a
sequence of frames on the same connection. Each frame is a JSON-RPC success
response carrying the request's `id`, whose `result` is one `StreamResponse`
(`task`, `message`, `statusUpdate` or `artifactUpdate`).

**Ordering.** One stream's events arrive in the order the server produced
them. Frames of different requests on one connection may interleave.

**Termination.** After the last event the server sends one more frame for the
same `id`:

```json
{"jsonrpc":"2.0","id":"7","result":{"status":"stream_complete"}}
```

This frame is a transport marker, not a `StreamResponse`. A stream that fails
ends with a JSON-RPC error frame for its `id` instead, and no marker.

**Reconnection.** There is no resumption. The HTTP binding's
`Last-Event-ID` has no counterpart here, and the server ignores one on the
upgrade request, so a client whose connection drops opens a new one and calls
`SubscribeToTask`. That stream's first event is the task's current snapshot,
from which the client continues; events in between are not replayed.

## 5. Errors (§5.4, §12.4)

Errors are JSON-RPC error objects carrying the JSON-RPC binding's codes and
`data`, an array of typed details (`google.rpc.ErrorInfo` for A2A errors,
`RetryInfo` when a retry delay applies, `BadRequest` for params that did not
parse):

```json
{"jsonrpc":"2.0","id":"1","error":{"code":-32001,"message":"Task not found: task-9","data":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"TASK_NOT_FOUND","domain":"a2a-protocol.org"}]}}
```

| A2A error | Code |
|---|---|
| Frame is not JSON | `-32700` |
| Frame is JSON but not a request | `-32600` |
| Method not found, or a v0.3 name | `-32601` |
| Invalid params | `-32602` (with `BadRequest` when the params did not parse) |
| Internal error, or more than 64 requests in progress on the connection | `-32603` |
| TaskNotFound | `-32001` |
| TaskNotCancelable | `-32002` |
| PushNotificationNotSupported | `-32003` |
| UnsupportedOperation | `-32004` |
| ContentTypeNotSupported | `-32005` |
| InvalidAgentResponse | `-32006` |
| ExtendedAgentCardNotConfigured | `-32007` |
| ExtensionSupportRequired | `-32008` |
| VersionNotSupported | `-32009` |

A refused credential is `-32600` (below). A mid-stream error keeps its own
code and details, including the `streamLagged` marker of a subscriber that
fell behind.

## 6. Authentication (§12.6)

Credentials travel in the upgrade request's headers (`Authorization`,
`X-API-Key`, or whatever the agent card's security schemes name) and are
checked by the server's interceptors on every request. Because the connection
is already upgraded, a refused credential cannot carry an HTTP status or a
`WWW-Authenticate` challenge: it is answered `-32600` in the JSON-RPC body.
Deployments whose clients need a `401` challenge authenticate the upgrade at a
gateway or reverse proxy.

Browsers cannot set `Authorization` on a WebSocket upgrade. This binding
offers no in-band alternative; a browser client authenticates at a gateway
that accepts its cookie or ticket and forwards a header.

## 7. Security considerations (§13.4)

- **Transport.** The dispatcher listens on plain `ws://`. Production
  deployments put a TLS terminator in front and publish `wss://` in the card.
  The SDK's client connects to `wss://` with the roots its HTTPS transports
  use (feature `tls-rustls`, on by default), or a configuration given to
  `WebSocketTransportConfig::with_tls_config`.
- **Origin.** The server does not check the upgrade's `Origin` header. Where
  credentials are ambient (a cookie a gateway turns into a header), a page on
  another site could open an authenticated connection (cross-site WebSocket
  hijacking); such a gateway must check `Origin`.
- **One identity per connection.** Credentials are read from the upgrade, so
  every request on a connection acts as one caller. The interceptors check
  them on every request, so a token that expires is refused from then on,
  but it cannot be replaced without opening a new connection.
- **Limits.** Incoming messages are capped at 4 MiB (a larger one ends the
  connection), 64 requests per connection, a handshake timeout, and optional
  caps on connections and idle time; see the
  [dispatcher guide](../building-agents/dispatchers.md#built-in-limits).

## 8. Differences from the JSON-RPC HTTP binding

| | JSON-RPC over HTTP | WebSocket |
|---|---|---|
| Service parameters | per request | per connection (upgrade headers) |
| Streams | SSE, one HTTP response each | frames on the shared connection, ended by `stream_complete` |
| Resumption | `Last-Event-ID` | none; resubscribe |
| Refused credential | `401` + `WWW-Authenticate` / `403` | `-32600` in the body |
| Batches | supported | not supported (`-32600`) |
| Binary payload frames | not applicable | refused with `-32700` |

The TCK's `--equivalence` mode runs the same scenarios over this binding and
the three specification bindings and compares the outcomes.
