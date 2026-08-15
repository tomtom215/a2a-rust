<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-slimrpc

A2A over the [AGNTCY SLIM](https://github.com/agntcy/slim) fabric, implementing
the [SLIMRPC custom protocol binding][spec] for the `a2a-protocol` SDK.

[spec]: https://github.com/a2aproject/experimental-cpb-slimrpc

```
protocolBinding: https://a2a-protocol.org/bindings/experimental-slimrpc/v1
address:         slim://[node-host[:port]/]domain/namespace/service
service:         lf.a2a.v1.A2AService
```

## Status

**Upstream-experimental.** The binding's own README reads *"community
contributed … not part of the core A2A specification"*, and the ratified A2A
v1.0 specification contains no occurrence of "slim" or "agntcy". Nothing here is
required for A2A conformance — this exists because the SLIM fabric is where some
deployments already live.

The binding itself is complete: all eleven methods in the spec's inventory, both
streaming methods included, verified end-to-end over a real SLIM datapath.

## Why it is not in the workspace

`agntcy-slim-rpc` brings **379 transitive dependencies**, including `aws-lc-sys`
— a native C crypto build. For comparison, in this repository:

| Crate | Unique transitive deps |
|---|---|
| `a2a-protocol-types` | 12 |
| `a2a-protocol-server` (default) | 53 |
| `a2a-protocol-sdk` (default) | 67 |
| `a2a-protocol-server` (all features) | 191 |
| **`agntcy-slim-rpc` alone** | **379** |

So this crate sits outside the workspace with its own `Cargo.lock`. None of that
graph reaches the lockfile, `deny.toml` allow-list, or audit surface of the four
published crates, and none of them depends on this one. The arrow points one
way.

## What it plugs into

| Extension point | Used by |
|---|---|
| `a2a_protocol_client::transport::Transport` | `SlimRpcTransport` |
| `A2aClientBuilder::with_custom_transport` | injecting it, no fork needed |
| `a2a_protocol_server::RequestHandler` | `SlimRpcServer` drives the same handler the HTTP bindings drive |
| `AgentInterface::protocol_binding` | advertising the binding on the agent card |

Building this binding required **one** addition to those crates:
`EventStream::from_event_channel`. Every other `EventStream` constructor is
`pub(crate)`, so before it existed a custom transport could implement the unary
half of `Transport` and not the streaming half — the extension point was
incomplete for streaming, and no amount of reading the signatures showed it. See
the CHANGELOG entry for `a2a-protocol-client`.

## Method inventory

All eleven, per `spec/v1/slimrpc.md`. Nine unary, two unary-request /
streaming-response.

| SLIMRPC method | Kind | `RequestHandler` |
|---|---|---|
| `SendMessage` | unary | `on_send_message(.., streaming: false, ..)` |
| `SendStreamingMessage` | unary→stream | `on_send_message(.., streaming: true, ..)` |
| `GetTask` | unary | `on_get_task` |
| `ListTasks` | unary | `on_list_tasks` |
| `CancelTask` | unary | `on_cancel_task` |
| `SubscribeToTask` | unary→stream | `on_resubscribe` |
| `CreateTaskPushNotificationConfig` | unary | `on_set_push_config` |
| `GetTaskPushNotificationConfig` | unary | `on_get_push_config` |
| `ListTaskPushNotificationConfigs` | unary | `on_list_push_configs` |
| `DeleteTaskPushNotificationConfig` | unary | `on_delete_push_config` |
| `GetExtendedAgentCard` | unary | `on_get_extended_agent_card` |

The handler is the same object every other binding drives, so an agent behaves
identically however it is reached. Nothing about task state, streaming, push
notifications, tenancy or authorisation is reimplemented here.

## Wire format

Protobuf — SLIMRPC uses the same service definitions as gRPC, so the payloads
are the canonical `lf.a2a.v1` messages `a2a-protocol-types` already generates,
byte-compatible with the official Go, Python and Java SDKs. `Pb<T>` is the
newtype that satisfies SLIMRPC's `Encoder`/`Decoder` for any `prost::Message`;
it adds no framing of its own.

## Errors

Status codes follow A2A §5.4, identical to the gRPC binding's mapping —
`RpcCode` is the gRPC code set.

Where SLIMRPC differs is error *identity*. gRPC attaches `google.rpc.ErrorInfo`
to `status.details`; SLIMRPC has no details protobuf, so the spec puts the A2A
error type name in the message:

```
TaskNotFoundError: task-123 not found
```

The binding writes that prefix on the way out and reads it on the way in, so an
A2A error survives as itself rather than collapsing into one of thirteen status
codes. This matters: `FailedPrecondition` alone cannot tell
`TaskNotCancelableError` from `ExtensionSupportRequiredError`.

## Client

```rust,no_run
use a2a_protocol_slimrpc::{SlimName, SlimRpcTransport};
use a2a_protocol_client::ClientBuilder;

let agent = SlimName::parse("slim://org/demo/echo_agent")?;
let transport = SlimRpcTransport::builder(agent)
    .with_shared_secret("caller", std::env::var("SLIM_SECRET")?)
    .connect()?;

let client = ClientBuilder::new("slim://org/demo/echo_agent")
    .with_custom_transport(transport)
    .build()?;
```

Retries, interceptors, auth and the typed method surface all work exactly as
they do over JSON-RPC — only the wire underneath changes.

## Server

```rust,no_run
use a2a_protocol_slimrpc::{SlimName, SlimRpcServer};

let name = SlimName::parse("slim://org/demo/echo_agent")?;
let server = SlimRpcServer::builder(handler, name)
    .with_shared_secret("echo_agent", std::env::var("SLIM_SECRET")?)
    .build()?;

// Advertise it so callers can discover the binding.
card.supported_interfaces.push(server.agent_interface());

server.serve().await?;
```

`SlimRpcServer::from_app` and `SlimRpcTransport::from_app` take a SLIM app the
caller already owns — the path for an app attached to a remote fabric node, or
shared between services.

## Tests

```
cargo test
```

27 tests: 20 unit, 7 end-to-end. The end-to-end suite is not mocked — one
in-process SLIM `Service` hosts an agent app and a caller app, and messages
travel the real SLIM datapath between them. A binding that type-checks proves
nothing about whether the two ends agree on the wire, so those seven cover
method registration, a unary round trip, a task fetched back by id, error
identity surviving the fabric, a streaming send running to its terminal event,
agent-card advertisement, and an unknown method being reported rather than
hanging.

## Limitations

- **Multicast is not implemented.** `spec/v1/slimrpc-multicast.md` is a separate
  document covering group channels, client discovery and response collection.
  SLIM supports it (`Channel::multicast_*`); this binding does not use it yet.
- **Remote fabric nodes are untested here.** The `slim://host/...` address form
  parses and `from_app` accepts an app attached to a node, but every test in
  this crate runs against an in-process service. Nothing verifies behaviour
  across a real SLIM node.
- **Shared-secret identity only in the builders.** `with_shared_secret` is what
  `build()`/`connect()` offer. Anything else — SPIFFE, JWT, mTLS — means
  constructing the `App` yourself and using `from_app`.
