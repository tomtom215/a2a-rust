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

The binding itself is complete **against the specification on upstream's
`main`**: all eleven methods in that inventory, both streaming methods included,
plus multicast — verified end-to-end over a real SLIM datapath and across a real
SLIM node on a socket. Upstream also develops specifications on branches that
`main` has not merged; the next section says which, and what this crate does
about them.

## Relationship to the official `a2a-slimrpc` crate

There are two Rust implementations of this binding. This one, and
[`a2a-slimrpc`](https://crates.io/crates/a2a-slimrpc) in
[`a2aproject/a2a-rs`](https://github.com/a2aproject/a2a-rs) — the A2A project's
own. Both implement all eleven A2A methods, and both pin the same four SLIM
crates. **Neither is a superset of the other**, which is the part worth knowing
before you choose:

| | this crate | official `a2a-slimrpc` |
|---|---|---|
| Eleven A2A methods | yes | yes |
| Multicast (`spec/v1/slimrpc-multicast.md`, on upstream `main`) | **yes** | no |
| Collaborate (`Collaborate` on `experimental.slimrpc.collaborative_channel.v1.CollaborativeChannelService`; its document no longer exists on any upstream branch — see below) | **no** | yes, at 0.2.7 |
| Channel moderator (`spec/v1/slimrpc-channel-moderator.md`, unmerged branch) | no | no |

Multicast and Collaborate are different operations, not two names for one. This
crate's multicast fans a single request out to N agents and returns per-agent
outcomes to the originating client. Collaborate is many-to-many: members of a
channel see each other's traffic, attributed by a `slim-src` metadata key. A
deployment that needs channel semantics is not served by multicast, and vice
versa.

Collaborate is not implemented here because its specification has not been
merged upstream. That is a judgement about a moving target rather than a
statement that it does not matter — the tracking item is B24 in
`docs/v0.9.0-post-release-review.md`, and `scripts/check_slimrpc_spec.sh` fails
CI if upstream gains a specification nobody here has triaged.

The target has since moved further. On 2026-09-03 the branch that held
`spec/v1/slimrpc-collaborative-channel.md` (`feat/slimrpc-collaborative-channel`)
replaced it with `spec/v1/slimrpc-broadcast-live.md` (and on 2026-09-10 split
its transport-independent half into `spec/v1/a2a-broadcast-live.md`, with the
`spec/v1/a2a-shared-task.md` extension it builds on), a different design: a
broadcast routing mode for A2A 1.1's `SendLiveMessage` (its §3 requires A2A 1.1
and that method), which no released A2A specification defines. The official
crate, at 0.2.7 on `a2a-rs` `main`, still ships `Collaborate` against the
withdrawn document. So the row above now records an implementation of a
specification that upstream has retracted, on both sides of the comparison.

Verified 2026-08-26 and re-verified 2026-09-10 by reading both sources and all
upstream branch tips, not by comparing feature lists. On the second date
`check_slimrpc_spec.sh` reported 2 files on upstream `main`, all vendored and
matching, and 5 branch-only specifications, all triaged.

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

## Backpressure

The SLIMRPC specification says nothing about it. What follows is what the code
does, examined 2026-09-10 by reading `agntcy-slim-rpc` 2.3.0,
`agntcy-slim-session` 0.7.10, `agntcy-slim-datapath` 0.18.7 and
`agntcy-slim-service` 0.12.10 — the versions in `Cargo.lock`; line numbers are
theirs — and measured in-process where a test can reach it
(`tests/unicast_backpressure.rs`, `tests/multicast_backpressure.rs`).

### The receive path

Every frame a peer sends crosses five hops on its way to an `EventStream`:

| Hop | Depth | When full |
|---|---|---|
| datapath → app connection (`agntcy-slim-datapath` `message_processing.rs:615`, `connection.rs:270`) | 512 | `send().await` blocks the datapath |
| app → session controller (`agntcy-slim-session` `session_builder.rs:595`, `session_controller.rs:568-580`) | 256 | blocks |
| session controller → app receiver (`session_layer.rs:561`, `session_controller.rs:274`) | **unbounded** | never |
| `agntcy-slim-rpc` dispatcher → the call's own channel (`channel.rs:89`, `:102`, `:144`) | **unbounded** | never |
| this crate's bridge task → `EventStream` (`src/client/bridge.rs:50`, `:80`, `:90`) | 64 | `send().await` parks the bridge task — for at most `slow_consumer_timeout`, then the call is abandoned |

The receiving session layer acks a frame the moment it arrives
(`session_receiver.rs:129` builds the ack before `:154` hands the frame to the
app), so the sender's reliable-delivery state is released whether or not
anything ever reads the frame.

### (a) A unicast stream whose consumer stops reading

**Mechanism.** `SlimRpcTransport` spawns one bridge task per streaming call
(`src/client/mod.rs:465`, `src/client/bridge.rs:80`) that pulls frames from
`Channel::unary_stream` and pushes them into a 64-slot channel with
`send().await` (`bridge.rs:90`). When the consumer stops reading, the bridge
parks on the 65th frame and is no longer polling the SLIM stream, so every
further frame the agent sends is delivered by the dispatcher task into the
channel `agntcy-slim-rpc` registered for the call (`channel.rs:550`), which is
unbounded (`:89`).

**Bound.** `slow_consumer_timeout` —
`SlimRpcTransportBuilder::with_slow_consumer_timeout` (`src/client/mod.rs:158`),
default 30 s (`bridge.rs:68`, the figure `ClientConfig` already allows a server
to produce a first event). The bridge's `send` runs under that timeout
(`bridge.rs:90`). When it expires the bridge drops the SLIM stream (`:99`):
the stream's `DispatcherGuard` (`channel.rs:132`) unregisters the call, and
every frame that still arrives for it is discarded by the dispatcher on
arrival (`channel.rs:150`) instead of kept. The bridge then offers the
consumer one `ClientError::Timeout` naming the setting (`:101`), which
waits behind the buffered events and is followed by the end of the stream.
What a stalled consumer can hold is therefore 64 events in the bridge channel,
64 in the `EventStream`'s own re-framing hop
(`crates/a2a-protocol-client/src/streaming/event_stream.rs:52`), one in
flight between them, and whatever the agent sent inside the window — not
everything it sends until the RPC deadline. That deadline — `with_timeout`,
otherwise `MAX_TIMEOUT` = 36 000 s (`agntcy-slim-rpc` `lib.rs:208`) — still
bounds the call as a whole, but it is a `select!` branch inside the stream
(`channel.rs:539`, `:583`) checked only when the stream is polled, which a
parked bridge does not do; before 2026-09-10 it was the only bound, and a peer
that streamed without end grew the client without limit for as long as the
consumer was not reading. Dropping the `EventStream` closes the bridge's
channel, the parked `send` returns an error, the bridge exits, and its
`DispatcherGuard` unregisters the call's channel and frees what it held
(`channel.rs:123-136`).

**Consequence.** A consumer slower than its agent but reading — one event
per window is enough — costs the *client* memory in proportion to what the
agent sends and loses nothing, and costs the agent nothing: its frames are
acked on receipt, its `send_response_stream` completes, and other calls on the
same channel — which share the session and the dispatcher task — are
unaffected. A consumer that stops costs at most 129 events plus one window of
the agent's output, and is then told, in place of the events it was not
reading. What the agent is *not* told is that the call was abandoned: SLIMRPC
has no client-to-server cancel frame, so the agent streams on to completion or
its own deadline, each frame acked by the client's session layer and dropped
by the dispatcher. That is the agent's send-side work, not the client's
memory, and it is upstream's to remove.

Measured: with one stream held unread, the agent's execution completes, a
second stream of 301 events is delivered in full, a unary call completes, and
the held stream delivers all 301 events with no lag report once read within
the window (`tests/unicast_backpressure.rs`, first two tests). A consumer
away for five 200 ms windows gets 129 events — the two 64-slot hops and the
one in flight — then the one error, then the end, milliseconds after it
starts reading rather than at the 20 s deadline (third test). The second test
fails — 129 of 300 delivered — when the bridge is changed to drop on a full
channel, which is the trade multicast makes (`src/multicast/fanout.rs:116`);
the third fails — all 302 delivered, no error — when the timeout around the
bridge's `send` is removed. Unicast buffers instead of dropping because the
bridge task is private to the call and parks nobody else; it stops buffering
after the window because nothing else was ever going to.

### (b) A server streaming to a peer that stops acking

**Mechanism.** `event_stream` (`src/server/methods/mod.rs:402`) pulls domain
events from the handler's queue as `agntcy-slim-rpc`'s `send_response_stream`
(`rpc_session.rs:219`) asks for them. That function publishes each frame with
`publish().await` and does not wait for its acknowledgement: it keeps one
`CompletionHandle` per frame in a `Vec` (`:234`, `:239`) and awaits them all
after the EOS (`:259`). `publish` puts the frame on the controller's 256-slot
channel (`session_controller.rs:568-580`); the controller's reliable sender
retains a copy in a 512-entry ring, oldest evicted (`session_sender.rs:106`),
starts a timer per frame (`:279`) with the session's settings — 1 s interval,
10 retries, chosen by the initiating client (`agntcy-slim-rpc`
`channel.rs:416-417`; defaults at `session_config.rs:71-72`) — and hands the
frame to the datapath's 512-slot connection channel (`session_controller.rs:269`,
`message_processing.rs:610`).

