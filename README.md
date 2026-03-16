<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. -->

# a2a-rust

![CI](https://github.com/tomtom215/a2a-rust/actions/workflows/ci.yml/badge.svg)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-1.93%2B-orange)

Pure Rust implementation of the [A2A (Agent-to-Agent) protocol](https://google.github.io/A2A/) v1.0.0.

Build, connect, and orchestrate AI agents using a type-safe, async-first SDK with both JSON-RPC 2.0 and REST transport bindings.

## Motivation

The A2A protocol — originally developed by Google and [donated to the Linux Foundation](https://developers.googleblog.com/en/google-cloud-donates-a2a-to-linux-foundation/) in June 2025 — provides a vendor-neutral standard for AI agent interoperability. The [official SDKs](https://a2a-protocol.org/latest/sdk/) cover Python, Go, Java, JavaScript, and C#/.NET, but there is no official Rust implementation. The [community samples](https://github.com/a2aproject/a2a-samples/tree/main/samples) follow the same pattern.

Community Rust efforts exist but target older protocol versions:

- [a2a-rs](https://github.com/EmilLindfors/a2a-rs) — active, full v0.3.0 coverage, hexagonal architecture (not yet v1.0)
- [A2A](https://github.com/robert-at-pretension-io/A2A) — testing framework and validator (GPL-3.0)

This project aims to be the first **v1.0.0-compliant** Rust SDK for A2A. We intend to contribute this work to the [A2A project](https://github.com/a2aproject) under the Linux Foundation so that Rust has first-class support alongside the other official SDKs.

## Features

- **Full A2A v1.0.0 wire types** — every struct, enum, and field from the specification with correct serde annotations
- **Triple transport** — JSON-RPC 2.0, REST, and WebSocket dispatchers, both client and server
- **SSE streaming** — real-time `SendStreamingMessage` and `SubscribeToTask` with broadcast-based multi-subscriber event streams
- **Push notifications** — pluggable `PushSender` trait with HTTP webhook implementation
- **Agent card discovery** — `/.well-known/agent.json` serving and client-side resolution
- **Pluggable stores** — `TaskStore` and `PushConfigStore` traits with in-memory defaults, SQLite reference implementations (`sqlite` feature flag), and tenant-aware variants for multi-tenancy
- **Multi-tenancy** — `TenantAwareInMemoryTaskStore` and `TenantAwareSqliteTaskStore` provide full tenant isolation via `tokio::task_local!` or `tenant_id` column partitioning
- **Executor ergonomics** — `boxed_future` helper, `agent_executor!` macro, and `EventEmitter` eliminate `Pin<Box<dyn Future>>` and event-emission boilerplate
- **Interceptors** — client-side `CallInterceptor` and server-side `ServerInterceptor` chains for auth, logging, etc.
- **HTTP caching** — `ETag`, `Last-Modified`, `304 Not Modified` for agent card discovery
- **Agent card signing** — JWS/ES256 with RFC 8785 JSON canonicalization (feature-gated)
- **Optional tracing** — structured logging via `tracing` crate, zero cost when disabled
- **TLS support** — HTTPS via `rustls`, no OpenSSL system dependency (feature-gated)
- **State transition validation** — `TaskState::can_transition_to()` enforces valid state machine transitions at the handler level
- **Executor timeout** — configurable via `RequestHandlerBuilder::with_executor_timeout()` to kill hung executors
- **CORS support** — `CorsConfig` for browser-based A2A clients with preflight handling
- **Graceful shutdown** — `RequestHandler::shutdown()` cancels all tokens and destroys queues
- **Enterprise hardening** — request body size limits, Content-Type validation, path traversal protection (including percent-encoded bypass), query string length limits, health/readiness endpoints
- **Task store management** — configurable TTL-based eviction, capacity limits, amortized eviction (every 64 writes), and cursor-based pagination via `TaskStoreConfig`
- **Security** — SSRF protection for push webhooks, header injection prevention, SSE memory limits, cancellation token map bounds with stale cleanup
- **Server startup helper** — `serve()` and `serve_with_addr()` reduce the ~25-line hyper boilerplate to a single call
- **Client retry** — configurable `RetryPolicy` with exponential backoff for transient failures (connection errors, timeouts, 429/502/503/504)
- **Request ID propagation** — `CallContext::request_id` auto-extracted from `X-Request-ID` header for distributed tracing
- **Rate limiting** — built-in `RateLimitInterceptor` with fixed-window per-caller limiting; plug in via `ServerInterceptor` chain
- **Task store metrics** — `TaskStore::count()` for monitoring and capacity management
- **WebSocket transport** — persistent bidirectional communication via `tokio-tungstenite` (`websocket` feature flag), JSON-RPC 2.0 over WebSocket text frames with native streaming
- **Zero framework lock-in** — built on raw `hyper` 1.x; bring your own web framework
- **No `unsafe`** — `#![deny(unsafe_op_in_unsafe_fn)]` in every crate

## Crate Structure

| Crate | Purpose | When to Use |
|---|---|---|
| [`a2a-protocol-types`](crates/a2a-types) | All A2A wire types — `serde` only, no I/O | You need types without the HTTP stack |
| [`a2a-protocol-client`](crates/a2a-client) | HTTP client for A2A requests | Building an orchestrator, gateway, or test harness |
| [`a2a-protocol-server`](crates/a2a-server) | Server framework for A2A agents | Building an agent that handles A2A requests |
| [`a2a-protocol-sdk`](crates/a2a-sdk) | Umbrella re-export + prelude | Quick-start / full-stack usage |

`a2a-protocol-client` and `a2a-protocol-server` are **siblings** — neither depends on the other. Use only what you need.

## Quick Start

### Add the dependency

```toml
[dependencies]
a2a-protocol-sdk = "0.2"
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
    }
}
```

## Examples

### Agent Team (Full Dogfood)

A comprehensive 4-agent team that exercises every SDK feature — 55 E2E tests covering all three transports (JSON-RPC, REST, WebSocket), streaming, push notifications, agent-to-agent orchestration, cancellation, concurrency stress, multi-tenancy, large payloads, metrics, and SDK regression testing:

```bash
cargo run -p agent-team
```

### Echo Agent

A minimal example demonstrating both JSON-RPC and REST transports with synchronous and streaming modes:

```bash
cargo run -p echo-agent
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
│  RequestHandler · AgentExecutor · Client   │
└─────────────────────┬──────────────────────┘
                      │
┌─────────────────────▼──────────────────────┐
│  Transport Layer                           │
│  JsonRpcDispatcher · RestDispatcher        │
│  WebSocketDispatcher (feature-gated)       │
│  JsonRpcTransport · RestTransport          │
│  WebSocketTransport (feature-gated)        │
└─────────────────────┬──────────────────────┘
                      │
┌─────────────────────▼──────────────────────┐
│  hyper 1.x · HTTP/1.1 + HTTP/2             │
└────────────────────────────────────────────┘
```

The server uses a 3-layer architecture:
1. **You implement `AgentExecutor`** — your agent logic, produces events via `EventQueueWriter`
2. **`RequestHandler` orchestrates** — manages tasks, stores, push notifications, interceptors
3. **Dispatchers handle HTTP** — `JsonRpcDispatcher` (JSON-RPC 2.0), `RestDispatcher` (REST), and `WebSocketDispatcher` (WebSocket) wire hyper to the handler

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
# Run all tests (600+ tests across 4 crates)
cargo test --workspace

# Run the end-to-end example
cargo run -p echo-agent

# Lint and format checks
cargo clippy --workspace --all-targets
cargo fmt --all -- --check

# Build documentation
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Run benchmarks (task store, event queue)
cargo bench -p a2a-protocol-server

# Fuzz JSON deserialization (requires nightly)
cd fuzz && cargo +nightly fuzz run json_deser
```

## Project Status

All phases are complete. The SDK is production-ready with all 11 A2A methods, dual transport, HTTP caching, agent card signing, optional `tracing`, TLS support, enterprise hardening (body limits, health checks, task TTL/eviction, CORS, SSRF protection), and a hardened CI pipeline. See [`docs/implementation/plan.md`](docs/implementation/plan.md) for the full roadmap and [`docs/ROADMAP.md`](docs/ROADMAP.md) for planned beyond-spec extensions.

| Phase | Status |
|---|---|
| 0. Project Foundation | ✅ Complete |
| 1. Protocol Types (`a2a-protocol-types`) | ✅ Complete |
| 2. HTTP Client (`a2a-protocol-client`) | ✅ Complete |
| 3. Server Framework (`a2a-protocol-server`) | ✅ Complete |
| 4. v1.0 Protocol Upgrade | ✅ Complete |
| 5. Server Tests & Bug Fixes | ✅ Complete |
| 6. Umbrella Crate & Examples | ✅ Complete |
| 7. v1.0 Spec Compliance Gaps | ✅ Complete |
| 7.5 Spec Compliance Fixes | ✅ Complete |
| 8. Caching, Signing & Release | ✅ Complete |
| 9. Production Hardening | ✅ Complete |

## Stability

All crates follow [Semantic Versioning 2.0.0](https://semver.org/). During the `0.x` series, minor versions may include breaking changes as the API stabilizes. Protocol enums are marked `#[non_exhaustive]` to allow forward-compatible additions in patch releases.

## Minimum Supported Rust Version

Rust **1.93** or later (stable).

## License

Apache-2.0 — see [LICENSE](LICENSE).
