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
cover the three spec bindings (JSON-RPC, HTTP+JSON, gRPC) plus WebSocket,
which §12 admits as a custom binding — four in all, and not this one.

It exists because the SLIM fabric is where some deployments already are.

The binding itself is complete against the A2A 1.0 surface of upstream's
`main`: all eleven A2A methods, both streaming methods, plus multicast, driving
the same `RequestHandler` the HTTP bindings drive.

## There are two Rust implementations, and neither contains the other

The A2A project ships its own — [`a2a-slimrpc`](https://crates.io/crates/a2a-slimrpc)
in [`a2aproject/a2a-rs`](https://github.com/a2aproject/a2a-rs). Both implement
all eleven A2A methods and are built on `agntcy-slim-rpc`. They diverge in both
directions:

| | this crate | official `a2a-slimrpc` |
|---|---|---|
| Multicast — one request to N agents, per-agent outcomes back to the caller | **yes** | no |
| Collaborate — many-to-many channel, members see each other's traffic, attributed by `slim-src` | **no** | yes, at 0.2.7 |

Multicast's specification is on upstream `main`. Collaborate's never reached
`main`: the branch that held it renamed the design twice —
`slimrpc-broadcast-live.md` on 2026-09-02, then `slimrpc-collaborative-task.md`
on 2026-09-11 — and it was that last form upstream merged to `main` on
2026-09-29, while the official crate still ships `Collaborate` against the
withdrawn text. Upstream's `main` now also specifies `SendLiveMessage`, an A2A
1.1 method, which this crate does not implement because no released A2A
specification defines it. The current design is not implemented here because it profiles
an A2A extension whose timeline semantics are A2A 1.1's, which no released A2A
specification defines, and because its native mode needs SLIM shared-responses
group channels this crate does not have. The tracking item is B24 in the
post-release review, and the binding's README carries the dated detail. Pick on
which of the two shapes your deployment needs, not on which crate looks more
complete — neither is.

## Installation

`a2a-protocol-slimrpc` is not yet published to crates.io, so a version
requirement such as `a2a-protocol-slimrpc = "0.7"` does not resolve. Until it
is published, depend on it from a checkout of this repository, and have the
SDK crates resolve to the same checkout:

```toml
[dependencies]
a2a-protocol-slimrpc = { path = "../a2a-rust/bindings/a2a-protocol-slimrpc" }
a2a-protocol-server  = "0.16"

# The binding is built against the SDK crates in the checkout, so yours must
# be those same copies (see "Why two version numbers").
[patch.crates-io]
a2a-protocol-types  = { path = "../a2a-rust/crates/a2a-protocol-types" }
a2a-protocol-client = { path = "../a2a-rust/crates/a2a-protocol-client" }
a2a-protocol-server = { path = "../a2a-rust/crates/a2a-protocol-server" }
```

### Why two version numbers

`a2a-protocol-slimrpc` is versioned independently of the SDK — it is at `0.7`
while the SDK is at `0.15`. Numbering it to match would claim eight minor
versions of API stability it has not earned, and would force a bump
on every SDK release even when nothing here changed.

Independence applies to the *numbers*, not the schedule. `SlimRpcServer::builder`
takes an `Arc<RequestHandler>` and `agent_interface()` returns an
`AgentInterface`, so `a2a-protocol-server` and `a2a-protocol-types` are **public
dependencies**: your `RequestHandler` must come from the same SDK version this
crate was built against. That is why the requirement is a tight pin on one SDK
minor — whichever minor the `[dependencies]` block above names — and not a
range: allow two and cargo links both, and you get `expected RequestHandler,
found RequestHandler`, which is among the least helpful errors in Rust.

Two consequences:

- Every SDK **minor** bump requires a matching release of this crate.
- If you only need the types the binding hands you, take them from the binding's
  own re-exports rather than a second dependency:

```rust,ignore
use a2a_protocol_slimrpc::{AgentExecutor, RequestHandler, RequestHandlerBuilder};
```

The SLIM crates are public dependencies in the same way: `with_identity`,
`from_app` and `from_app_with_connection` take types from `agntcy-slim-auth`,
`-service` and `-session`, and `SlimName::to_proto_name` returns one from
`-datapath`. Depend on the versions this crate was built with —
`agntcy-slim-rpc` 3.4, `agntcy-slim-auth` 0.17, `agntcy-slim-datapath` 0.19,
`agntcy-slim-service` 0.13, `agntcy-slim-config` 0.17 and
`agntcy-slim-session` 0.7 — or cargo links two copies and the types do not
match.

## Server

```rust,ignore
use std::sync::Arc;
use a2a_protocol_slimrpc::{RequestHandlerBuilder, SlimName, SlimRpcServer};

let name = SlimName::parse("slim://org/demo/echo_agent")?;

// Advertise it on the card the handler is built from. A2A §5.2 requires the
// card to declare every binding the agent serves, and clients read the SLIM
// name from this interface's `url` (SLIMRPC §2.1, §9). The server logs a
// warning when it is built if the card does not declare the binding.
card.supported_interfaces.push(name.to_agent_interface());
let handler = Arc::new(
    RequestHandlerBuilder::new(executor).with_agent_card(card).build()?,
);

let server = SlimRpcServer::builder(handler, name)
    .with_shared_secret("echo_agent", std::env::var("SLIM_SECRET")?)?
    .build()?;

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

Retries, interceptors, auth and the typed method surface are the ones you have
over JSON-RPC. The differences are in the wire: a unicast stream's lag arrives
as error text without the `streamLagged` marker (`is_stream_lagged()` is
false; a multicast member's is typed), and a stream carries no event ids to
resume from.

Credentials travel as SLIMRPC session metadata, which is how the SLIMRPC
specification (§3) carries A2A service parameters. Every per-call header the
client sends, including the `authorization` value a `BearerAuthInterceptor`
adds, becomes a metadata entry of the same name, beside `a2a-version`. The
server lowercases the keys (they are case-insensitive) and hands them to the
`RequestHandler` as request headers, so its authentication interceptors and
tenant resolvers read them as they would over HTTP. Three kinds of key are
dropped first: SLIMRPC's own (`rpc-id`, `service`, `method`, the deadline, the
status code and SLIM's `slim-peer-subject`), the proxy-forwarding headers
`x-forwarded-for`, `x-real-ip` and `forwarded` (no proxy appends to them on a
SLIM session, so a caller's value would pick its own rate-limit and throttle
bucket), and any key beginning with `:`. That prefix names what a server
records itself, such as the connection's address (`:peer-addr`), so from a
caller it would be a claim the handler reads as fact. This is separate from the
SLIM transport identity (`with_identity`: SPIFFE, JWT, a static token or a
shared secret), which authenticates the app to the SLIM fabric. The caller's
identity as SLIM verified it, for a session with MLS, is recorded in
`:slim-peer-subject` and `:slim-peer-issuer`, which `SlimPeerAuthInterceptor`,
below, turns into the caller identity.

`SlimPeerAuthInterceptor` authenticates by the peer SLIM verified for an MLS
session instead: `new()` names each such caller, `required()` also refuses a
call without one. A2A §13.3 has requests authenticated with a scheme the
public card declares, and SLIMRPC §8.4 asks the card to say that SLIM
provides authentication, so declare it with the interceptor itself:

```rust,ignore
let auth = SlimPeerAuthInterceptor::required();
auth.declare_on(&mut card); // securitySchemes.slimMls, and a requirement for it
let handler = RequestHandlerBuilder::new(executor)
    .with_agent_card(card)
    .with_interceptor(auth)
    .build()?;
```

A2A has no scheme type for SLIM, so `security_scheme()` declares a mutual-TLS
scheme (`SecurityScheme::MutualTls`, `mtlsSecurityScheme` in the card JSON)
whose description names SLIM: of A2A's kinds, the one whose credential the
transport proves for both peers. The handler's build refuses `required()`
behind a card that declares only a bearer or API-key scheme, which it cannot
check.

The server reads `a2a-version` as the HTTP bindings read the header: absent or
empty means 0.3 (A2A §3.6.2), which it refuses with `VersionNotSupportedError`.
`SlimRpcServer::with_require_version(false)` serves such a call as 1.0 instead,
for a caller that sends no version.

## Why the code blocks on this page are not compiled

Every other Rust block in this book is compiled as a doctest by the
`a2a-book-tests` crate, except one `rust,ignore` block in
[Audit Trail](../deployment/audit.md). The blocks on this page are not, and the
reason is structural rather than neglect: `a2a-protocol-slimrpc` is
deliberately outside the root workspace with its own `Cargo.lock`, because
`agntcy-slim-rpc` pulls 387 transitive dependencies (measured on 2026-10-25 at
the locked `agntcy-slim-rpc` 3.4.6) including `aws-lc-sys`, a native C crypto
build. Against 220 for `a2a-protocol-server` at all features and 11 for
`a2a-protocol-types` at its defaults — all counting normal edges only, as
`cargo tree --edges normal` reports them; add build-dependencies and the server
reading is 256 — none of that belongs in the published crates' audit surface,
or in the book's test build.

The snippets above follow the doctests in the crate's `src/lib.rs`, its
`examples/in_process.rs` and its test suite, which do compile and run; the crate
README's fragments are not compiled.

## Traps worth knowing before you start

These each cost real debugging time. All present as something other than what
they are.

- **SLIM names carry a fourth instance component** (`org/ns/agent/NULL_COMPONENT`).
  Key on the full rendering and every multicast response files under a name no
  member matches: all agents answer, and all of them report as timeouts. A
  member's error carries the same rendering as text, in `MulticastRpc`'s
  `origin`, and this crate keyed it that way until 2026-10-13: an agent that
  refused a broadcast was reported as having timed out.
- **A client must announce its own name to the node** —
  `app.subscribe(&reply_to, Some(conn))`. `Channel` routes outward only, so
  without it every call fails its session handshake. This is invisible
  in-process; only a remote-node topology catches it.
  `SlimRpcTransport::from_app_with_connection` does this when it is given a
  connection id; you only need to do it yourself if you build on
  `slim_rpc::Channel` directly.
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
inventory in the [crate README][readme] is checked against the document this
binding claims to implement rather than against the A2A proto as a stand-in, and
`scripts/check_slimrpc_spec.sh` fails CI when upstream moves. That closes the
one claim here that previously could only be verified by opening a browser — it
does **not** make this binding TCK-conformant, which remains out of reach for
the reason given above.

## Further reading

The crate's own [README][readme] goes deeper: the full method inventory, wire
format, error mapping, multicast semantics, identity modes, security posture,
how to run a node, and the test suite layout.

[readme]: https://github.com/tomtom215/a2a-rust/tree/main/bindings/a2a-protocol-slimrpc
