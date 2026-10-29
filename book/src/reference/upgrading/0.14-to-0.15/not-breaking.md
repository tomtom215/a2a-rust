<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Also in 0.15, not breaking

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). These change behaviour without breaking an API; they are the ones a gateway, a test or a client is most likely to meet.

- **Deprecated.** Besides
  [`sign_agent_card`](./agent-cards.md#sign_agent_card-is-deprecated-and-refuses-none)
  and
  [`ClientConfig::preferred_bindings`](./client.md#clientbuilderfrom_card-follows-the-cards-interface-order):
  `HotReloadAgentCardHandler::update` is deprecated for `try_update`, which
  checks the card as `reload_from_json` does and refuses an invalid one with
  `InvalidParams`, leaving the current card in place. `OAuthFlows::Implicit`
  and `OAuthFlows::Password` are deprecated because the A2A 1.0 proto marks
  both; they stay, parse and serialize as before (spec §1.4 keeps deprecated
  definitions until the next major version), and code that names them gets a
  warning.
- **Several replicas sharing a store need session affinity.** Without every
  turn of a task routed to one replica, two replicas can run the same task at
  once, and a continuation saved on one can overwrite what the other's turn
  wrote meanwhile. One replica, or several with affinity, is the supported
  topology for 0.15; see
  [Running More Than One Replica](../../../deployment/horizontal-scaling.md).
- **`serve` and `serve_with_addr` cut off a slow peer.** They run on `Server`
  under `ServeConfig::default()`, with a 30-second header-read timeout and a
  75-second idle timeout (traffic in either direction counts, so an SSE stream
  with its 30-second keep-alive stays open). Their signatures are unchanged;
  for a ceiling, TLS or a graceful drain, use `Server` directly.
- **A header sent as several lines is one field.** Every binding joins the
  lines in order with `", "` (`"; "` for `cookie`), where each kept the last.
  `A2A-Extensions` on two lines activates both; a field that takes one value,
  such as `Authorization`, reaches the interceptors as both values joined, so a
  bearer token sent on two lines is refused.
- **A `Content-Type` is read as a media type**: the type and subtype before any
  `;` parameter, compared without regard to case. `Application/JSON` is
  accepted; `application/json-rpc` and `application/json-patch+json`, which a
  prefix match served, are refused with `ContentTypeNotSupportedError`.
- **On HTTP+JSON the path's id is the request's.** A body's `id` or `taskId` no
  longer replaces the path's, an empty path segment reaches its handler as an
  empty id and is `InvalidParams`, and the Axum router reads the body of
  `:cancel` and `:subscribe` (a malformed one is `400`).
- **JSON-RPC follows three more rules of JSON-RPC 2.0.** An `id` that is not a
  string, a number or `null` is `-32600`; omitted `params` read as `{}`; and
  `SendStreamingMessage` or `SubscribeToTask` inside a batch is
  `UnsupportedOperationError` (`-32004`), not `-32602`.
- **The WebSocket binding no longer routes `message/stream`**, the one v0.3
  method name it still served; it is `-32601`, as on every other binding.
- **SLIMRPC refuses a call that declares no `A2A-Version`** with
  `VersionNotSupportedError`, as the HTTP and gRPC bindings do by default.
  `SlimRpcServer::with_require_version(false)` restores the old behaviour.
- **ListTasks follows the proto.** `statusTimestampAfter` includes its bound
  ("greater than or equal to"), where every store excluded it, and `totalSize`
  is the number of tasks the filters match before pagination.
- **Listing or deleting the push configs of an unknown task is
  `TaskNotFound`** (`404` on HTTP+JSON, `-32001` on JSON-RPC, `NOT_FOUND` on
  gRPC); it answered an empty list and success. Deleting a config that is
  already gone from a task that exists still succeeds.

These are the ones a gateway, a test or a client is most likely to meet. The
rest are under `### Behaviour Changes` in the `## [0.15.0]` section of
CHANGELOG.md.
