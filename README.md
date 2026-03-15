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
- **Dual transport** — JSON-RPC 2.0 and REST dispatchers, both client and server
- **SSE streaming** — real-time `SendStreamingMessage` and `SubscribeToTask` with async event streams
- **Push notifications** — pluggable `PushSender` trait with HTTP webhook implementation
- **Agent card discovery** — `/.well-known/agent.json` serving and client-side resolution
- **Pluggable stores** — `TaskStore` and `PushConfigStore` traits with in-memory defaults
- **Interceptors** — client-side `CallInterceptor` and server-side `ServerInterceptor` chains for auth, logging, etc.
- **HTTP caching** — `ETag`, `Last-Modified`, `304 Not Modified` for agent card discovery
- **Agent card signing** — JWS/ES256 with RFC 8785 JSON canonicalization (feature-gated)
- **Optional tracing** — structured logging via `tracing` crate, zero cost when disabled
- **TLS support** — HTTPS via `rustls`, no OpenSSL system dependency (feature-gated)
- **Zero framework lock-in** — built on raw `hyper` 1.x; bring your own web framework
- **No `unsafe`** — `#![deny(unsafe_op_in_unsafe_fn)]` in every crate

## Crate Structure

| Crate | Purpose | When to Use |
|---|---|---|
| [`a2a-types`](crates/a2a-types) | All A2A wire types — `serde` only, no I/O | You need types without the HTTP stack |
| [`a2a-client`](crates/a2a-client) | HTTP client for A2A requests | Building an orchestrator, gateway, or test harness |
| [`a2a-server`](crates/a2a-server) | Server framework for A2A agents | Building an agent that handles A2A requests |
| [`a2a-sdk`](crates/a2a-sdk) | Umbrella re-export + prelude | Quick-start / full-stack usage |

`a2a-client` and `a2a-server` are **siblings** — neither depends on the other. Use only what you need.

## Quick Start

### Add the dependency

```toml
[dependencies]
a2a-sdk = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

### Implement an agent

```rust
use std::future::Future;
use std::pin::Pin;
use a2a_sdk::prelude::*;

struct MyAgent;

impl AgentExecutor for MyAgent {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Transition to Working
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ctx.context_id.clone(),
                status: TaskStatus::new(TaskState::Working),
                metadata: None,
            })).await?;

            // Produce an artifact
            queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ctx.context_id.clone(),
                artifact: Artifact::new("result", vec![Part::text("Hello from my agent!")]),
                append: None,
                last_chunk: Some(true),
                metadata: None,
            })).await?;

            // Mark completed
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ctx.context_id.clone(),
                status: TaskStatus::new(TaskState::Completed),
                metadata: None,
            })).await?;

            Ok(())
        })
    }
}
```

### Start a server

```rust
use std::sync::Arc;
use a2a_sdk::prelude::*;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyAgent)
        .with_agent_card(agent_card)
        .build()
        .expect("build handler"),
);

// JSON-RPC transport
let jsonrpc = Arc::new(JsonRpcDispatcher::new(handler.clone()));

// REST transport
let rest = Arc::new(RestDispatcher::new(handler));
```

### Use the client

```rust
use a2a_sdk::prelude::*;

let client = ClientBuilder::new("http://localhost:8080")
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

### Echo Agent

A full-stack example demonstrating both JSON-RPC and REST transports with synchronous and streaming modes:

```bash
cargo run -p echo-agent
```

This starts servers on random ports and runs 5 demos:
1. Synchronous `SendMessage` via JSON-RPC
2. Streaming `SendStreamingMessage` via JSON-RPC
3. Synchronous `SendMessage` via REST
4. Streaming `SendStreamingMessage` via REST
5. `GetTask` retrieval

## Architecture

```
┌───────────────────────────────────────────────┐
│                 User Code                     │
│  (implements AgentExecutor or uses Client)    │
└──────────────────────┬────────────────────────┘
                       │
┌──────────────────────▼────────────────────────┐
│           a2a-server / a2a-client             │
│   RequestHandler · AgentExecutor · Client     │
└──────────────────────┬────────────────────────┘
                       │
┌──────────────────────▼────────────────────────┐
│              Transport Layer                  │
│   JsonRpcDispatcher · RestDispatcher          │
│   JsonRpcTransport  · RestTransport           │
└──────────────────────┬────────────────────────┘
                       │
┌──────────────────────▼────────────────────────┐
│                hyper 1.x                      │
└───────────────────────────────────────────────┘
```

The server uses a 3-layer architecture:
1. **You implement `AgentExecutor`** — your agent logic, produces events via `EventQueueWriter`
2. **`RequestHandler` orchestrates** — manages tasks, stores, push notifications, interceptors
3. **Dispatchers handle HTTP** — `JsonRpcDispatcher` (JSON-RPC 2.0) and `RestDispatcher` (REST) wire hyper to the handler

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
# Run all tests (225 tests across 4 crates)
cargo test --workspace

# Run the end-to-end example
cargo run -p echo-agent

# Lint and format checks
cargo clippy --workspace --all-targets
cargo fmt --all -- --check

# Build documentation
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Project Status

All phases are complete. The SDK is production-ready with all 11 A2A methods, dual transport, HTTP caching, agent card signing, optional `tracing`, TLS support, and a hardened CI pipeline. See [`docs/implementation/plan.md`](docs/implementation/plan.md) for the full roadmap.

| Phase | Status |
|---|---|
| 0. Project Foundation | ✅ Complete |
| 1. Protocol Types (`a2a-types`) | ✅ Complete |
| 2. HTTP Client (`a2a-client`) | ✅ Complete |
| 3. Server Framework (`a2a-server`) | ✅ Complete |
| 4. v1.0 Protocol Upgrade | ✅ Complete |
| 5. Server Tests & Bug Fixes | ✅ Complete |
| 6. Umbrella Crate & Examples | ✅ Complete |
| 7. v1.0 Spec Compliance Gaps | ✅ Complete |
| 7.5 Spec Compliance Fixes | ✅ Complete |
| 8. Caching, Signing & Release | ✅ Complete |
| 9. Production Hardening | ✅ Complete |

## Minimum Supported Rust Version

Rust **1.93** or later (stable).

## License

Apache-2.0 — see [LICENSE](LICENSE).
