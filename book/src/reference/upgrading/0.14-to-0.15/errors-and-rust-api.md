<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Errors and Rust API

Part of [Upgrading from 0.14 to 0.15](../0.14-to-0.15.md). Each section names what breaks and what to change; the [upgrade guide](../../upgrading.md) has the policy and how to check your own code.

## Request-field errors are `ServerError::InvalidField`

The handler's own checks of a request field (ids, a message's role, parts and
metadata, a push config's fields, `ListTasks` filters, REST query
parameters) return `ServerError::InvalidField { field, message }`, which
answers as `InvalidParams` does plus a `google.rpc.BadRequest` detail naming
the field. They returned `ServerError::InvalidParams(message)`. The code, the
HTTP status and the `invalid_params` metric label are unchanged, and the
response gains the `BadRequest` detail on JSON-RPC, HTTP+JSON, WebSocket and
gRPC. The `SendMessage` id checks now name the field by its wire path, in the
violation's `field` and in the message: `message.messageId`,
`message.contextId` and `message.taskId` (they named `message.id`,
`context_id` and `task_id`), and `message.referenceTaskIds`, which was not
checked before. A client or test that matched the old strings matches the new
ones. A `match` or `matches!` on the variant is affected, and still compiles.
Match either variant, or the code:

```rust
use a2a_protocol_server::ServerError;
use a2a_protocol_types::error::ErrorCode;

fn is_invalid_params(err: &ServerError) -> bool {
    matches!(err, ServerError::InvalidParams(_) | ServerError::InvalidField { .. })
        || err.to_a2a_error().code == ErrorCode::InvalidParams
}

let err = ServerError::InvalidField {
    field: "message.parts".into(),
    message: "message must contain at least one part".into(),
};
assert!(is_invalid_params(&err));
```

## Error codes some clients may match on

- A rate-limit refusal from `RateLimitInterceptor` is HTTP `429` with
  `Retry-After` and gRPC `RESOURCE_EXHAUSTED`, not `500` / `INTERNAL`. The
  JSON-RPC code is still `-32603`.
- On WebSocket, errors carry their A2A code and typed `data`: a lagged
  stream is `-32603` with the `streamLagged` marker, and an over-capacity
  request `-32603`, where both were `-32000`.
- SLIMRPC status codes follow the SLIMRPC specification's table:
  `JSONParseError` is `INTERNAL`, `ContentTypeNotSupportedError`
  `UNIMPLEMENTED`, `VersionNotSupportedError` `FAILED_PRECONDITION`.
- A credential refused over WebSocket is still `-32600`, now with an
  `ErrorInfo` from `a2a-rust.com` (`UNAUTHENTICATED` with the challenge as
  `www-authenticate` metadata, or `PERMISSION_DENIED`; the
  [binding specification](../../../bindings/websocket.md#6-authentication-126)
  §6). The SDK's client reports it as `ClientError::UnexpectedStatus` with
  status `401` or `403`, as over HTTP and gRPC; code that matched
  `ClientError::Protocol` with `ErrorCode::InvalidRequest` for it matches
  `UnexpectedStatus` instead.

## `ConvertError` and `SseParseError` are `#[non_exhaustive]`

Build a `ConvertError` with `ConvertError::new(field, reason)`, and give a
`match` on `SseParseError` a wildcard arm.

## `serve::Server` is no longer `UnwindSafe`

With `tls-rustls` (a default feature of the SDK, not of `a2a-protocol-server`
alone), `Server` holds the TLS acceptor that `Server::with_tls` sets, built on
rustls' server configuration, which implements neither `UnwindSafe` nor
`RefUnwindSafe`; neither does `Server` now. `cargo-semver-checks` reports it
as `auto_trait_impl_removed`. Only code that moved a `Server` across
`std::panic::catch_unwind` notices, and the migration is the one the
`InMemoryQueueWriter` item in
[0.11 → 0.12](../0.11-to-0.12.md#inmemoryqueuewriter-server-is-no-longer-unwindsafe--refunwindsafe)
gives:

```text
// 0.14
let outcome = std::panic::catch_unwind(move || drop(server));
```

```rust
use std::panic::{AssertUnwindSafe, catch_unwind};
use a2a_protocol_server::serve::Server;

fn drop_guarded(server: Server) -> bool {
    catch_unwind(AssertUnwindSafe(move || drop(server))).is_ok()
}
```

## The conformance kit's cancel check is `cancel_succeeds`

The `conformance` feature's `cancel_emits_terminal` check is renamed
`cancel_succeeds`; a CI step that greps for the old name finds nothing. It
failed a `cancel` hook that wrote no terminal state, because such a hook left
the task's streams without one. The handler now writes `Canceled` after such a
hook (spec §3.1.2), so the check passes it, and fails a hook that errors,
panics or overruns.
