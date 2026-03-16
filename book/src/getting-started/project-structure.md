# Project Structure

a2a-rust is organized as a Cargo workspace with four crates, each with a clear responsibility. Understanding this structure helps you choose the right dependency for your use case.

## Workspace Layout

```
a2a-rust/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── a2a-protocol-types/          # Wire types (serde, no I/O)
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
│   ├── a2a-protocol-client/         # HTTP client
│   │   └── src/
│   │       ├── lib.rs          # A2aClient, ClientBuilder
│   │       ├── transport/      # JsonRpcTransport, RestTransport
│   │       ├── streaming/      # SSE parser, EventStream
│   │       ├── methods/        # send_message, tasks, push_config
│   │       ├── auth.rs         # CredentialsStore, AuthInterceptor
│   │       └── interceptor.rs  # CallInterceptor, InterceptorChain
│   │
│   ├── a2a-protocol-server/         # Server framework
│   │   └── src/
│   │       ├── lib.rs          # Public re-exports
│   │       ├── handler/        # RequestHandler (core orchestration)
│   │       │   ├── mod.rs          # Struct definition, SendMessageResult
│   │       │   ├── limits.rs       # HandlerLimits config
│   │       │   ├── messaging.rs    # SendMessage / SendStreamingMessage
│   │       │   ├── lifecycle.rs    # GetTask, ListTasks, CancelTask, etc.
│   │       │   ├── push_config.rs  # Push notification config CRUD
│   │       │   ├── event_processing.rs  # Event collection, push delivery
│   │       │   ├── shutdown.rs     # Graceful shutdown
│   │       │   └── helpers.rs      # Validation, context builders
│   │       ├── builder.rs      # RequestHandlerBuilder
│   │       ├── executor.rs     # AgentExecutor trait
│   │       ├── dispatch/       # JsonRpcDispatcher, RestDispatcher
│   │       ├── store/          # TaskStore trait, InMemoryTaskStore
│   │       ├── push/           # PushConfigStore, PushSender
│   │       ├── streaming/      # EventQueueWriter, EventQueueManager
│   │       ├── agent_card/     # Static/Dynamic card handlers
│   │       ├── call_context.rs # CallContext with HTTP headers
│   │       ├── metrics.rs      # Metrics trait (incl. on_latency)
│   │       └── request_context.rs  # RequestContext
│   │
│   └── a2a-protocol-sdk/            # Umbrella crate
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
a2a-protocol-sdk
├── a2a-protocol-client
│   └── a2a-protocol-types
├── a2a-protocol-server
│   └── a2a-protocol-types
└── a2a-protocol-types
```

The dependency graph is intentionally shallow:

- **`a2a-protocol-types`** has no internal dependencies — just `serde` and `serde_json`
- **`a2a-protocol-client`** depends on `a2a-protocol-types` plus HTTP crates (`hyper`, `hyper-util`, `http-body-util`)
- **`a2a-protocol-server`** depends on `a2a-protocol-types` plus the same HTTP stack
- **`a2a-protocol-sdk`** depends on all three, adding nothing of its own

## Choosing Your Dependency

| Use Case | Crate |
|----------|-------|
| Just want the types (e.g., for a custom transport) | `a2a-protocol-types` |
| Building a client that calls remote agents | `a2a-protocol-client` |
| Building an agent (server) | `a2a-protocol-server` |
| Building both client and server | `a2a-protocol-sdk` |
| Quick prototyping / examples | `a2a-protocol-sdk` (use the `prelude`) |

## The Prelude

The SDK's `prelude` module exports the most commonly used types so you can get started with a single import:

```rust
use a2a_protocol_sdk::prelude::*;
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
