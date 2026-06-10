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
[![MSRV](https://img.shields.io/badge/rust-1.93%2B-orange.svg)](https://www.rust-lang.org)
[![A2A Conformance](https://img.shields.io/badge/A2A%20v1.0-TCK%20conformant-brightgreen)](tck/)

Pure Rust implementation of the [**Agent2Agent (A2A) protocol**](https://a2a-protocol.org/), built to the final **v1.0.0** wire specification — the open, vendor-neutral standard for AI-agent interoperability.

Build, connect, and orchestrate AI agents with a type-safe, async-first SDK spanning four transports — JSON-RPC 2.0, REST, WebSocket, and gRPC — for both client and server.

## Motivation

The A2A protocol — originally developed by Google and [donated to the Linux Foundation](https://developers.googleblog.com/en/google-cloud-donates-a2a-to-linux-foundation/) in June 2025 — provides a vendor-neutral standard for AI agent interoperability. The [official SDKs](https://a2a-protocol.org/latest/sdk/) cover Python, Go, Java, JavaScript, and C#/.NET, but there is no official Rust implementation. The [community samples](https://github.com/a2aproject/a2a-samples/tree/main/samples) cover the same five languages — Rust is absent there too.

This project aims to be the first **v1.0.0-compliant** Rust SDK for A2A. We intend to contribute this work to the [A2A project](https://github.com/a2aproject) under the Linux Foundation so that Rust has first-class support alongside the other official SDKs.

## Features

### Protocol & Transport

| | |
|---|---|
| **Full A2A v1.0.0 wire types** | Every struct, enum, and field from the spec with correct serde annotations |
| **Quad transport** | JSON-RPC 2.0, REST, WebSocket (`websocket`), and gRPC (`grpc`) — client and server |
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
| **Interceptors** | Client `CallInterceptor` + server `ServerInterceptor` chains for auth, logging, etc. |
| **State validation** | `TaskState::can_transition_to()` enforces valid state machine transitions |
| **Rate limiting** | Built-in `RateLimitInterceptor` with fixed-window per-caller limiting |
| **Graceful shutdown** | `RequestHandler::shutdown()` cancels all tokens and destroys queues |
| **Server startup** | `serve()` / `serve_with_addr()` reduce ~25-line hyper boilerplate to one call |

### Client

| | |
|---|---|
| **Retry policy** | Configurable `RetryPolicy` with jittered exponential backoff (connection errors, timeouts, 429/502/503/504) |
| **TLS support** | HTTPS via `rustls`, no OpenSSL dependency (`tls-rustls` feature) |
| **Axum integration** | Feature-gated `A2aRouter` for idiomatic Axum servers (`axum` feature) |
| **Zero framework lock-in** | Core built on raw `hyper` 1.x; Axum optional, or bring your own |

### Observability & Operations

| | |
|---|---|
| **OpenTelemetry** | Native OTLP metrics export — request counts, latency histograms, error rates, queue depth, pool stats (`otel` feature) |
| **Metrics trait** | Pluggable callbacks for requests, responses, errors, latency, and connection pool statistics |
| **Tracing** | Structured logging via `tracing` crate, zero cost when disabled |
| **Request ID propagation** | `CallContext::request_id` auto-extracted from `X-Request-ID` header |

### Security & Hardening

| | |
|---|---|
| **Enterprise hardening** | Body size limits, Content-Type validation, path traversal protection, query length limits, health endpoints |
| **SSRF protection** | Push webhook URL validation, header injection prevention, SSE memory limits |
| **CORS support** | `CorsConfig` for browser-based clients with preflight handling |
| **Executor timeout** | Configurable via `RequestHandlerBuilder::with_executor_timeout()` to kill hung executors |
| **Task eviction** | TTL-based eviction, capacity limits, amortized sweeps, cursor-based pagination |

### Quality

| | |
|---|---|
| **Mutation-tested** | `cargo-mutants` runs on every pull request (incremental, changed-files only) and fails the build if any mutant goes undetected by the test suite; mutants that time out are reported separately in the job summary rather than failing the build. A full-sweep matrix runs on demand |
| **No `unsafe`** | `#![forbid(unsafe_code)]` at every library crate root; zero `unsafe` blocks in `crates/`, `tck/`, or the benches harness |
| **Regression-gated benchmarks** | Pull requests run `transport_throughput` and `protocol_overhead` twice (base branch vs PR) and fail when the 95 %-CI lower bound of a benchmark's median regression exceeds 50 % — only statistically confident, substantial regressions trip the gate. See [`book/src/reference/regression-gate.md`](book/src/reference/regression-gate.md) for the threshold's derivation and the runner-noise limitations behind it |
| **TCK conformance** | The A2A v1.0 Technology Compatibility Kit runs on every push to `main` and every pull request, covering the JSON-RPC and REST bindings; the WebSocket and gRPC transports are exercised by the agent-team end-to-end suite rather than the TCK |

## Crate Structure

| Crate | Purpose | When to Use |
|---|---|---|
| [`a2a-protocol-types`](crates/a2a-protocol-types) | All A2A wire types — `serde` only, no I/O | You need types without the HTTP stack |
| [`a2a-protocol-client`](crates/a2a-protocol-client) | HTTP client for A2A requests | Building an orchestrator, gateway, or test harness |
| [`a2a-protocol-server`](crates/a2a-protocol-server) | Server framework for A2A agents | Building an agent that handles A2A requests |
| [`a2a-protocol-sdk`](crates/a2a-protocol-sdk) | Umbrella re-export + prelude | Quick-start / full-stack usage |

`a2a-protocol-client` and `a2a-protocol-server` are **siblings** — neither depends on the other. Use only what you need.

## Quick Start

### Add the dependency

```toml
[dependencies]
a2a-protocol-sdk = "0.6"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Implement an agent

```rust
use a2a_protocol_sdk::prelude::*;

struct MyAgent;

// The agent_executor! macro eliminates Pin<Box<dyn Future>> boilerplate
agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);

    emit.status(TaskState::Working).await?;
    emit.artifact("result", vec![Part::text("Hello from my agent!")], None, Some(true)).await?;
    emit.status(TaskState::Completed).await?;

    Ok(())
});
```

> **Note:** `AgentExecutor` is object-safe — methods return `Pin<Box<dyn Future>>`.
> This means `RequestHandler`, `RestDispatcher`, and `JsonRpcDispatcher` are **not generic**;
> they store the executor as `Arc<dyn AgentExecutor>` for easy composition.

### Start a server

```rust
use std::sync::Arc;
use a2a_protocol_sdk::prelude::*;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyAgent)
        .with_agent_card(agent_card)
        .build()
        .expect("build handler"),
);

// One-liner server startup (replaces ~25 lines of hyper boilerplate)
serve("0.0.0.0:3000", JsonRpcDispatcher::new(handler)).await?;
```

### Use the client

```rust
use a2a_protocol_sdk::prelude::*;

let client = ClientBuilder::new("http://localhost:8080")
    .with_retry_policy(RetryPolicy::default())  // automatic retry on transient errors
    .build()
    .expect("build client");

// Synchronous request
let response = client
    .send_message(params)
    .await
    .expect("send_message");

// Streaming request
let mut stream = client
    .stream_message(params)
    .await
    .expect("stream_message");

while let Some(event) = stream.next().await {
    match event? {
        StreamResponse::StatusUpdate(ev) => println!("Status: {:?}", ev.status.state),
        StreamResponse::ArtifactUpdate(ev) => println!("Artifact: {}", ev.artifact.id),
        StreamResponse::Task(task) => println!("Task: {}", task.id),
        StreamResponse::Message(msg) => println!("Message: {:?}", msg),
        // StreamResponse is #[non_exhaustive] — always keep a catch-all.
        _ => {}
    }
}
```

## Examples

### Incident-Response Agent Team (start here)

The hands-on answer to "how is an agent different from a wrapped prompt?":
three cooperating agents triage a production incident — a vague alert parks
the task in `INPUT_REQUIRED`, the operator's answer resumes the *same task*,
the orchestrator delegates to a deterministic log-search agent and an
LLM-backed runbook agent over real A2A calls, progress streams live, the
incident report lands as an artifact, and a parked task can be cancelled.
Runs fully local with Qwen3-0.6B (a ~640 MB Apache-2.0 model, via llama-server or Ollama) or
with no model at all:

```bash
cargo run -p incident-response
```

### Agent Team (Full Dogfood)

A comprehensive 4-agent team that exercises every SDK feature — 81 base E2E tests (94 with all optional features: WebSocket, gRPC, Axum, SQLite, signing, and OTel) covering all four transports (JSON-RPC, REST, WebSocket, gRPC), streaming, push notifications, agent-to-agent orchestration, cancellation, concurrency stress, multi-tenancy, large payloads, metrics, SDK regression testing, batch JSON-RPC, auth rejection, extended/dynamic agent cards, HTTP caching, backpressure, agent card signing, Axum framework integration, and SQLite-backed stores:

```bash
cargo run -p agent-team

# With all optional features
cargo run -p agent-team --features grpc,websocket,axum,sqlite,signing,otel
```

### Echo Agent

A minimal example demonstrating both JSON-RPC and REST transports with synchronous and streaming modes:

```bash
cargo run -p echo-agent
```

### Multi-Language Agent Team

A Rust coordinator agent that delegates to worker agents written in Python, JavaScript, Go, and Java — proving cross-language A2A interoperability:

```bash
# Start the ITK worker agents first (see itk/README.md), then:
cargo run -p multi-lang-team
```

### AI Framework Integrations

Real LLM agents behind the A2A protocol — both pass the TCK 20/20 and run
against hosted providers or any local OpenAI-compatible server, with
honest failure semantics (provider errors fail the task; they are never
disguised as successful artifacts):

```bash
# rig AI framework (https://github.com/0xPlaygrounds/rig)
OPENAI_API_KEY=sk-... cargo run -p rig-a2a-agent

# genai multi-provider LLM client (https://crates.io/crates/genai)
GENAI_MODEL=gpt-4o-mini cargo run -p genai-a2a-agent
```

### Technology Compatibility Kit (TCK)

A standalone conformance test runner that validates any A2A server against
the protocol spec over the JSON-RPC and REST bindings (the gRPC and
WebSocket transports are covered by the agent-team E2E tests instead):

```bash
# Test a local server
cargo run -p a2a-tck -- --url http://localhost:8080 --binding jsonrpc

# Run the full cross-language ITK (requires Docker)
docker compose -f itk/docker-compose.yml up --build --abort-on-container-exit
```

## Architecture

```
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

## Testing

```bash
# Run the test suite (2,000+ tests with --all-features; CI runs nine feature combinations)
cargo test --workspace --all-features

# Run the end-to-end example
cargo run -p echo-agent

# Lint and format checks
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# Build documentation
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Run benchmarks (Criterion suites ×14 — transport, protocol,
# lifecycle, concurrency, cross-language, realistic, error paths, backpressure,
# data volume, memory, enterprise, production, advanced scenarios, and
# coordinator chain under fault — the last is the only agent-level one,
# see book/src/reference/benchmarks.md for caveats on how to read it)
cargo bench -p a2a-benchmarks

# Mutation testing (requires cargo-mutants)
cargo mutants --workspace

# Fuzz JSON deserialization (requires nightly)
cd fuzz && cargo +nightly fuzz run json_deser
```

## Project Status

All phases are complete. The SDK is production-ready with all 11 A2A methods, quad transport, HTTP caching, agent card signing, optional `tracing`, TLS support, enterprise hardening (body limits, health checks, task TTL/eviction, CORS, SSRF protection), and a hardened CI pipeline. See [`docs/implementation/plan.md`](docs/implementation/plan.md) for the full implementation roadmap and beyond-spec extensions.


## Stability

All crates follow [Semantic Versioning 2.0.0](https://semver.org/). During the `0.x` series, minor versions may include breaking changes as the API stabilizes. Protocol enums and key structs that can grow with the A2A specification are marked `#[non_exhaustive]` to allow forward-compatible additions in patch releases; the two deliberate exceptions are closed sets fixed by their underlying standards (`ApiKeyLocation` — OpenAPI's header/query/cookie — and `JsonRpcResponse` — JSON-RPC 2.0's result/error), which stay exhaustive so consumers can match them completely.

## Minimum Supported Rust Version

Rust **1.93** or later (stable).

## License

Apache-2.0 — see [LICENSE](LICENSE).
