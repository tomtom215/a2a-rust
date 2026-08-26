# SLIMRPC over AGNTCY SLIM

A2A carried over the [AGNTCY SLIM](https://github.com/agntcy/slim) fabric,
implementing the [SLIMRPC custom protocol binding][spec].

[spec]: https://github.com/a2aproject/experimental-cpb-slimrpc

```text
protocolBinding: https://a2a-protocol.org/bindings/experimental-slimrpc/v1
address:         slim://[node-host[:port]/]domain/namespace/service
service:         lf.a2a.v1.A2AService
```

## Read this first

**This is not part of the A2A specification.** The upstream binding describes
itself as "community contributed … not part of the core A2A specification", and
the ratified v1.0 spec contains no occurrence of "slim" or "agntcy". Nothing
here is required for A2A conformance, and the project's TCK conformance claims
cover the four spec bindings (JSON-RPC, HTTP+JSON, gRPC) plus WebSocket — not
this one.

It exists because the SLIM fabric is where some deployments already are.

The binding itself is complete against upstream's `main`: all eleven A2A
methods, both streaming methods, plus multicast, driving the same
`RequestHandler` the HTTP bindings drive.

## There are two Rust implementations, and neither contains the other

The A2A project ships its own — [`a2a-slimrpc`](https://crates.io/crates/a2a-slimrpc)
in [`a2aproject/a2a-rs`](https://github.com/a2aproject/a2a-rs). Both implement
all eleven A2A methods and pin the same SLIM crates. They diverge in both
directions:

| | this crate | official `a2a-slimrpc` |
|---|---|---|
| Multicast — one request to N agents, per-agent outcomes back to the caller | **yes** | no |
| Collaborate — many-to-many channel, members see each other's traffic, attributed by `slim-src` | **no** | yes |

Multicast's specification is on upstream `main`. Collaborate's is on a branch
`main` has not merged, which is why it is not implemented here; the tracking
item is B24 in the post-release review. Pick on which of the two shapes your
deployment needs, not on which crate looks more complete — neither is.

## Installation

```toml
[dependencies]
a2a-protocol-slimrpc = "0.2"
a2a-protocol-server  = "0.9"
```

### Why two version numbers

`a2a-protocol-slimrpc` is versioned independently of the SDK — it is at `0.2`
while the SDK is at `0.9`. Numbering it to match would claim nine minor
versions of API stability it has not earned, and would force a bump
on every SDK release even when nothing here changed.

Independence applies to the *numbers*, not the schedule. `SlimRpcServer::builder`
takes an `Arc<RequestHandler>` and `agent_interface()` returns an
`AgentInterface`, so `a2a-protocol-server` and `a2a-protocol-types` are **public
dependencies**: your `RequestHandler` must come from the same SDK version this
crate was built against. That is why the requirement is a tight `0.9` and not a
range — allow two and cargo links both, and you get `expected RequestHandler,
found RequestHandler`, which is among the least helpful errors in Rust.

Two consequences:

- Every SDK **minor** bump requires a matching release of this crate.
- If you only need the types the binding hands you, take them from the binding's
  own re-exports rather than a second dependency:

```rust,ignore
use a2a_protocol_slimrpc::{AgentExecutor, RequestHandler, RequestHandlerBuilder};
```

## Server

```rust,ignore
use a2a_protocol_slimrpc::{SlimName, SlimRpcServer};

let name = SlimName::parse("slim://org/demo/echo_agent")?;
let server = SlimRpcServer::builder(handler, name)
    .with_shared_secret("echo_agent", std::env::var("SLIM_SECRET")?)?
    .build()?;

// Advertise it, or callers cannot discover the binding — §5 makes the card the
// only way a client learns where to connect.
card.supported_interfaces.push(server.agent_interface());

server.serve().await?;
```

## Client

```rust,ignore
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_slimrpc::{SlimName, SlimRpcTransport};

let agent = SlimName::parse("slim://org/demo/echo_agent")?;
let transport = SlimRpcTransport::builder(agent)
    .with_shared_secret("caller", std::env::var("SLIM_SECRET")?)?
    .connect()?;

let client = ClientBuilder::new("slim://org/demo/echo_agent")
    .with_custom_transport(transport)
    .build()?;
```

Retries, interceptors, auth and the typed method surface behave exactly as they
do over JSON-RPC. Only the wire underneath changes.

## Why the code blocks on this page are not compiled

Every other Rust block in this book is compiled as a doctest by the
`a2a-book-tests` crate. These are not, and the reason is structural rather than
neglect: `a2a-protocol-slimrpc` is deliberately outside the root workspace with
its own `Cargo.lock`, because `agntcy-slim-rpc` pulls 379 transitive
dependencies including `aws-lc-sys`, a native C crypto build. Against 12 for
`a2a-protocol-types` and 191 for `a2a-protocol-server` at all features, none of
that belongs in the published crates' audit surface — or in the book's test
build.

The snippets above are kept in step with the crate's own README and its test
suite, which do compile and run.

## Traps worth knowing before you start

These each cost real debugging time. All present as something other than what
they are.

- **SLIM names carry a fourth instance component** (`org/ns/agent/NULL_COMPONENT`).
  Key on the full rendering and every multicast response files under a name no
  member matches: all agents answer, and all of them report as timeouts.
- **A client must announce its own name to the node** —
  `app.subscribe(&reply_to, Some(conn))`. `Channel` routes outward only, so
  without it every call fails its session handshake. This is invisible
  in-process; only a remote-node topology catches it.
- **`SpireIdentityManager` must be built once and cloned** for provider and
  verifier, because it holds an MLS signing key — *and* each app needs its own
  SPIFFE ID, since two apps sharing one cannot complete an MLS handshake. Both
  present as a session that never completes, not as an auth error.
- **SPIFFE federation is bundles-before-entries.** A `-federatesWith` entry is
  rejected unless that domain's bundle is already imported.

## What is verified, and what is not

Verified end-to-end across ten topologies, including a real OS process boundary
between apps and node, mTLS, and three suites against a real SPIRE.

Not verified — stated plainly because a documented limitation is a work item,
not a disclaimer:

| Gap | Status |
|---|---|
| Real-network conditions (loss, latency, MTU, NAT) | Everything runs on one machine over loopback |
| Node restart mid-session | Untested |
| Agent-card discovery over SLIM | You must know the agent's SLIM name out of band |
| Federation via bundle endpoint | Manual bundle exchange only; `https_spiffe`/`https_web` profiles untouched |
| X.509 SVID rotation, node TLS cert rotation under a live connection | Untested; rotation coverage is JWT-SVIDs only |
| Push notification *delivery* over SLIM | Config methods speak SLIM; delivery is still an HTTP webhook |
| Static-token identity (`with_identity`) | Supported, no test |
| Sustained load | Rotation is checked with a handful of calls, not traffic |
| TCK coverage | The "TCK all bindings" job does not include this binding |

The specification itself is vendored at `spec/slimrpc_v1/`, so the method
inventory above is checked against the document this binding claims to
implement rather than against the A2A proto as a stand-in, and
`scripts/check_slimrpc_spec.sh` fails CI when upstream moves. That closes the
one claim here that previously could only be verified by opening a browser — it
does **not** make this binding TCK-conformant, which remains out of reach for
the reason given above.

## Further reading

The crate's own [README][readme] goes deeper: the full method inventory, wire
format, error mapping, multicast semantics, identity modes, security posture,
how to run a node, and the test suite layout.

[readme]: https://github.com/tomtom215/a2a-rust/tree/main/bindings/a2a-protocol-slimrpc
