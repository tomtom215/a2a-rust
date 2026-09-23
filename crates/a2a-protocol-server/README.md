<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-server

Server framework for the A2A protocol v1.0 -- build, serve, and scale AI agents.

## Overview

- Complete server framework for building A2A-compliant agents
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
use a2a_protocol_types::{AgentCard, AgentInterface, Part, TaskState};

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
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"));
let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card).build()?);
serve("0.0.0.0:3000", JsonRpcDispatcher::new(handler)).await?;
# Ok(())
# }
```

`serve` is the shortest path, not the production one: it serves every
connection with hyper's defaults, with no connection cap, no idle timeout,
and no way to stop short of dropping its future. For a deployment, bind a `Server`, give it a `ServeConfig`, and hand it the signal
that ends it; in-flight work finishes (or is cancelled after
`completion_grace`) before the connections drain:

```rust,no_run
# use std::sync::Arc;
# use a2a_protocol_server::{EventEmitter, JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
# use a2a_protocol_types::{AgentCard, AgentInterface, TaskState};
# struct MyAgent;
# agent_executor!(MyAgent, |ctx, queue| async {
#     EventEmitter::new(ctx, queue).status(TaskState::Completed).await
# });
use a2a_protocol_server::{ServeConfig, Server};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"));
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card).build()?);
let report = Server::bind("0.0.0.0:3000")
    .await?
    .with_config(ServeConfig::default())
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
┌─────────────────────────────────────────────────┐
│                  Dispatchers                      │
│  JsonRpcDispatcher · RestDispatcher · A2aRouter   │
│  WebSocketDispatcher · GrpcDispatcher             │
├─────────────────────────────────────────────────┤
│               RequestHandler                      │
│  Interceptors · Rate Limiting · Multi-tenancy     │
├─────────────────────────────────────────────────┤
│              AgentExecutor (your code)            │
│  EventEmitter · EventQueueWriter                  │
├─────────────────────────────────────────────────┤
│               Storage Layer                       │
│  TaskStore · PushConfigStore · EventQueueManager  │
│  InMemory · SQLite · PostgreSQL                   │
└─────────────────────────────────────────────────┘
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
| `ServerInterceptor` | Trait -- before/after middleware hooks |
| `RateLimitInterceptor` | Fixed-window per-caller rate limiting |
| `serve()` | One-liner HTTP server startup |

## Features

`tracing` is the only feature on by default.

| Feature | Default | Purpose |
|---------|---------|---------|
| `signing` | No | Forwards `a2a-protocol-types/signing`; this crate itself neither signs nor verifies the card it serves |
| `tracing` | Yes | Structured logging via the `tracing` crate; `default-features = false` compiles it out |
| `tls-rustls` | No | HTTPS delivery for the bundled push-notification sender |
| `sqlite` | No | SQLite-backed task and push-config stores |
| `postgres` | No | PostgreSQL-backed task and push-config stores |
| `websocket` | No | WebSocket transport |
| `grpc` | No | gRPC transport via tonic (`lf.a2a.v1.A2AService`) |
| `grpc-tls` | No | TLS on the gRPC listener; implies `grpc` |
| `otel` | No | OpenTelemetry OTLP export of the metrics catalogue (metrics only; no traces) |
| `conformance` | No | A harness that grades an `AgentExecutor` against the protocol's invariants |
| `axum` | No | Axum integration (`A2aRouter`) |
| `auth-jwt` | No | JWT bearer-token authentication (HS256/RS256/ES256, static or remote JWKS) |

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
| `PostgresTaskStore` | `postgres` | Multi-node production |

All stores have tenant-aware variants for multi-tenancy.

## Multi-Tenancy

Tenant resolution strategies:

- `HeaderTenantResolver` -- from HTTP header
- `BearerTokenTenantResolver` -- from bearer token
- `PathSegmentTenantResolver` -- from URL path

Per-tenant configuration with `TenantLimits`.

## Observability

- `Metrics` trait for custom metrics callbacks
- `OtelMetrics` (feature: `otel`) for native OTLP export
- `tracing` integration (feature: `tracing`)

## License

Apache-2.0
