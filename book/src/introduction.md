# Introduction

**a2a-rust** is a pure Rust implementation of the [A2A (Agent-to-Agent) protocol v1.0.0](https://google.github.io/A2A/) — an open standard for connecting AI agents over the network.

If you're building AI agents that need to talk to each other, discover capabilities, delegate tasks, and stream results — this library gives you the full protocol stack with zero `unsafe` code, compile-time type safety, and production-grade hardening.

## What is the A2A Protocol?

The Agent-to-Agent protocol defines how AI agents discover, communicate with, and delegate work to each other. Think of it as HTTP for AI agents: a shared language that lets any agent talk to any other, regardless of implementation.

The protocol defines:

- **Agent Cards** — Discovery documents describing what an agent can do
- **Tasks** — Units of work with well-defined lifecycle states
- **Messages** — Structured payloads carrying text, data, or binary content
- **Streaming** — Real-time progress via Server-Sent Events (SSE)
- **Push Notifications** — Webhook delivery for async results

## Why Rust?

Rust gives you performance, safety, and correctness without compromise:

- **Zero-cost abstractions** — Trait objects, generics, and async/await with no runtime overhead
- **No `unsafe`** — The entire codebase is free of unsafe code
- **Thread safety at compile time** — All public types implement `Send + Sync`
- **Exhaustive pattern matching** — The compiler catches missing protocol states
- **Production-ready** — Battle-tested HTTP via hyper, robust error handling, no panics in library code

## Architecture at a Glance

a2a-rust is organized as a Cargo workspace with four crates:

```
┌─────────────────────────────────────────────────┐
│                    a2a-sdk                       │
│          (umbrella re-exports + prelude)         │
├───────────────────────┬─────────────────────────┤
│      a2a-client       │       a2a-server        │
│     (HTTP client)     │   (agent framework)     │
├───────────────────────┴─────────────────────────┤
│                    a2a-types                     │
│          (wire types, serde, no I/O)            │
└─────────────────────────────────────────────────┘
```

| Crate | Purpose |
|-------|---------|
| **`a2a-types`** | All A2A wire types with serde serialization. Pure data — no I/O, no async. |
| **`a2a-client`** | HTTP client for calling remote A2A agents. Supports JSON-RPC and REST transports. |
| **`a2a-server`** | Server framework for *building* A2A agents. Pluggable stores, interceptors, and dispatchers. |
| **`a2a-sdk`** | Umbrella crate that re-exports everything with a convenient `prelude` module. |

## Key Features

- **Full v1.0 wire types** — Every A2A type with correct JSON serialization
- **Dual transport** — JSON-RPC 2.0 and REST, both client and server
- **SSE streaming** — Real-time `SendStreamingMessage` and `SubscribeToTask`
- **Push notifications** — Pluggable `PushSender` with SSRF protection
- **Agent card discovery** — Static and dynamic card handlers with HTTP caching (ETag, Last-Modified, 304)
- **Pluggable stores** — `TaskStore` and `PushConfigStore` traits for custom backends
- **Interceptor chains** — Client and server middleware for auth, logging, metrics
- **Task state machine** — Validated transitions per the A2A specification
- **Executor timeout** — Kills hung agent tasks automatically
- **CORS support** — Configurable cross-origin policies
- **No panics in library code** — All fallible operations return `Result`

## All 11 Protocol Methods

| Method | Description |
|--------|-------------|
| `SendMessage` | Synchronous message send; returns completed task |
| `SendStreamingMessage` | Streaming send with real-time SSE events |
| `GetTask` | Retrieve a task by ID |
| `ListTasks` | Query tasks with filtering and pagination |
| `CancelTask` | Request cancellation of a running task |
| `SubscribeToTask` | Re-subscribe to an existing task's event stream |
| `CreateTaskPushNotificationConfig` | Register a webhook for push delivery |
| `GetTaskPushNotificationConfig` | Retrieve a push config by ID |
| `ListTaskPushNotificationConfigs` | List all push configs for a task |
| `DeleteTaskPushNotificationConfig` | Remove a push config |
| `GetExtendedAgentCard` | Fetch authenticated agent card |

## What's Next?

- **[Installation](./getting-started/installation.md)** — Add a2a-rust to your project
- **[Quick Start](./getting-started/quick-start.md)** — See the protocol in action in 5 minutes
- **[Your First Agent](./getting-started/first-agent.md)** — Build an echo agent from scratch
- **[Protocol Overview](./concepts/protocol-overview.md)** — Understand the A2A protocol model
