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

```rust
use std::sync::Arc;
use a2a_protocol_server::prelude::*; // via a2a-protocol-sdk

struct MyAgent;

agent_executor!(MyAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.artifact("result", vec![Part::text("Hello!")], None, Some(true)).await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

let handler = Arc::new(
    RequestHandlerBuilder::new(MyAgent)
        .with_agent_card(card)
        .build()?,
);
serve("0.0.0.0:3000", JsonRpcDispatcher::new(handler)).await?;
```

## Architecture

```
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

| Feature | Purpose |
|---------|---------|
| `signing` | Agent card signing verification |
| `tracing` | Structured logging via tracing crate |
| `sqlite` | SQLite-backed task and push config stores |
| `postgres` | PostgreSQL-backed stores |
| `websocket` | WebSocket transport |
| `grpc` | gRPC transport via tonic |
| `otel` | OpenTelemetry OTLP metrics export |
| `axum` | Axum framework integration |

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