**Bound.** Everything on the send side is bounded and blocking: 256 + 512
frames of channel depth, 512 retained frames, and per-frame timer state that
lives at most `interval × retries` = 10 s. If the datapath stops draining — a
stalled node link — `publish().await` blocks, `event_stream` stops pulling, the
handler's broadcast queue overruns at `DEFAULT_QUEUE_CAPACITY` = 256
(`crates/a2a-protocol-server/src/streaming/event_queue/mod.rs:45`), and the
reader is handed a lag error (`in_memory.rs:424`) that `event_stream` turns into
an `InternalError` ending the stream. If the datapath drains but the peer never
acks — it has gone away — each frame's timer expires after ten retries;
`on_timer_failure` → `on_failure` (`session_sender.rs:485`, `:454`) clears the
state, the participant marks the peer offline and removes it as an endpoint
(`session_participant.rs:292`), and the frame's `CompletionHandle` resolves
**`Ok(())`** (`session_sender.rs:478`: a missing ack is read as the peer being
unreachable, not as a failed send). The whole handler is additionally bounded
by the RPC deadline (`rpc_session.rs:102`, `:116`), 10 h when the client sent
none. The `usize::MAX` in the message-size table on `SlimRpcTransport` is
tonic's *encoder* limit, a size, not a queue depth; it plays no part here.

