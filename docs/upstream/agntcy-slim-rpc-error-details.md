<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Issue draft for `agntcy/slim` — `RpcError::details` never reaches the caller

**Status: DRAFT, not sent.** Written and verified 2026-10-23 against
`agntcy-slim-rpc` 3.4.6, the newest release on crates.io that day (`cargo
search`) and the version `bindings/a2a-protocol-slimrpc/Cargo.lock` resolves,
read from the published crate source. The reproduction below was run against
3.4.6 that day and fails as described. Duplicate check: a GitHub issue search
of `agntcy/slim` for "slimrpc RpcError details dropped send_error" on
2026-10-23 found none (closest: #2255, errors sent to the whole group,
closed; different). Search the tracker again before filing. Spec ledger
§12.4 (PARTIAL until this is fixed; maintainer decision 2026-10-20).

---

## Title

slim-rpc: `RpcError::details` is dropped on the wire — a handler's error details never reach the caller

## Body

`agntcy-slim-rpc` 3.4.6 gives `RpcError` structured details:

```rust
// src/error.rs:149-155
pub enum RpcError {
    #[error("{message}")]
    Rpc {
        code: RpcCode,
        message: String,
        details: Option<Vec<u8>>,
    },
```

with a constructor, `RpcError::with_details` (`src/error.rs:239`), and a
getter, `RpcError::details` (`src/error.rs:358`). But the server sends only
the code and the message. `send_error_to`, which the server uses for every
handler error (`src/server.rs:382`) and for an unknown method
(`src/server.rs:486`), publishes `error.message()` as the payload and the
status metadata, and nothing else:

```rust
// src/rpc_session.rs:152-162
pub async fn send_error_to(
    session: &SessionTx,
    target: &Name,
    error: RpcError,
    rpc_id: &str,
) -> Result<(), RpcError> {
    let message = error.message().to_string();
    let metadata = create_status_metadata(error.code(), rpc_id);
    let handle = session
        .publish(
            target,
            message.into_bytes(),
```

`create_status_metadata` (`src/rpc_session.rs:209-217`) holds only the status
code (`slimrpc-code`) and the `rpc-id`. On the receiving side both paths
rebuild the error from code and payload, so `details` is always `None`:

```rust
// src/channel.rs:715 (responses for unary and streaming calls, P2P and group)
let err = RpcError::new(code, msg_text);
// src/stream_types.rs:119
return Poll::Ready(Some(Err(RpcError::new(code, message))));
```

So a handler cannot send structured error information at all: the
`details` field, its constructor and its getter exist, and a caller always
sees `None`.

### Why it matters

The SLIMRPC A2A binding specification (`a2aproject/experimental-cpb-slimrpc`,
`spec/v1/slimrpc.md` §6.2) gives every error "optional structured details",
and A2A v1.0.1 §12.4 requires a custom binding to ensure "error details are
accessible to clients". On gRPC, A2A errors carry a `google.rpc.ErrorInfo`
(its `reason`, e.g. `TASK_NOT_FOUND`, and metadata) and, for invalid
parameters, a `google.rpc.BadRequest` naming the field. Over SLIMRPC none of
that can travel today, so an A2A implementation on `agntcy-slim-rpc` (ours,
`a2a-protocol-slimrpc`) can carry only a code and a type-name prefix in the
message.

### Reproduction

Against the public API of 3.4.6 alone (plus `agntcy-slim-service`,
`agntcy-slim-auth`, `agntcy-slim-config` and `agntcy-slim-datapath` for the
app; `tokio` with `macros` and `rt-multi-thread`). One in-process service, a
server whose only handler fails with details, and a channel that calls it:

```rust
use std::sync::Arc;
use std::time::Duration;

use slim_auth::auth_provider::{AuthProvider, AuthVerifier};
use slim_auth::shared_secret::SharedSecret;
use slim_config::component::id::{ID, Kind};
use slim_datapath::api::ProtoName;
use slim_rpc::{Channel, Context, RpcCode, RpcError, Server};
use slim_service::service::Service;

const SECRET: &str = "details-repro-shared-secret-0123456789abcdef";

#[tokio::test]
async fn rpc_error_details_do_not_reach_the_caller() {
    let service = Service::new(ID::new_with_name(Kind::new("slim").unwrap(), "repro").unwrap());
    let app = |name: &ProtoName, id: &str| {
        let secret = SharedSecret::new(id, SECRET).unwrap();
        service
            .create_app(
                name,
                AuthProvider::shared_secret(secret.clone()),
                AuthVerifier::shared_secret(secret),
            )
            .unwrap()
    };
    let server_name = ProtoName::from_strings(["org", "repro", "server"]);
    let client_name = ProtoName::from_strings(["org", "repro", "client"]);

    let (server_app, notifications) = app(&server_name, "server");
    let server = Server::new(Arc::new(server_app), server_name.clone(), notifications);
    server.register_unary_unary("Repro", "Fail", |_req: Vec<u8>, _ctx: Context| async move {
        Err::<Vec<u8>, _>(RpcError::with_details(
            RpcCode::NotFound,
            "task-123 not found",
            vec![0x0a, 0x03, b'a', b'b', b'c'],
        ))
    });
    let server = Arc::new(server);
    let serving = Arc::clone(&server);
    tokio::spawn(async move { serving.serve().await });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let (client_app, _) = app(&client_name, "client");
    let channel =
        Channel::new_with_members(Arc::new(client_app), vec![server_name], false, None).unwrap();
    let err = channel
        .unary::<Vec<u8>, Vec<u8>>("Repro", "Fail", vec![], Some(Duration::from_secs(10)), None)
        .await
        .unwrap_err();

    assert_eq!(err.code(), RpcCode::NotFound); // passes
    assert_eq!(err.message(), "task-123 not found"); // passes
    assert_eq!(err.details(), Some(&[0x0a, 0x03, b'a', b'b', b'c'][..])); // fails
}
```

Observed on 3.4.6:

```text
assertion `left == right` failed: Rpc { code: NotFound, message: "task-123 not found", details: None }
  left: None
 right: Some([10, 3, 97, 98, 99])
```

Expected: the details the handler attached arrive with the code and the
message.

### A possible fix

Carry the details beside the status, for example as a base64 metadata entry
(`slimrpc-details`) written by `create_status_metadata` when
`error.details()` is `Some`, and read back where `src/channel.rs:715` and
`src/stream_types.rs:119` rebuild the error (`RpcError::with_details` when
the entry is present). Metadata keeps the payload the plain message it is
today, so a receiver that does not know the key is unaffected. gRPC's own
answer, a serialized `google.rpc.Status` in `grpc-status-details-bin`, would
fit the same slot. The language bindings that read errors from the Rust core
would need the same change to surface them.
