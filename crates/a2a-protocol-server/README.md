<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-server

Server framework for the A2A protocol v1.0 -- build, serve, and scale AI agents.

Part of [a2a-rust](https://github.com/tomtom215/a2a-rust), an independent
(not official) A2A SDK written against specification release 1.0.1. Guide:
<https://a2a-rust.com> · API: <https://docs.rs/a2a-protocol-server>.

```toml
[dependencies]
a2a-protocol-server = "0.15"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## Overview

- All eleven A2A methods over JSON-RPC, HTTP+JSON and gRPC — the three bindings the official TCK grades — plus WebSocket, a custom binding (A2A §12) this project defines and grades with its in-repo runner. Results and gaps: [Project Status](https://github.com/tomtom215/a2a-rust#project-status)
- Built on hyper 1.x with tokio async runtime
- Pluggable dispatchers: JSON-RPC 2.0, REST, WebSocket, gRPC, Axum
- Pluggable storage: in-memory, SQLite, PostgreSQL
- SSE streaming for real-time task updates
- Multi-tenancy, rate limiting, interceptors, push notifications

## Quick Start

Every Rust block in this README is compiled as a doctest of the crate; the
two that open a listener are compiled but not run.

```rust,no_run
use std::sync::Arc;

use a2a_protocol_server::{
    EventEmitter, JsonRpcDispatcher, RequestHandlerBuilder, agent_executor, serve,
};
use a2a_protocol_types::{AgentCard, AgentInterface, AgentSkill, Part, TaskState};

struct MyAgent;

agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.artifact("result", vec![Part::text("Hello!")], None, Some(true)).await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card).build()?);
serve("0.0.0.0:3000", JsonRpcDispatcher::new(handler)).await?;
# Ok(())
# }
```

`serve` is the shortest path, not the production one: it serves every
connection with hyper's defaults, with no connection cap, no idle timeout,
and no way to stop short of dropping its future. For a deployment, bind a
`Server`, give it a `ServeConfig`, and hand it the signal that ends it;
in-flight work finishes (or is cancelled after `completion_grace`) before the
connections drain. `ServeConfig::default()` adds header-read and idle
timeouts but no connection cap; set one with `with_max_connections`:

```rust,no_run
# use std::sync::Arc;
# use a2a_protocol_server::{EventEmitter, JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
# use a2a_protocol_types::{AgentCard, AgentInterface, AgentSkill, TaskState};
# struct MyAgent;
# agent_executor!(MyAgent, |ctx, queue| async {
#     EventEmitter::new(ctx, queue).status(TaskState::Completed).await
# });
use a2a_protocol_server::{ServeConfig, Server};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card).build()?);
let report = Server::bind("0.0.0.0:3000")
    .await?
    .with_config(ServeConfig::default().with_max_connections(10_000))
    .serve_with_shutdown(JsonRpcDispatcher::new(handler), async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await;
println!("{report:?}");
# Ok(())
# }
```

## Architecture

```text
┌───────────────────────────────────────────────────┐
│                  Dispatchers                      │
│  JsonRpcDispatcher · RestDispatcher · A2aRouter   │
│  WebSocketDispatcher · GrpcDispatcher             │
├───────────────────────────────────────────────────┤
│               RequestHandler                      │
│  Interceptors · Rate Limiting · Multi-tenancy     │
├───────────────────────────────────────────────────┤
│              AgentExecutor (your code)            │
│  EventEmitter · EventQueueWriter                  │
├───────────────────────────────────────────────────┤
│               Storage Layer                       │
│  TaskStore · PushConfigStore · EventQueueManager  │
│  InMemory · SQLite · PostgreSQL                   │
└───────────────────────────────────────────────────┘
```

## Key Types

| Type | Purpose |
|------|---------|
| `AgentExecutor` | Trait -- implement your agent logic |
| `RequestHandler` | Protocol orchestrator (task lifecycle, events, push) |
| `RequestHandlerBuilder` | Fluent builder with stores, interceptors, card |
| `EventEmitter` | Helper for emitting status/artifact events |
| `JsonRpcDispatcher` | JSON-RPC 2.0 transport |
| `RestDispatcher` | REST transport |
| `A2aRouter` | Axum framework integration (feature-gated) |
| `TaskStore` | Trait -- pluggable task persistence |
| `InMemoryTaskStore` | Default in-memory store |
| `ServerInterceptor` | Trait -- `before` / `after` / `on_complete` (runs on every outcome) |
| `RateLimitInterceptor` | Fixed-window per-caller rate limiting |
| `serve()` | Shortest path: no limits, no shutdown |
| `Server` + `ServeConfig` | Production listener: timeouts, optional connection cap, TLS (`tls-rustls`), graceful shutdown, `ServeReport` |

## Features

`tracing` is the only feature on by default.

| Feature | Default | Purpose |
|---------|---------|---------|
| `signing` | No | Forwards `a2a-protocol-types/signing`; this crate itself neither signs nor verifies the card it serves |
| `tracing` | Yes | Structured logging via the `tracing` crate; `default-features = false` compiles it out |
| `tls-rustls` | No | HTTPS for the bundled push-notification sender, and TLS on `Server`'s own listener (`Server::with_tls`, `with_tls_pem`) |
| `sqlite` | No | SQLite-backed task and push-config stores |
| `postgres` | No | PostgreSQL-backed task and push-config stores |
| `websocket` | No | WebSocket transport |
| `grpc` | No | gRPC transport via tonic (`lf.a2a.v1.A2AService`) |
| `grpc-tls` | No | TLS on the gRPC listener; implies `grpc` |
| `otel` | Yes | `OtelMetrics` and the spans' OpenTelemetry bridge, through the OpenTelemetry API only (implies `tracing`; no exporter) |
| `otlp` | No | `otel::Telemetry`: OTLP export of traces, metrics and logs over gRPC or HTTP/protobuf, configured by `OTEL_*`, with a Langfuse preset; and `init_otlp_pipeline` (implies `otel`) |
| `conformance` | No | A harness that grades an `AgentExecutor` against the protocol's invariants |
| `axum` | No | Axum integration (`A2aRouter`) |
| `auth-jwt` | No | JWT bearer-token authentication (HS256/RS256/ES256, static or remote JWKS) |
| `encryption` | No | Encryption at rest for task content: `store::encrypted::EncryptedTaskStore`, AES-256-GCM over a key ring, or a cipher of your own |
| `audit` | No | Tamper-evident audit trail (`RequestHandlerBuilder::with_audit`, ADR 0015): calls, runs, task events and cancel requests in a SHA-256 hash chain per tenant, signed checkpoints, legal holds, a six-month retention floor; SQLite/PostgreSQL audit stores with those features |

## Agent Cards

Three serving strategies:

- `StaticAgentCardHandler` -- fixed agent card
- `DynamicAgentCardHandler` -- dynamically generated
- `HotReloadAgentCardHandler` -- auto-reload from file (SIGHUP or polling)

HTTP caching with ETag, Last-Modified, 304 Not Modified.

## Storage

Pluggable via `TaskStore` and `PushConfigStore` traits:

| Store | Feature | Use Case |
|-------|---------|----------|
| `InMemoryTaskStore` | (default) | Development, testing, single-instance |
| `SqliteTaskStore` | `sqlite` | Single-node production, edge |
| `PostgresTaskStore` | `postgres` | Multi-replica deployments; see the guide's [Running More Than One Replica](https://a2a-rust.com/deployment/horizontal-scaling.html) for what does and does not cross replicas |

All stores have tenant-aware variants for multi-tenancy.

## Multi-Tenancy

Tenant resolution strategies:

- `HeaderTenantResolver` -- from HTTP header
- `BearerTokenTenantResolver` -- from bearer token
- `PathSegmentTenantResolver` -- from URL path

Per-tenant limits: `PerTenantConfig` (a default `TenantLimits` plus
overrides) through `RequestHandlerBuilder::with_tenant_config`; its
`rate_limit_rps` takes effect only through
`RateLimitInterceptor::with_tenant_config`. Isolation needs the
`TenantAware*` stores, and a handler whose stores cannot isolate refuses a
request naming a tenant.

## Observability

- `Metrics` trait for custom metrics callbacks
- `otel::Telemetry` (feature: `otel`): one-call OTLP export of traces,
  metrics and logs, configured from `OTEL_*`, with a Langfuse preset
- `OtelMetrics` (feature: `otel`) for OTLP metrics alone
- `tracing` integration (feature: `tracing`)

## License

Apache-2.0
