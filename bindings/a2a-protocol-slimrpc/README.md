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
streaming methods included, plus multicast — verified end-to-end over a real
SLIM datapath and across a real SLIM node on a socket.

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
caller already owns. Their `*_with_connection` variants additionally take the
connection id `Service::connect` returned, which is what makes an agent
reachable **through a SLIM node** rather than only from its own process — see
the remote-node suite below.

## Multicast

One message, several agents, one outcome each — `spec/v1/slimrpc-multicast.md`.
Only `SendMessage` and `SendStreamingMessage` may be broadcast; task management
stays point-to-point, because a task id is meaningful to exactly one agent.

```rust,no_run
use a2a_protocol_slimrpc::{SlimName, SlimRpcMulticast};

let group = SlimRpcMulticast::from_app(app, vec![
    SlimName::parse("slim://org/demo/triage")?,
    SlimName::parse("slim://org/demo/classify")?,
])?
.with_timeout(Duration::from_secs(30));

let outcome = group.send_message(params, None).await?;
for (agent, response) in outcome.succeeded() { /* … */ }
for (agent, why) in outcome.failed() { /* … */ }
```

`MulticastOutcome` carries **exactly one outcome per invited agent**, always.
That is the spec's requirement — *"Clients must wait for outcomes from every
invited agent"* — and dropping a silent agent from the result would make a
partial broadcast look like a complete one.

Two failure kinds are kept distinct, because they call for different responses:

| Situation | Reported as | Why it matters |
|---|---|---|
| Agent answered with an error, or stayed silent past the timeout | a per-agent `failed()` outcome | isolated; the other agents' answers stand |
| A member could not be invited at all | `Err` from the whole call | the group is misconfigured; waiting will not fix it |

`stream_message` gives each agent its own `EventStream`, demultiplexed from
SLIM's interleaved source-tagged frames, so one agent's stream ending does not
affect another's.

## Tests

```
cargo test
```

45 tests: 25 unit, 18 end-to-end across three topologies, 2 doc. None of the
end-to-end ones are mocked.

| Suite | Topology | Covers |
|---|---|---|
| `e2e.rs` | one in-process `Service` | method registration, unary round trip, task fetched by id, error identity, streaming to a terminal event, card advertisement, unknown method |
| `multicast.rs` | group channel, several agents | attribution per agent, a silent agent as a failed outcome, failure isolation, per-agent streams, empty group, uninvitable member |
| `remote_node.rs` | **three separate services over TCP** | send, get, streaming and error identity routed through a real SLIM node |

`remote_node.rs` is the one that earns the "works in a deployment" claim. The
agent and the client share no `Service` and no memory; every message crosses a
loopback socket twice and is routed by a node in between. It found a real bug —
a client never announces its own name to the node, so nothing could route an
agent's reply back, and every call failed its session handshake. In-process that
is invisible, which is exactly why the suite exists.

## Limitations

- **Shared-secret identity only in the builders.** `with_shared_secret` is what
  `build()`/`connect()` offer. Anything else — SPIFFE, JWT, mTLS — means
  constructing the `App` yourself and using `from_app`.
- **One node, loopback, plaintext.** The remote-node suite proves routing across
  a node process; it does not exercise multi-hop node topologies, TLS between
  node and app, or a node on another host.
- **Multicast group inbox is unused.** SLIM lets a member observe other members'
  responses (`subscribe_group_inbox`); the A2A spec does not ask for it and this
  binding does not expose it.