**Consequence.** A server streaming to a peer that has vanished neither grows
without bound nor hangs: it finishes about 10 s after its last frame and
reports success. `SendStreamingMessage` toward a dead peer completes as `Ok` on
the agent side with nothing delivered; the agent cannot tell the two apart, and
the task's stored state is what records that the work happened. This path is
read, not run: an in-process test cannot kill the peer's session layer without
killing the datapath both ends share.

### (c) A multicast request (`send_message`)

**Mechanism.** `multicast_unary` (`src/multicast/mod.rs:193`) is the same
response stream as unicast, over a group session, and its frames arrive through
the same unbounded per-call channel (`channel.rs:89`). The consumer is
`send_message`'s own loop, which polls continuously — there is no application
consumer in between — and files one decoded response per source into a map
keyed by member (`:208`, `:227`), a later response from a member replacing its
earlier one.

**Bound.** One map entry per responding member. The loop ends when every
member has sent EOS, on `DeadlineExceeded` (`:250`) — `with_timeout`, otherwise
10 h — or on an interaction-level error, and `collect_outcomes` (`:308`) then
emits exactly one outcome per invited member. The per-call channel holds only
what arrives between two polls of a loop that does nothing else.

**Consequence.** Bounded by member count and by time. The knob that matters is
`with_timeout`: without it a silent member holds the call open for
`MAX_TIMEOUT`. The silent-member case is covered by `tests/multicast.rs`
(`a_silent_agent_is_a_failed_outcome_not_a_missing_one`).

### Not covered here

