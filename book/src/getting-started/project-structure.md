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
│   │       ├── builder/        # ClientBuilder (fluent config)
│   │       │   ├── mod.rs            # Builder struct, configuration setters
│   │       │   └── transport_factory.rs  # build() / build_grpc() assembly
│   │       ├── transport/      # Transport trait + implementations
│   │       │   ├── mod.rs          # Transport trait, truncate_body
│   │       │   ├── rest/           # REST transport
│   │       │   │   ├── mod.rs          # RestTransport struct, constructors
│   │       │   │   ├── request.rs      # URI/request building, execution
│   │       │   │   ├── streaming.rs    # SSE streaming, body reader
│   │       │   │   ├── routing.rs      # Route definitions, method mapping
│   │       │   │   └── query.rs        # Query string building, encoding
│   │       │   └── jsonrpc.rs      # JsonRpcTransport
│   │       ├── streaming/      # SSE parser, EventStream
│   │       │   ├── event_stream.rs # EventStream for consuming SSE
│   │       │   └── sse_parser/     # SSE frame parser
│   │       │       ├── mod.rs          # Re-exports
│   │       │       ├── types.rs        # SseFrame, SseParseError
│   │       │       └── parser.rs       # SseParser state machine
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
│   │       │   ├── lifecycle/        # Task lifecycle handlers
│   │       │   │   ├── mod.rs            # Re-exports
│   │       │   │   ├── get_task.rs       # GetTask handler
│   │       │   │   ├── list_tasks.rs     # ListTasks handler
│   │       │   │   ├── cancel_task.rs    # CancelTask handler
│   │       │   │   ├── subscribe.rs      # SubscribeToTask handler
│   │       │   │   └── extended_card.rs  # GetExtendedAgentCard handler
│   │       │   ├── push_config.rs  # Push notification config CRUD
│   │       │   ├── event_processing/  # Event collection & push delivery
│   │       │   │   ├── mod.rs          # Re-exports
│   │       │   │   ├── sync_collector.rs   # Sync-mode event collection
│   │       │   │   └── background/       # Background event processor
│   │       │   │       ├── mod.rs            # Event loop orchestration
│   │       │   │       ├── state_machine.rs  # Event dispatch, state transitions
│   │       │   │       └── push_delivery.rs  # Push notification delivery
│   │       │   ├── shutdown.rs     # Graceful shutdown
│   │       │   └── helpers.rs      # Validation, context builders
│   │       ├── builder.rs      # RequestHandlerBuilder
│   │       ├── executor.rs     # AgentExecutor trait
│   │       ├── executor_helpers.rs # boxed_future, agent_executor!, EventEmitter
│   │       ├── dispatch/       # Protocol dispatchers
│   │       │   ├── mod.rs          # DispatchConfig, re-exports
│   │       │   ├── rest/           # REST dispatcher
│   │       │   │   ├── mod.rs          # RestDispatcher, route handlers
│   │       │   │   ├── response.rs     # HTTP response helpers
│   │       │   │   └── query.rs        # Query/URL parsing utilities
│   │       │   ├── jsonrpc/        # JSON-RPC 2.0 dispatcher
│   │       │   │   ├── mod.rs          # JsonRpcDispatcher, dispatch logic
│   │       │   │   └── response.rs     # JSON-RPC response serialization
│   │       │   └── grpc/           # gRPC dispatcher (feature-gated)
│   │       │       ├── mod.rs          # Proto includes, re-exports
│   │       │       ├── config.rs       # GrpcConfig
│   │       │       ├── dispatcher.rs   # GrpcDispatcher, server setup
│   │       │       ├── service.rs      # A2aService trait implementation
│   │       │       └── helpers.rs      # JSON codec, error mapping
│   │       ├── store/          # Task persistence
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── task_store/     # TaskStore trait + in-memory impl
│   │       │   │   ├── mod.rs          # TaskStore trait, TaskStoreConfig
│   │       │   │   └── in_memory/      # InMemoryTaskStore
│   │       │   │       ├── mod.rs          # Core CRUD, TaskStore impl
│   │       │   │       └── eviction.rs     # TTL + capacity eviction
│   │       │   └── tenant/         # Multi-tenant isolation
│   │       │       ├── mod.rs          # Re-exports
│   │       │       ├── context.rs      # TenantContext (task-local)
│   │       │       └── store.rs        # TenantAwareInMemoryTaskStore
│   │       ├── push/           # PushConfigStore, PushSender
│   │       ├── streaming/      # Event streaming
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── sse.rs          # SSE response builder
│   │       │   └── event_queue/    # Event queue system
│   │       │       ├── mod.rs          # Traits, constants, constructors
│   │       │       ├── in_memory.rs    # Broadcast-backed queue impl
│   │       │       └── manager.rs      # EventQueueManager
│   │       ├── agent_card/     # Static/Dynamic card handlers
│   │       ├── call_context.rs # CallContext with HTTP headers
│   │       ├── metrics.rs      # Metrics trait
│   │       ├── rate_limit.rs   # RateLimitInterceptor, RateLimitConfig
│   │       ├── serve.rs        # serve(), serve_with_addr() helpers
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
- Core types: `Task`, `TaskState`, `TaskStatus`, `Message`, `Part`, `Artifact`, `ArtifactId`
- ID types: `TaskId`, `ContextId`, `MessageId`, `MessageRole`
- Events: `StreamResponse`, `TaskStatusUpdateEvent`, `TaskArtifactUpdateEvent`
- Agent card: `AgentCard`, `AgentInterface`, `AgentCapabilities`, `AgentSkill`
- Params: `MessageSendParams`, `TaskQueryParams`
- Responses: `SendMessageResponse`, `TaskListResponse`
- Errors: `A2aError`, `A2aResult`, `ClientError`, `ClientResult`, `ServerError`, `ServerResult`
- Client: `A2aClient`, `ClientBuilder`, `EventStream`, `RetryPolicy`
- Server: `AgentExecutor`, `RequestHandler`, `RequestHandlerBuilder`, `RequestContext`, `EventQueueWriter`, `EventEmitter`, `Dispatcher`
- Dispatchers: `JsonRpcDispatcher`, `RestDispatcher`
- Utilities: `serve`, `serve_with_addr`, `RateLimitInterceptor`, `RateLimitConfig`

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
