<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# a2a-protocol-server

Server framework for the A2A protocol v1.0 -- build, serve, and scale AI agents.

Part of [a2a-rust](https://github.com/tomtom215/a2a-rust), an independent
(not official) A2A SDK written against specification release 1.0.1. Guide:
<https://a2a-rust.com> · API: <https://docs.rs/a2a-protocol-server>.

```toml
[dependencies]
a2a-protocol-server = "0.16"
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
    EventEmitter, JsonRpcDispatcher, RequestHandlerBuilder, Server, agent_executor,
    shutdown_signal,
};
use a2a_protocol_types::{AgentCard, AgentInterface, AgentSkill};

struct MyAgent;

// The answer as the task's artifact, then the terminal `Completed` status.
agent_executor!(MyAgent, |ctx, queue| async {
    EventEmitter::new(ctx, queue).complete_with_text("Hello!").await
});

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
    .with_description("An example agent")
    .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card).build()?);
let report = Server::bind("0.0.0.0:3000")
    .await?
    .serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown_signal())
    .await;
if let Some(why) = report.refused {
    return Err(why.into());
}
# Ok(())
# }
```

This runs under `ServeConfig::default()` (a 30 s header-read timeout, a 75 s
idle timeout and the loopback `Host` check, with no connection cap) until
SIGTERM or Ctrl-C; in-flight work finishes (or is cancelled after
`completion_grace`) before the connections drain. For a deployment, give the
`Server` a `ServeConfig` with a connection cap (`with_max_connections`). The
gRPC and WebSocket dispatchers take the same `ServeConfig` through
`with_serve_config`, and the axum `RestRouter` runs on `Server` itself. The
free `serve` and `serve_with_addr` are deprecated since 0.16.0: neither can be
stopped.

```rust,no_run
# use std::sync::Arc;
# use a2a_protocol_server::{EventEmitter, JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
# use a2a_protocol_types::{AgentCard, AgentInterface, AgentSkill, TaskState};
# struct MyAgent;
# agent_executor!(MyAgent, |ctx, queue| async {
#     EventEmitter::new(ctx, queue).status(TaskState::Completed).await
# });
use a2a_protocol_server::{ServeConfig, Server, shutdown_signal};

# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let card = AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]));
# let handler = Arc::new(RequestHandlerBuilder::new(MyAgent).with_agent_card(card).build()?);
let report = Server::bind("0.0.0.0:3000")
    .await?
    .with_config(ServeConfig::default().with_max_connections(10_000))
    .serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown_signal())
    .await;
println!("{report:?}");
# Ok(())
# }
```

## Architecture

```text
┌───────────────────────────────────────────────────┐
│                  Dispatchers                      │
│  JsonRpcDispatcher · RestDispatcher · RestRouter   │
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
| `RestRouter` | Axum framework integration (feature-gated) |
| `TaskStore` | Trait -- pluggable task persistence |
| `InMemoryTaskStore` | Default in-memory store |
| `ServerInterceptor` | Trait -- `before` / `after` / `on_complete` (runs on every outcome) |
| `RateLimitInterceptor` | Fixed-window per-caller rate limiting |
| `serve()` | Deprecated since 0.16.0: default timeouts, no connection cap, and nothing can stop it. Use `Server` |
| `Server` + `ServeConfig` | Production listener: timeouts, optional connection cap, TLS (`tls-rustls`), graceful shutdown, `ServeReport` |

## Features

`tracing` and `otel` are on by default; `otlp`, which exports, is not.

| Feature | Default | Purpose |
|---------|---------|---------|
| `signing` | No | Forwards `a2a-protocol-types/signing`; this crate itself neither signs nor verifies the card it serves |
| `tracing` | Yes | Structured logging via the `tracing` crate; `default-features = false` compiles it out |
| `tls-rustls` | Yes | HTTPS for the bundled push-notification sender, JWKS and OIDC fetches and the model client, and TLS on `Server`'s own listener (`Server::with_tls`, `with_tls_pem`); on by default since 0.16.0, `default-features = false` opts out |
| `sqlite` | No | SQLite-backed task and push-config stores |
| `postgres` | No | PostgreSQL-backed task and push-config stores |
| `websocket` | No | WebSocket transport |
| `grpc` | No | gRPC transport via tonic (`lf.a2a.v1.A2AService`) |
| `grpc-tls` | No | TLS on the gRPC listener; implies `grpc` |
| `otel` | Yes | `OtelMetrics` and the spans' OpenTelemetry bridge, through the OpenTelemetry API only (implies `tracing`; no exporter) |
| `otlp` | No | `otel::Telemetry`: OTLP export of traces, metrics and logs over gRPC or HTTP/protobuf, configured by `OTEL_*`, with a Langfuse preset; and `init_otlp_pipeline` (implies `otel`) |
| `otlp-json` | No | OTLP/HTTP with JSON bodies (`OtlpProtocol::HttpJson`, `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`); off by default because `opentelemetry-otlp`'s `http-json` also makes JSON the default of any exporter built without a protocol (implies `otlp`) |
| `otel-prometheus` | No | `OTEL_METRICS_EXPORTER=prometheus`: serve the metrics at `/metrics` for a Prometheus that scrapes, with no collector in between (`OTEL_EXPORTER_PROMETHEUS_HOST`, default `localhost`, and `_PORT`, default 9464); off by default because it adds `opentelemetry-prometheus` and a listener (implies `otlp`) |
| `conformance` | No | A harness that grades an `AgentExecutor` against the protocol's invariants |
| `axum` | No | Axum integration (`RestRouter`) |
| `tool-loop` | No | Tool calling inside a task (ADR 0022): `tool_loop::ToolLoop`, `ToolLoopExecutor`, and `OpenAiChatModel` for the chat-completions API; each tool call carries an idempotency key and a repeated call runs once (ADR 0026); HTTPS to the model needs `tls-rustls` |
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
- `otel::Telemetry` (feature: `otlp`): one-call OTLP export of traces,
  metrics and logs, configured from `OTEL_*`, with a Langfuse preset
- `OtelMetrics` (feature: `otel`): records through the OpenTelemetry API to
  whatever meter provider the process installs (`Telemetry` installs one that
  exports)
- `tracing` integration (feature: `tracing`)

## License

Apache-2.0