Admission. Each new `rpc-id` on a session spawns a handler task
(`agntcy-slim-rpc` `server.rs:337`) with no concurrency cap in SLIMRPC itself;
the `RequestHandler`'s own `HandlerLimits` apply per call. That is request-rate
control rather than stream backpressure, and this section does not examine it.

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
| SPIFFE trust-domain boundary | **verified** — an unfederated domain's SVID is refused | `spiffe_federation.rs` |
| SPIFFE federation between domains | **verified**, incl. a cross-domain A2A call | `spiffe_federation.rs` |
| Credential rotation mid-session | **verified** — agent keeps answering, old SVID stops verifying | `spiffe_rotation.rs` |
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

## Example

`examples/in_process.rs` is the smallest complete deployment: one in-process
SLIM `Service`, an agent served by `SlimRpcServer` on a `RequestHandler`, and
an ordinary `A2aClient` built on `SlimRpcTransport` through
`with_custom_transport`. The client sends one blocking message and one
streaming message, prints what comes back, shuts both ends down and exits 0.
No node, no network, no credential beyond a shared secret.

```
cargo run --example in_process
```

```
agent  slim://org/demo/echo_agent serving 11 methods
client slim://org/demo/caller dialling slim://org/demo/echo_agent

SendMessage("hello")
  task 9d91def0-f5c7-4b7a-b10d-f3a0ddb927b6 is Completed
  artifact echo: echo: hello

SendStreamingMessage("hello, streaming")
  task e177f82b-e2c6-478e-a6e3-13fa08a8bd9a Submitted
  status Working
  artifact echo: echo: hello, streaming
  status Completed
  stream ended
```

The setup is the one `tests/e2e.rs` uses, copied rather than shared so the
file is complete on its own. CI runs it after the test step, so the example
cannot rot while the tests stay green.

## Tests

```
cargo test -- --test-threads=1                      # 72 tests, plus the doc tests
SPIRE_BIN_DIR=... cargo test -- --ignored           # + 9 against real SPIRE
```

81 tests across ten topologies — 39 unit, 33 integration, 9 needing SPIRE,
counted from the 2026-09-10 run. None are mocked, and each topology exists
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
| `spiffe_federation.rs` | **two** SPIRE deployments | that a trust domain is a boundary, and that federation crosses it |
| `spiffe_rotation.rs` | SPIRE with 40-second SVIDs | an agent outliving the credential it started with |

Two further suites reuse the first two topologies to measure a property
rather than a method: `unicast_backpressure.rs` holds a point-to-point stream
unread and checks that the agent, the other calls on the channel, and every
event survive it; `multicast_backpressure.rs` does the same in a group, where
the answer is different. Both are explained under Backpressure above.

The three SPIFFE suites are `#[ignore]`d because they need `spire-server` and
`spire-agent` on `PATH` or in `SPIRE_BIN_DIR`; CI installs them and runs
`--ignored` explicitly. The testbed *panics* rather than skipping when they are
missing, so it can never quietly report coverage it does not have.
`spiffe_rotation.rs` is slow on purpose — it waits for real wall-clock expiry,
which is the only way to test what happens after it.

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

`spiffe_federation.rs` found that a registration entry naming `-federatesWith`
is rejected outright unless that trust domain's bundle is *already* imported —
so bundles must be exchanged before entries are created, which is why the
testbed splits `start_with` from `register` instead of doing both at once.

## Limitations

- **One machine.** `out_of_process.rs` puts a real OS process boundary between
  the apps and the node, which is the part of "another host" that reproduces on
  a single machine. Actual cross-host behaviour — real network loss, latency,
  MTU, NAT — is not exercised here.
- **Federation is by manual bundle exchange, not a bundle endpoint.** SPIRE
  supports both; `spire-server bundle set` needs no second listener and no
  refresh interval to wait out, which suits a test. A deployment using the
  `https_spiffe` or `https_web` bundle-endpoint profiles exercises a code path
  nothing here touches.
- **Rotation is tested for JWT-SVIDs, which is what SLIM's app identity uses.**
  X.509 SVID rotation, and rotation of the *node's* TLS certificate underneath a
  live connection, are not exercised.
- **No sustained load.** Rotation is verified against a live agent, but with a
  handful of calls either side, not traffic. Nothing here would catch a leak or
  a slow degradation that only appears over hours.
