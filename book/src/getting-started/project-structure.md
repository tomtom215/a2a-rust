# Project Structure

a2a-rust is organized as a Cargo workspace with four crates, each with a clear responsibility. Understanding this structure helps you choose the right dependency for your use case.

## Workspace Layout

```
a2a-rust/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── a2a-types/          # Wire types (serde, no I/O)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── task.rs         # Task, TaskState, TaskStatus, TaskId
│   │       ├── message.rs      # Message, Part, PartContent, MessageRole
│   │       ├── artifact.rs     # Artifact, ArtifactId
│   │       ├── agent_card.rs   # AgentCard, AgentInterface, AgentSkill
│   │       ├── events.rs       # StreamResponse, status/artifact events
│   │       ├── params.rs       # MessageSendParams, TaskQueryParams, ...
│   │       ├── responses.rs    # SendMessageResponse, TaskListResponse
│   │       ├── jsonrpc.rs      # JSON-RPC 2.0 envelope types
│   │       ├── push.rs         # Push notification config types
│   │       ├── error.rs        # A2aError, ErrorCode, A2aResult
│   │       ├── security.rs     # Security schemes and requirements
│   │       └── extensions.rs   # Extension and signing types
│   │
│   ├── a2a-client/         # HTTP client
│   │   └── src/
│   │       ├── lib.rs          # A2aClient, ClientBuilder
│   │       ├── transport/      # JsonRpcTransport, RestTransport
│   │       ├── streaming/      # SSE parser, EventStream
│   │       ├── methods/        # send_message, tasks, push_config
│   │       ├── auth.rs         # CredentialsStore, AuthInterceptor
│   │       └── interceptor.rs  # CallInterceptor, InterceptorChain
│   │
│   ├── a2a-server/         # Server framework
│   │   └── src/
│   │       ├── lib.rs          # Public re-exports
│   │       ├── handler.rs      # RequestHandler (core orchestration)
│   │       ├── builder.rs      # RequestHandlerBuilder
│   │       ├── executor.rs     # AgentExecutor trait
│   │       ├── dispatch/       # JsonRpcDispatcher, RestDispatcher
│   │       ├── store/          # TaskStore trait, InMemoryTaskStore
│   │       ├── push/           # PushConfigStore, PushSender
│   │       ├── streaming/      # EventQueueWriter, EventQueueManager
│   │       ├── agent_card/     # Static/Dynamic card handlers
│   │       ├── metrics.rs      # Metrics trait, NoopMetrics
│   │       └── request_context.rs  # RequestContext
│   │
│   └── a2a-sdk/            # Umbrella crate
│       └── src/
│           └── lib.rs          # Re-exports + prelude
│
├── examples/
│   └── echo-agent/         # Working example agent
│
├── docs/
│   └── adr/                # Architecture Decision Records
│
└── fuzz/                   # Fuzz testing targets
```

## Crate Dependencies

```
a2a-sdk
├── a2a-client
│   └── a2a-types
├── a2a-server
│   └── a2a-types
└── a2a-types
```

The dependency graph is intentionally shallow:

- **`a2a-types`** has no internal dependencies — just `serde` and `serde_json`
- **`a2a-client`** depends on `a2a-types` plus HTTP crates (`hyper`, `hyper-util`, `http-body-util`)
- **`a2a-server`** depends on `a2a-types` plus the same HTTP stack
- **`a2a-sdk`** depends on all three, adding nothing of its own

## Choosing Your Dependency

| Use Case | Crate |
|----------|-------|
| Just want the types (e.g., for a custom transport) | `a2a-types` |
| Building a client that calls remote agents | `a2a-client` |
| Building an agent (server) | `a2a-server` |
| Building both client and server | `a2a-sdk` |
| Quick prototyping / examples | `a2a-sdk` (use the `prelude`) |

## The Prelude

The SDK's `prelude` module exports the most commonly used types so you can get started with a single import:

```rust
use a2a_sdk::prelude::*;
```

This gives you:
- Core types: `Task`, `TaskState`, `TaskStatus`, `Message`, `Part`, `Artifact`
- ID types: `TaskId`, `ContextId`, `MessageId`
- Events: `StreamResponse`, `TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent`
- Agent card: `AgentCard`, `AgentInterface`, `AgentCapabilities`, `AgentSkill`
- Params: `MessageSendParams`, `TaskQueryParams`, `ListTasksParams`
- Responses: `SendMessageResponse`, `TaskListResponse`
- Errors: `A2aError`, `A2aResult`
- Client: `A2aClient`, `ClientBuilder`, `EventStream`
- Server: `AgentExecutor`, `RequestHandler`, `RequestHandlerBuilder`, `RequestContext`, `EventQueueWriter`
- Dispatchers: `JsonRpcDispatcher`, `RestDispatcher`

## External Dependencies

a2a-rust keeps its dependency tree lean:

| Dependency | Used For |
|-----------|----------|
| `serde` / `serde_json` | JSON serialization |
| `hyper` / `hyper-util` | HTTP/1.1 and HTTP/2 |
| `http-body-util` | HTTP body utilities |
| `tokio` | Async runtime |
| `uuid` | Task and message ID generation |
| `bytes` | Efficient byte buffers |

No web framework (axum, actix, warp) is required — the dispatchers work directly with hyper.

## Next Steps

- **[Protocol Overview](../concepts/protocol-overview.md)** — Understand the A2A protocol model
- **[The AgentExecutor Trait](../building-agents/executor.md)** — The core of agent implementation
