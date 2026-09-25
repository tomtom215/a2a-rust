<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

<p align="center">
  <a href="https://a2a-rust.com">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="book/static/brand/og-card-editorial-dark.png">
      <img alt="a2a-rust — Agent2Agent (A2A) Protocol SDK for Rust" src="book/static/brand/og-card-editorial-light.png" width="840">
    </picture>
  </a>
</p>

# a2a-rust — Agent2Agent (A2A) Protocol SDK for Rust

[![CI](https://github.com/tomtom215/a2a-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/tomtom215/a2a-rust/actions/workflows/ci.yml)
[![TCK](https://github.com/tomtom215/a2a-rust/actions/workflows/tck.yml/badge.svg)](https://github.com/tomtom215/a2a-rust/actions/workflows/tck.yml)
[![codecov](https://codecov.io/gh/tomtom215/a2a-rust/graph/badge.svg)](https://codecov.io/gh/tomtom215/a2a-rust)
[![Crates.io](https://img.shields.io/crates/v/a2a-protocol-sdk.svg)](https://crates.io/crates/a2a-protocol-sdk)
[![docs.rs](https://img.shields.io/docsrs/a2a-protocol-sdk)](https://docs.rs/a2a-protocol-sdk)
[![Guide](https://img.shields.io/badge/guide-a2a--rust.com-blue)](https://a2a-rust.com)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org)
[![A2A Conformance](https://img.shields.io/badge/official%20TCK-88%2F114%20MUST%2C%204%20failing-blue)](docs/official-tck-findings.md)

Pure Rust implementation of the [**Agent2Agent (A2A) protocol**](https://a2a-protocol.org/), written against the **v1.0.1** wire specification — the open, vendor-neutral standard for AI-agent interoperability.

Build, connect, and orchestrate AI agents with a type-safe, async-first SDK spanning four transports — JSON-RPC 2.0, REST, WebSocket, and gRPC — for both client and server.

## About

The A2A protocol was originally developed by Google and [donated to the Linux Foundation](https://developers.googleblog.com/en/google-cloud-donates-a2a-to-linux-foundation/) in June 2025. The A2A project maintains its own [official SDKs](https://a2a-protocol.org/latest/sdk/) and publishes the specification and conformance suite this implementation is measured against.

**This is an independent project.** It is not affiliated with, endorsed by, or governed by the A2A project, the Linux Foundation, or Google, and it is not an official SDK. It tracks the published v1.0.1 specification (released 2026-05-28); the protocol version on the wire remains `1.0`, because §3.6 keeps patch numbers out of requests, responses and Agent Cards. It is graded against the A2A project's two official conformance suites, the Technology Compatibility Kit and ACTS; where it falls short of either, this repository records exactly where and why (see [Project Status](#project-status)).

## Features

### Protocol & Transport

| | |
|---|---|
| **A2A v1.0.1 wire types** | The spec's structs, enums, and fields, with serde annotations matched to the wire format |
| **Quad transport** | JSON-RPC 2.0, REST, WebSocket (`websocket`), and gRPC (`grpc`) — client and server |
| **SLIMRPC binding** | A2A over the [AGNTCY SLIM](https://github.com/agntcy/slim) fabric via [`a2a-protocol-slimrpc`](bindings/a2a-protocol-slimrpc) — all eleven methods plus multicast. Community-contributed binding, **not** part of the ratified v1.0 spec, and outside the TCK conformance claim |
| **SSE streaming** | Real-time `SendStreamingMessage` / `SubscribeToTask` with broadcast multi-subscriber event streams |
| **Push notifications** | Pluggable `PushSender` trait with HTTP webhook implementation |
| **Agent card discovery** | `/.well-known/agent-card.json` serving + client-side resolution; hot-reload via file polling or SIGHUP |
| **Agent card signing** | JWS/ES256 with RFC 8785 JSON canonicalization (`signing` feature) |
| **HTTP caching** | `ETag`, `Last-Modified`, `304 Not Modified` for agent card endpoints |

### Server Framework

| | |
|---|---|
| **Pluggable stores** | `TaskStore` / `PushConfigStore` traits; in-memory defaults + SQLite (`sqlite`) + PostgreSQL (`postgres`) with migrations |
| **Multi-tenancy** | Tenant-aware stores, `PerTenantConfig` for per-tenant limits, `TenantResolver` strategies (header, bearer, path) |
| **Executor ergonomics** | `agent_executor!` macro, `EventEmitter`, `boxed_future` — no manual `Pin<Box<dyn Future>>` |
| **Interceptors** | Client `CallInterceptor` + server `ServerInterceptor` chains for auth, logging, etc.; `ServerInterceptor::on_complete` runs once per call with its outcome — succeeded, failed or cancelled — so cleanup cannot be skipped by an error or a client that disconnects |
| **State validation** | `TaskState::can_transition_to()` enforces valid state machine transitions |
| **Rate limiting** | Built-in `RateLimitInterceptor` with fixed-window per-caller limiting |
| **Graceful shutdown** | Ends work instead of orphaning it, and reports what it could not end. `Server::serve_with_shutdown()` stops accepting, lets in-flight tasks finish for up to `completion_grace`, cancels the rest and gives their executors `task_grace` to write a terminal event, then drains connections; its `ServeReport` names any task that ignored cancellation and any connection abandoned at the deadline. The gRPC and WebSocket dispatchers' `serve_with_shutdown()` do the same, and behind Axum `RequestHandler::finish_in_flight()` runs the task phases on its own. `RequestHandler::shutdown()` then runs the executor's cleanup hook; its `ShutdownReport` counts any live stream it cut |
| **Server startup** | `serve()` / `serve_with_addr()` reduce ~25-line hyper boilerplate to one call. `Server::bind()` adds what a deployment needs on top: a shutdown signal, a `max_connections` ceiling, and traced connection errors |

### Client

| | |
|---|---|
| **Retry policy** | Configurable `RetryPolicy` with jittered exponential backoff (connection errors, timeouts, 429/502/503/504) |
| **Idempotency keys** | A client-supplied key on `SendMessage` that the server deduplicates on, so a send that failed ambiguously can be retried without starting a second task. An extension (`https://a2a-rust.com/extensions/idempotency/v1`), **not** part of A2A v1.0, advertised on the agent card exactly when the configured `TaskStore` supports it |
| **TLS support** | HTTPS via `rustls`, no OpenSSL dependency — on by default in the client/SDK (`tls-rustls`; `default-features = false` opts either out), and the server's push sender delivers to HTTPS webhooks with it |
| **Axum integration** | Feature-gated `A2aRouter` for idiomatic Axum servers (`axum` feature) |
| **Zero framework lock-in** | Core built on raw `hyper` 1.x; Axum optional, or bring your own |

### Observability & Operations

| | |
|---|---|
| **OpenTelemetry** | Native OTLP metrics export — request counts, latency histograms, error rates, queue depth, pool stats, **persistence failures and push-delivery outcomes** (`otel` feature). A CI gate asserts the exporter forwards every `Metrics` callback, so a new one cannot be added and silently not exported |
| **Metrics trait** | Pluggable callbacks for requests, responses, errors, latency, connection pool statistics, background persistence failures, and push-delivery outcomes. The last two are the paths a client cannot observe: a stream delivers its events whether or not the store accepted them |
| **Tracing** | Structured logging via `tracing` crate, zero cost when disabled |
| **Request ID propagation** | `CallContext::request_id` auto-extracted from `X-Request-ID` header |

### Security & Hardening

| | |
|---|---|
| **Authentication** | Bearer-token, API-key and JWT/OIDC interceptors. A refused credential answers each binding's own status — HTTP `401` with `WWW-Authenticate`, or `403`; gRPC `UNAUTHENTICATED` / `PERMISSION_DENIED` — so a client knows to refresh its token ([ADR 0014](docs/adr/0014-auth-rejection-status.md)) |
| **Request hardening** | Body size limits, Content-Type validation, path traversal protection, query length limits, message parts refused in media types the agent card does not declare (`allow_undeclared_input_modes()` opts out), and split liveness (`/health`) / readiness (`/ready`, probes the task store) endpoints |
| **SSRF protection** | Push webhook URL validation, header injection prevention, SSE memory limits |
| **CORS support** | `CorsConfig` for browser-based clients with preflight handling |
| **Executor timeout** | Bounded by default (1 hour) so a hung executor cannot pin a task, its queue and its cancellation token forever; tune with `with_executor_timeout()` or opt out explicitly with `without_executor_timeout()` |
| **Task eviction** | TTL-based eviction, capacity limits, amortized sweeps, cursor-based pagination |

### Quality

| | |
|---|---|
| **Mutation-tested** | `cargo-mutants` runs on every pull request, on the lines it changes (`--in-diff`), and fails the build if any mutant goes undetected by the test suite; mutants that time out are reported separately in the job summary rather than failing the build. A full sweep runs weekly and on demand |
| **No `unsafe`** | `#![forbid(unsafe_code)]` at the root of all four published library crates, the benches harness crate, and the TCK runner; zero `unsafe` in `crates/*/src`, `tck/src`, or `benches/src`. The attribute is an inner one, so it reaches neither build scripts nor bench targets, and two kinds of file outside its reach do use `unsafe`: five `build.rs` files — the three published crates' that compile protobuf, the TCK runner's and the ITK's — each wrap `std::env::set_var("PROTOC", …)` in it, and `benches/benches/memory_overhead.rs` carries an `unsafe impl GlobalAlloc` for its allocation counter. The out-of-workspace `a2a-protocol-slimrpc` binding does not carry the attribute either, though it contains no `unsafe` |
| **Regression-gated benchmarks** | Pull requests run `transport_throughput` and `protocol_overhead` twice (base branch vs PR) and fail when the 95 %-CI lower bound of a benchmark's median regression exceeds 50 % (default; `from_str/16384` is excluded from the gate outright, with the measurements that justified it in `benchmarks.yml`, because a 75 % override was tried and was not enough) — only statistically confident, substantial regressions trip the gate. See [`book/src/reference/regression-gate.md`](book/src/reference/regression-gate.md) for the threshold's derivation and the runner-noise limitations behind it |
| **Conformance-gated** | The in-repo conformance runner grades all four bindings — JSON-RPC, REST, WebSocket, and gRPC — plus cross-binding equivalence, on every push to `main` and every pull request. Measurement against the A2A project's *official* TCK is reported separately under [Project Status](#project-status), including what that suite does not cover |

## Crate Structure

| Crate | Purpose | When to Use |
|---|---|---|
| [`a2a-protocol-types`](crates/a2a-protocol-types) | All A2A wire types — `serde` only, no I/O | You need types without the HTTP stack |
| [`a2a-protocol-client`](crates/a2a-protocol-client) | HTTP client for A2A requests | Building an orchestrator, gateway, or test harness |
| [`a2a-protocol-server`](crates/a2a-protocol-server) | Server framework for A2A agents | Building an agent that handles A2A requests |
| [`a2a-protocol-sdk`](crates/a2a-protocol-sdk) | Umbrella re-export + prelude | Quick-start / full-stack usage |
| [`a2a-protocol-slimrpc`](bindings/a2a-protocol-slimrpc) | A2A over the AGNTCY SLIM fabric | Your agents already live on SLIM |

`a2a-protocol-client` and `a2a-protocol-server` are **siblings** — neither depends on the other. Use only what you need.

`a2a-protocol-slimrpc` sits outside the workspace with its own lockfile, because
`agntcy-slim-rpc` brings 359 transitive dependencies (including a native C
crypto build) against 11 for `a2a-protocol-types` (normal dependencies, as
`cargo tree -e normal` counts them, 2026-09-25). None of that reaches the four
crates above, which do not depend on it. It is versioned independently and is
currently on the `0.6` line —
[`bindings/a2a-protocol-slimrpc/Cargo.toml`](bindings/a2a-protocol-slimrpc/Cargo.toml)
is the authority for its exact version — see
[the book chapter](https://a2a-rust.com/bindings/slimrpc.html) for why, and for
the version-coupling rule that independence does *not* remove.

## Quick Start

### Add the dependency

```toml
[dependencies]
a2a-protocol-sdk = "0.14"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### A complete agent, and a client that calls it

One file, `src/main.rs`. It starts the agent on a port the OS picks, then
sends it a message and streams a second one. `cargo run` prints `Hello, Tom!`
and then the streamed events.

```rust,no_run
use std::sync::Arc;

use a2a_protocol_sdk::prelude::*;

struct MyAgent;

// `agent_executor!` writes the `AgentExecutor` impl: no `Pin<Box<dyn Future>>`
// by hand.
agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    let who = ctx.message.text().unwrap_or("world");
    emit.artifact("greeting", vec![Part::text(format!("Hello, {who}!"))], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The server, on a port the OS picks.
    let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).build()?);
    let addr = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(handler)).await?;

    // A client for it.
    let client = ClientBuilder::new(format!("http://{addr}")).build()?;
    let reply = client
        .send_message(MessageSendParams::new(Message::user_text("m1", "Tom")))
        .await?;
    if let SendMessageResponse::Task(task) = reply {
        println!("{}", task.text().unwrap_or("(no text)")); // Hello, Tom!
    }

    // The same call, streamed: each event as the agent emits it.
    let mut stream = client
        .stream_message(MessageSendParams::new(Message::user_text("m2", "Ana")))
        .await?;
    while let Some(event) = stream.next().await {
        match event? {
            StreamResponse::StatusUpdate(ev) => println!("status: {:?}", ev.status.state),
            StreamResponse::ArtifactUpdate(ev) => println!("artifact: {}", ev.artifact.id),
            // `StreamResponse` is `#[non_exhaustive]`: keep a catch-all.
            _ => {}
        }
    }
    Ok(())
}
```

`AgentExecutor` is object-safe — its methods return `Pin<Box<dyn Future>>` —
so `RequestHandler` and the dispatchers are not generic over your agent; they
hold it as `Arc<dyn AgentExecutor>`. `serve_with_addr` returns once the
listener is bound; `serve` runs until the process ends, for a standalone
server. `RestDispatcher` serves the HTTP+JSON binding the same way, and the
[book](https://a2a-rust.com/) covers the gRPC and WebSocket
ones.

This program is compiled by `cargo test --workspace` (the `a2a-book-tests`
crate includes this README), so it cannot quietly stop compiling as the
Quick Start once did.

## Examples

### Incident-Response Agent Team (the multi-agent tour)

The hands-on answer to "how is an agent different from a wrapped prompt?":
three cooperating agents triage a production incident — a vague alert parks
the task in `INPUT_REQUIRED`, the operator's answer resumes the *same task*,
the orchestrator delegates to a deterministic log-search agent and an
LLM-backed runbook agent over real A2A calls, progress streams live, the
incident report lands as an artifact, and a parked task can be cancelled.
Runs fully local with Qwen3.5-0.8B (a ~500 MB Apache-2.0 model, via llama-server or Ollama) or
with no model at all:

```bash
cargo run -p incident-response
```

### Agent Team (Full Dogfood)

A 4-agent team that exercises the SDK broadly — 102 end-to-end tests on the default feature set, which already enables WebSocket, gRPC, Axum, SQLite, signing and OTel (87 with `--no-default-features`; both figures are what `cargo run -p agent-team` prints) covering all four transports (JSON-RPC, REST, WebSocket, gRPC), streaming, push notifications, agent-to-agent orchestration, cancellation, concurrency stress, multi-tenancy, large payloads, metrics, SDK regression testing, batch JSON-RPC, auth rejection, extended/dynamic agent cards, HTTP caching, backpressure, agent card signing, Axum framework integration, and SQLite-backed stores:

```bash
cargo run -p agent-team

# The same, with every feature (the defaults already cover the list above)
cargo run -p agent-team --all-features
```

### Hello Agent (smallest complete agent)

The whole SDK in one screen — 28 lines of code above its tests (counted
2026-09-24, blank and comment lines excluded), the SDK plus `tokio` for the
runtime, no SDK feature flags. It greets whoever sends it a message:

```bash
cargo run -p hello-agent

curl -X POST http://127.0.0.1:3000 \
  -H 'content-type: application/json' -H 'A2A-Version: 1.0' \
  -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Tom"}]}}}'
```

It depends on the same two crates the Quick Start names, so a gap in the
prelude shows up there too; the Quick Start program itself is compiled by
`a2a-book-tests`, as noted above.

### Deploy Agent (the other end of the funnel)

```sh
cargo run -p deploy-agent
docker build -f examples/deploy-agent/Dockerfile -t deploy-agent .
kubectl apply -f examples/deploy-agent/deployment.yaml
```

`hello-agent` is the smallest agent that answers A2A; this is the smallest one
you can ship. Environment configuration, `/healthz` and `/readyz`, `SIGTERM`
draining, a `0.0.0.0` bind, a two-stage container and a Kubernetes manifest
whose probes point at those endpoints. Its sharpest test asserts the agent card
advertises the **public** URL and never leaks the bind address — the deployment
bug whose only symptom is clients failing to call back. See
[`examples/deploy-agent`](examples/deploy-agent).

### Echo Agent

A minimal example demonstrating both JSON-RPC and REST transports with synchronous and streaming modes:

```bash
cargo run -p echo-agent
```

### Multi-Language Agent Team

A Rust coordinator agent that delegates to worker agents written in Python, JavaScript, Go, and Java. It shows the shape of cross-language delegation; it does not prove it on its own — CI runs it with every worker unreachable, so a green job there means the coordinator's A2A surface works, not that four languages round-tripped. Cross-SDK interoperability is measured by `tck.yml`'s cross-language jobs and `scripts/go_sdk_interop.sh` instead:

```bash
# Start the ITK worker agents first (see itk/README.md), then:
cargo run -p multi-lang-team
```

### AI Framework Integrations

Real LLM agents behind the A2A protocol — both pass the in-repo TCK (JSON-RPC
binding: 21/21 graded, 1 N/A, gated on every push and pull request by
`tck.yml`'s `tck-example-agents` job) and run against hosted providers or any
local OpenAI-compatible server. Each defaults to a small local model name
(genai sends it to `localhost:11434`; rig also needs `OPENAI_BASE_URL` pointed
at your local server — see each example's page), so name a hosted model to use
a key:

```bash
# rig AI framework (https://github.com/0xPlaygrounds/rig)
OPENAI_API_KEY=sk-... RIG_MODEL=gpt-4o-mini cargo run -p rig-a2a-agent

# genai multi-provider LLM client (https://crates.io/crates/genai)
OPENAI_API_KEY=sk-... GENAI_MODEL=gpt-4o-mini cargo run -p genai-a2a-agent
```

A plain `cargo run` is a self-driving demo: it drives every method over every
binding, prints whether the model answered, and exits; an answer given without
a model is labelled as a mechanical fallback, never passed off as the model's.
Set `A2A_BIND_ADDR` to serve instead, where a provider error fails the task.

### Technology Compatibility Kit (TCK)

A standalone conformance test runner that grades an A2A server over any of
the four bindings — JSON-RPC, REST, WebSocket and gRPC — and `tck.yml` runs
it against this repository's server on all four:

```bash
# Test a local server
cargo run -p a2a-tck -- --url http://localhost:8080 --binding jsonrpc

# Run the full cross-language ITK (requires Docker)
docker compose -f itk/docker-compose.yml up --build --abort-on-container-exit
```

## Command line

`a2a` is a command-line client over `a2a-protocol-client`: fetch a card, send
or stream a message, get, cancel and list tasks, all as JSON, over any of the
four bindings. It is **unpublished** — a `publish = false` workspace member,
built from this repository, not on crates.io:

```bash
cargo run -p a2a-cli -- card http://127.0.0.1:3111
cargo run -p a2a-cli -- send http://127.0.0.1:3111 "hello"
cargo run -p a2a-cli -- stream http://127.0.0.1:3111 "hello"
```

Commands, flags, a captured transcript and the exit-code table are in
[`tools/a2a-cli/README.md`](tools/a2a-cli/README.md).

## Architecture

```text
┌────────────────────────────────────────────┐
│  Your Code                                 │
│  implements AgentExecutor or uses Client   │
└─────────────────────┬──────────────────────┘
                      │
┌─────────────────────▼──────────────────────┐
│  a2a-protocol-server / a2a-protocol-client │
│ RequestHandler · AgentExecutor · A2aClient │
└─────────────────────┬──────────────────────┘
                      │
┌─────────────────────▼──────────────────────┐
│  Transport Layer                           │
│  JsonRpcDispatcher · RestDispatcher        │
│  A2aRouter (axum, feature-gated)           │
│  WebSocketDispatcher (feature-gated)       │
│  GrpcDispatcher (feature-gated)            │
│  JsonRpcTransport · RestTransport          │
│  WebSocketTransport (feature-gated)        │
│  GrpcTransport (feature-gated)             │
└─────────────────────┬──────────────────────┘
                      │
┌─────────────────────▼──────────────────────┐
│  hyper 1.x · HTTP/1.1 + HTTP/2             │
└────────────────────────────────────────────┘
```

The server uses a 3-layer architecture:
1. **You implement `AgentExecutor`** — your agent logic, produces events via `EventQueueWriter`
2. **`RequestHandler` orchestrates** — manages tasks, stores, push notifications, interceptors
3. **Dispatchers handle HTTP/gRPC** — `JsonRpcDispatcher` (JSON-RPC 2.0), `RestDispatcher` (REST), `A2aRouter` (Axum), `WebSocketDispatcher` (WebSocket), and `GrpcDispatcher` (gRPC) wire hyper/tonic/axum to the handler

## Supported Methods

| Method | JSON-RPC | REST |
|---|---|---|
| `SendMessage` | POST | `POST /message:send` |
| `SendStreamingMessage` | POST → SSE | `POST /message:stream` |
| `GetTask` | POST | `GET /tasks/{id}` |
| `ListTasks` | POST | `GET /tasks` |
| `CancelTask` | POST | `POST /tasks/{id}:cancel` |
| `SubscribeToTask` | POST → SSE | `GET\|POST /tasks/{id}:subscribe` |
| `CreateTaskPushNotificationConfig` | POST | `POST /tasks/{id}/pushNotificationConfigs` |
| `GetTaskPushNotificationConfig` | POST | `GET /tasks/{id}/pushNotificationConfigs/{configId}` |
| `ListTaskPushNotificationConfigs` | POST | `GET /tasks/{id}/pushNotificationConfigs` |
| `DeleteTaskPushNotificationConfig` | POST | `DELETE /tasks/{id}/pushNotificationConfigs/{configId}` |
| `GetExtendedAgentCard` | POST | `GET /extendedAgentCard` |

gRPC serves the same eleven methods as `lf.a2a.v1.A2AService`, and WebSocket
carries them as JSON-RPC messages over one connection.

## Testing

```bash
# Run the test suite. Tests that need a live PostgreSQL are #[ignore]d here
# and run in CI's postgres job; CI's `test` job runs fourteen feature
# combinations per matrix cell
cargo test --workspace --all-features

# Run the end-to-end example
cargo run -p echo-agent

# Lint and format checks
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Build documentation
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Run benchmarks (Criterion suites ×15 — transport, protocol,
# lifecycle, concurrency, cross-language, realistic, error paths, backpressure,
# data volume, memory, enterprise, production, advanced scenarios,
# coordinator chain under fault, and send latency breakdown — the coordinator
# chain is the only agent-level one,
# see book/src/reference/benchmarks.md for caveats on how to read it)
cargo bench -p a2a-benchmarks

# Mutation testing (requires cargo-mutants and cargo-nextest).
# --test-tool=nextest is not optional: .config/nextest.toml supplies the
# per-test kill that stops a hung mutant reporting TIMEOUT instead of caught.
# See book/src/deployment/testing.md for the full CI invocation.
cargo mutants --workspace --test-tool=nextest --all-features

# Fuzz JSON deserialization (requires nightly)
cd fuzz && cargo +nightly fuzz run json_deser
```

## Project Status

Published as `0.x`. All 11 A2A methods are implemented across the four transports, alongside HTTP caching, agent-card signing, optional `tracing` and OpenTelemetry, TLS, and the request-hardening features listed above. The API is still stabilizing — minor versions may carry breaking changes, as described under [Stability](#stability). [`docs/implementation/plan.md`](docs/implementation/plan.md) covers the implementation history and beyond-spec extensions.

Against the A2A project's official Technology Compatibility Kit, **88 of 114 MUST requirements pass and 4 fail** across the three profiles CI grades — 84 on the full profile, and the four capability-negotiation requirements (`CORE-CAP-001` to `004`) on the minimal and required-extension profiles, which a full-capability server cannot exercise (re-measured 2026-09-24 against `a2a-tck@263b9cf`, the same result as 2026-09-01 at `de6af18`). All four failures are the same cause, and it is not a deviation from the specification: the suite grades §5.4's error-mapping table against the copy of the specification it vendors, which its own `specification/version.json` records as A2A **v1.0.0**, taken 2026-03-13. A2A released **v1.0.1** on 2026-05-28, which rewrote six of that table's nine rows. Each of the four fails on exactly the one binding whose cell the two copies disagree about and passes on the bindings where they agree; this SDK answers what the published table says, as does the official Python SDK. They are baselined in `tck/conformance-baseline.json` with the evidence in [§20](docs/official-tck-findings.md#20-grpc-err-002-and-http_json-status-001-the-suites-vendored-specification-is-stale) and [§21](docs/official-tck-findings.md#21-core-cancel-002-and-stream-sub-003-two-more-rows-of-20s-stale-table), and they clear when the suite refreshes its copy — reported upstream as [a2aproject/a2a-tck#231](https://github.com/a2aproject/a2a-tck/issues/231). Of the remaining 22, 21 have no test function in the upstream suite and one (`CARD-EXT-002`) is structurally inapplicable — so they are unmeasured rather than passing. [`docs/official-tck-findings.md`](docs/official-tck-findings.md) has the per-requirement breakdown and reproduction steps; [§16](docs/official-tck-findings.md#16-what-the-21-not-tested-musts-actually-are-one-family-at-a-time) accounts for the 21 family by family — six the upstream suite tags unautomatable, two it has ruled out of scope, and thirteen open backlog items in its own tracker — and shows why none can be closed from this repository.

Against the A2A project's second suite, **ACTS** (a2aproject/a2a-itk), the ITK agent in [`itk/`](itk) is rated **conformant on all three bindings it grades, with every MUST passing**: JSON-RPC 101/101, gRPC 88/88, HTTP+JSON 91/92 (measured 2026-09-25, a2a-rust `d04d64eb`, a2a-itk `429945f6`). The one failure, `REST-CT-001`, is a SHOULD this SDK deliberately does not follow: HTTP+JSON responses are `application/json` rather than `application/a2a+json`, because the official Go SDK's client cannot read errors labelled the other way. The conformance history's [Deliberate deviations](https://a2a-rust.com/reference/conformance-history.html#deliberate-deviations) section gives the evidence and what would reverse it.

[ROADMAP.md](ROADMAP.md) is the honest counterpart to this section: it records where this project's own gates do not yet measure everything they appear to, which conformance claims rest on the in-repo runner rather than the official suite, and which questions are still undecided. Worth reading before depending on this SDK for anything load-bearing.


## Stability

All crates follow [Semantic Versioning 2.0.0](https://semver.org/). During the `0.x` series, minor versions may include breaking changes as the API stabilizes. Since 2026-09-09 that is governed by [STABILITY.md](STABILITY.md): deprecate for at least one minor release before removing, batch breaking changes into at most one minor release per month (a release carrying a fix that cannot wait may declare an exception in its notes), list them under `### Breaking Changes` with a migration each, list observable changes with no signature change under `### Behaviour Changes`, and prove compatibility with `cargo-semver-checks` on every pull request. It also states what is designed to stay compatible and the criteria for `1.0`.

The server crate's twelve public traits — `AgentExecutor`, `TaskStore`, `PushConfigStore`, `PushSender`, `ServerInterceptor`, `TenantResolver`, `Metrics`, `Dispatcher`, `AgentCardProducer`, `RateLimitCounter`, and the two event-queue traits — are **unsealed and will stay that way**: they are the extension points a deployment substitutes its own infrastructure into, and the out-of-workspace [`a2a-protocol-slimrpc`](bindings/a2a-protocol-slimrpc) binding exists only because they are open. New trait methods are always added with defaults so external implementations keep compiling; the rules maintainers follow when doing so — including why a defaulted method is *not* free — are in [CONTRIBUTING.md](CONTRIBUTING.md#extending-a-public-trait). Protocol enums and key structs that can grow with the A2A specification are marked `#[non_exhaustive]` to allow forward-compatible additions in patch releases; the three deliberate exceptions are closed sets fixed by their underlying standards (`ApiKeyLocation` — OpenAPI's header/query/cookie; `JsonRpcResponse` — JSON-RPC 2.0's result/error; and `JsonRpcRequestId` — JSON-RPC 2.0's absent/null/value id states), which stay exhaustive so consumers can match them completely.

## Minimum Supported Rust Version

Rust **1.88** or later (stable), edition 2024.

**Policy.** The MSRV is treated as part of the public API: raising it is a
**minor** version bump, never a patch, and the release notes say so. It is
raised only when a language or standard-library feature earns it — not
incidentally, because a transitive dependency moved. The edition-2024
resolver selects dependency versions compatible with the declared
`rust-version`, and CI builds and tests the workspace on exactly that
toolchain. The full policy is in [STABILITY.md](STABILITY.md#5-minimum-supported-rust-version).

**History.** The floor was 1.93 until 2026-09-09, when it was lowered to
1.88 — the workspace had never needed anything newer, and 1.88 is the
oldest toolchain the current dependency tree (`time`, `serde_with`,
`darling`) declares support for. Lowering it further would mean holding
those crates at older releases, a cost weighed against the adoption benefit
on the [roadmap](ROADMAP.md).

**Depending on the git repository instead of crates.io.** Cargo older than
1.85 cannot read an edition-2024 manifest, and for a git dependency it does
not say so: every lockfile operation fails with
`no matching package named 'a2a-protocol-client' found`, although the crate
is where it always was. Measured 2026-09-24 against `v0.13.0` with
`cargo generate-lockfile`: cargo 1.80.1 and 1.84.1 fail that way, and 1.85.0
resolves. 1.84.1 understands `resolver = "3"`, so the resolver setting is
not the cause; the edition is, and it applies from 0.12.0 on. Resolve and
build with the toolchain you ship, 1.88 or later. Pinning by a short `rev`
resolved on cargo 1.88, 1.96 and 1.98, with and without
`net.git-fetch-with-cli`; a full 40-character SHA is still the safer pin,
because a short one can become ambiguous as the repository grows.

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for coding
standards, testing requirements, and quality gates, and
[GOVERNANCE.md](GOVERNANCE.md) for how decisions get made. Participation is
governed by the [Code of Conduct](CODE_OF_CONDUCT.md) (Contributor Covenant
2.1).

[ROADMAP.md](ROADMAP.md) lists what is committed for upcoming releases,
alongside the verification gaps and open questions noted under
[Project Status](#project-status).

Every commit must be signed off under the
[Developer Certificate of Origin](DCO) (`git commit -s`) by a human git author;
CI enforces this. [PROVENANCE.md](PROVENANCE.md) documents this project's use
of AI coding assistants, the provenance of third-party material in the tree,
and the blanket DCO certification covering commits made before the DCO was
adopted.

To report a security vulnerability, follow [SECURITY.md](SECURITY.md) — not the
public issue tracker.

## License

Apache-2.0 — see [LICENSE](LICENSE), and [NOTICE](NOTICE) for the project's
copyright notice and third-party attributions.
