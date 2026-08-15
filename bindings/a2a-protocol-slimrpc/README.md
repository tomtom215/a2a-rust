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
    .with_shared_secret("caller", std::env::var("SLIM_SECRET")?)?
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
    .with_shared_secret("echo_agent", std::env::var("SLIM_SECRET")?)?
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

## Identity

`with_identity` takes SLIM's own `AuthProvider` and `AuthVerifier`, so every
mechanism SLIM supports works — SPIFFE via SPIRE, JWT, a static token, or a
shared secret. Enumerating them in the binding would mean growing a method every
time SLIM gained one, and silently lagging behind until someone noticed.

```rust,no_run
// SPIFFE: one manager, cloned. `SpireIdentityManager` is a unified
// provider + verifier and generates an MLS signature key at build time, so two
// separately-built managers carry two different keys and their handshake never
// completes.
let mut spiffe = SpireIdentityManager::builder()
    .with_socket_path("/run/spire/agent.sock")
    .with_target_spiffe_id("spiffe://example.org/a2a/echo_agent")
    .with_jwt_audiences(vec!["slim".into()])
    .build()?;
spiffe.initialize().await?;

let server = SlimRpcServer::builder(handler, name)
    .with_identity(
        AuthProvider::spire(spiffe.clone()),
        AuthVerifier::spire(spiffe),
    )
    .build()?;
```

`with_shared_secret` is the convenience for the simplest case. There is no
default: SLIM has no anonymous mode, so a builder with no identity is a build
error rather than something that quietly stands in for one.

Two things about SPIFFE that cost time to discover and are easy to get wrong:

- **One manager, cloned.** Building two managers alike gives them two different
  MLS keys. The symptom is a session that never completes, not an
  authentication error.
- **Distinct SPIFFE IDs per app.** Two SLIM apps holding the *same* SPIFFE ID
  cannot complete an MLS handshake. A process hosting several apps registers
  several entries and selects between them with `with_target_spiffe_id`.

## Security posture

What is proven by a test here, and what is merely available. The distinction is
the point: "supported" and "verified" are different claims, and only the second
is evidence.

| Control | Status | Where |
|---|---|---|
| Server TLS, client verifies | **verified**, incl. refusing an untrusted CA | `remote_node_tls.rs` |
| Mutual TLS (client certificates) | **verified**, incl. refusing no-cert and wrong-CA | `remote_node_mtls.rs` |
| SPIFFE identity via real SPIRE | **verified**, incl. refusing a wrong-audience SVID | `spiffe.rs` |
| JWT identity | **verified** end-to-end | `e2e.rs` |
| Shared-secret identity | **verified** end-to-end | every suite |
| A2A error identity across a node | **verified** | `remote_node.rs` |
| Static-token identity | supported via `with_identity`, untested | — |
| SPIFFE X.509 SVIDs for the *node* link | available in SLIM (`TlsSource::Spire`), unused here | — |

Every negative case above is paired with a control that succeeds in the same
window, because "did not connect" on its own can mean the fixture was broken
rather than the security control working.

## Running a node

`slim-node` is a standalone SLIM node — it routes and runs nothing itself.

```
slim-node --listen 127.0.0.1:46357
slim-node --listen 0.0.0.0:46357 --tls-cert node.pem --tls-key node.key
```

It prints `listening on <addr>` once the socket is accepting, so a supervisor
waits for readiness instead of sleeping. Half a TLS configuration (`--tls-cert`
without `--tls-key`) is refused rather than silently serving plaintext.

Bringing up a node otherwise means installing the full AGNTCY SLIM
distribution, which is a large ask for someone who only wants to try the
binding.

## Tests

```
cargo test                                          # 56 tests
SPIRE_BIN_DIR=... cargo test -- --ignored           # + 3 against real SPIRE
```

59 tests across eight topologies. None are mocked, and each topology exists
because it can fail in a way the ones above it cannot.

| Suite | Topology | What only this can catch |
|---|---|---|
| `e2e.rs` | one in-process `Service` | the eleven methods, error identity, streaming, card advertisement, JWT identity |
| `multicast.rs` | group channel, several agents | per-agent attribution, a silent agent as a failed outcome, failure isolation, per-agent streams |
| `remote_node.rs` | three services, one node, TCP | routing that needs real subscription propagation |
| `remote_node_tls.rs` | same, with **verified TLS** | a TLS path that actually verifies |
| `remote_node_mtls.rs` | same, with **mutual TLS** | a node that authenticates its apps, not just itself |
| `remote_node_multihop.rs` | **two peered nodes** | subscriptions crossing a node-to-node link |
| `out_of_process.rs` | node in a **separate OS process** | anything relying on shared memory or a shared runtime |
| `spiffe.rs` | **real SPIRE** server + agent | workload identity from a real attesting authority |

The SPIFFE suite is `#[ignore]`d because it needs `spire-server` and
`spire-agent` on `PATH` or in `SPIRE_BIN_DIR`; CI installs them and runs
`--ignored` explicitly. The testbed *panics* rather than skipping when they are
missing, so it can never quietly report coverage it does not have.

Three of these found real bugs the tier above could not have:

`remote_node.rs` found that a client never announced its own name to the node,
so nothing could route an agent's reply back and every call failed its session
handshake. In-process, that is invisible.

`multicast.rs` found the response join key was wrong — SLIM names arrive with a
fourth instance component, so every response filed under a name no invited
member matched. All agents answered; all were reported as timeouts.

`spiffe.rs` found that two apps sharing one SPIFFE ID cannot complete an MLS
handshake, which is why the testbed registers an identity per app rather than
one per process.

## Limitations

- **One machine.** `out_of_process.rs` puts a real OS process boundary between
  the apps and the node, which is the part of "another host" that reproduces on
  a single machine. Actual cross-host behaviour — real network loss, latency,
  MTU, NAT — is not exercised here.
- **The SPIFFE trust domain is not federated.** One trust domain, one SPIRE
  server. Cross-trust-domain federation, and rejecting an SVID from a *different*
  trust domain, are not exercised.
- **No credential rotation under load.** SVIDs and certificates are issued once
  per test with an hour of validity. Nothing here runs long enough to see SPIRE
  rotate an SVID mid-session, which is the interesting failure mode in a
  long-lived deployment.
