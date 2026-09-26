# Project Structure

a2a-rust is organized as a Cargo workspace with four crates, each with a clear responsibility. Understanding this structure helps you choose the right dependency for your use case.

## Workspace Layout

```text
a2a-rust/
├── Cargo.toml              # Workspace root
├── crates/
│   ├── a2a-protocol-types/   # Wire types (serde, no I/O)
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── task.rs         # Task, TaskState, TaskStatus, TaskId
│   │       ├── message.rs      # Message, Part, PartContent, MessageRole
│   │       ├── artifact.rs     # Artifact, ArtifactId
│   │       ├── agent_card/     # AgentCard, AgentInterface, AgentSkill
│   │       ├── events.rs       # StreamResponse, status/artifact events
│   │       ├── params/         # MessageSendParams, TaskQueryParams, ...
│   │       ├── responses.rs    # SendMessageResponse, TaskListResponse
│   │       ├── jsonrpc.rs      # JSON-RPC 2.0 envelope types
│   │       ├── push.rs         # Push notification config types
│   │       ├── error.rs        # A2aError, ErrorCode, A2aResult
│   │       ├── security.rs     # Security schemes and requirements
│   │       ├── signing.rs      # Agent card signing (JWS/ES256)
│   │       └── extensions.rs   # Extension types
│   │
│   ├── a2a-protocol-client/  # HTTP client
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
│   │       │   ├── jsonrpc.rs      # JsonRpcTransport
│   │       │   ├── websocket.rs   # WebSocketTransport (feature-gated)
│   │       │   └── grpc.rs        # GrpcTransport (feature-gated)
│   │       ├── streaming/      # SSE parser, EventStream
│   │       │   ├── event_stream.rs # EventStream for consuming SSE
│   │       │   └── sse_parser/     # SSE frame parser
│   │       │       ├── mod.rs          # Re-exports
│   │       │       ├── types.rs        # SseFrame, SseParseError
│   │       │       └── parser.rs       # SseParser state machine
│   │       ├── methods/        # A2A client methods
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── send_message.rs # send_message, stream_message
│   │       │   ├── tasks.rs        # get_task, list_tasks, cancel_task, subscribe_to_task
│   │       │   ├── push_config.rs  # set/get/list/delete push configs
│   │       │   └── extended_card.rs # get_extended_agent_card
│   │       ├── auth.rs         # CredentialsStore, AuthInterceptor
│   │       ├── interceptor.rs  # CallInterceptor, InterceptorChain
│   │       ├── retry.rs        # RetryPolicy, RetryTransport
│   │       ├── testing/        # ScriptedPeer hostile test peer (feature-gated)
│   │       ├── error/          # ClientError, ClientResult
│   │       ├── config.rs       # ClientConfig
│   │       ├── discovery.rs    # Agent card discovery
│   │       └── tls.rs          # TLS configuration helpers
│   │
│   ├── a2a-protocol-server/  # Server framework
│   │   └── src/
│   │       ├── lib.rs          # Public re-exports
│   │       ├── handler/        # RequestHandler (core orchestration)
│   │       │   ├── mod.rs          # Struct definition, SendMessageResult
│   │       │   ├── limits/         # HandlerLimits config
│   │       │   ├── messaging/      # SendMessage / SendStreamingMessage
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
│   │       │   │       └── push_delivery/    # Push notification delivery
│   │       │   ├── shutdown/       # Graceful shutdown
│   │       │   └── helpers.rs      # Validation, context builders
│   │       ├── auth/           # Server-side authentication interceptors
│   │       ├── builder.rs      # RequestHandlerBuilder
│   │       ├── executor.rs     # AgentExecutor trait
│   │       ├── executor_helpers.rs # boxed_future, agent_executor!, EventEmitter
│   │       ├── dispatch/       # Protocol dispatchers
│   │       │   ├── mod.rs          # DispatchConfig, re-exports
│   │       │   ├── rest/           # REST dispatcher
│   │       │   │   ├── mod.rs          # RestDispatcher, route handlers
│   │       │   │   ├── response.rs     # HTTP response helpers
│   │       │   │   ├── error_response.rs # AIP-193 error bodies
│   │       │   │   └── query.rs        # Query/URL parsing utilities
│   │       │   ├── jsonrpc/        # JSON-RPC 2.0 dispatcher
│   │       │   │   ├── mod.rs          # JsonRpcDispatcher, dispatch logic
│   │       │   │   └── response.rs     # JSON-RPC response serialization
│   │       │   ├── axum_adapter.rs  # A2aRouter (axum feature-gated)
│   │       │   ├── websocket.rs    # WebSocketDispatcher (feature-gated)
│   │       │   ├── cors.rs         # CorsConfig
│   │       │   └── grpc/           # gRPC dispatcher (feature-gated)
│   │       │       ├── mod.rs          # Proto includes, re-exports
│   │       │       ├── config.rs       # GrpcConfig
│   │       │       ├── dispatcher.rs   # GrpcDispatcher, server setup
│   │       │       ├── native.rs       # lf.a2a.v1.A2AService implementation
│   │       │       ├── shutdown.rs     # GrpcDispatcher::serve_with_shutdown
│   │       │       └── helpers.rs      # Metadata extraction, error → gRPC status mapping
│   │       ├── store/          # Task persistence
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── task_store/     # TaskStore trait + in-memory impl
│   │       │   │   ├── mod.rs          # TaskStore trait, TaskStoreConfig
│   │       │   │   └── in_memory/      # InMemoryTaskStore
│   │       │   │       ├── mod.rs          # Core CRUD, TaskStore impl
│   │       │   │       └── eviction/       # TTL + capacity eviction
│   │       │   ├── sqlite_store/       # SqliteTaskStore (feature-gated)
│   │       │   ├── migration.rs        # Schema migration runner
│   │       │   ├── tenant_sqlite_store.rs # TenantAwareSqliteTaskStore
│   │       │   ├── postgres_store/     # PostgresTaskStore (feature-gated)
│   │       │   ├── pg_migration.rs     # PostgreSQL migration runner
│   │       │   ├── tenant_postgres_store/ # TenantAwarePostgresTaskStore
│   │       │   └── tenant/         # Multi-tenant isolation
│   │       │       ├── mod.rs          # Re-exports
│   │       │       ├── context.rs      # TenantContext (task-local)
│   │       │       └── store.rs        # TenantAwareInMemoryTaskStore
│   │       ├── push/           # PushConfigStore, PushSender
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── config_store.rs # InMemoryPushConfigStore
│   │       │   ├── sender.rs       # PushSender trait, HttpPushSender
│   │       │   ├── sqlite_config_store.rs      # SqlitePushConfigStore
│   │       │   ├── tenant_config_store.rs      # TenantAwareInMemoryPushConfigStore
│   │       │   ├── tenant_sqlite_config_store.rs # TenantAwareSqlitePushConfigStore
│   │       │   ├── postgres_config_store.rs    # PostgresPushConfigStore
│   │       │   └── tenant_postgres_config_store.rs # TenantAwarePostgresPushConfigStore
│   │       ├── streaming/      # Event streaming
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── sse.rs          # SSE response builder
│   │       │   └── event_queue/    # Event queue system
│   │       │       ├── mod.rs          # Traits, constants, constructors
│   │       │       ├── in_memory.rs    # Broadcast-backed queue impl
│   │       │       └── manager.rs      # EventQueueManager
│   │       ├── agent_card/     # Static/Dynamic card handlers
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── static_handler.rs  # StaticAgentCardHandler
│   │       │   ├── dynamic_handler.rs # DynamicAgentCardHandler, AgentCardProducer
│   │       │   ├── hot_reload.rs   # HotReloadAgentCardHandler
│   │       │   └── caching.rs      # ETag/Last-Modified caching
│   │       ├── otel/           # OpenTelemetry (feature-gated)
│   │       │   ├── mod.rs          # Re-exports
│   │       │   ├── pipeline.rs     # OTLP pipeline setup
│   │       │   └── builder.rs      # OtelMetrics builder
│   │       ├── call_context.rs # CallContext with HTTP headers
│   │       ├── conformance/    # AgentExecutor conformance harness (feature-gated)
│   │       ├── metrics/        # Metrics trait
│   │       ├── rate_limit/     # RateLimitInterceptor, RateLimitConfig
│   │       ├── serve/          # serve(), serve_with_addr(), Server, ServeConfig
│   │       ├── request_context.rs  # RequestContext
│   │       ├── interceptor/    # ServerInterceptor trait
│   │       ├── error/          # ServerError, ServerResult
│   │       ├── tenant_config/  # PerTenantConfig, TenantLimits
│   │       └── tenant_resolver.rs # TenantResolver trait + impls
│   │
│   └── a2a-protocol-sdk/      # Umbrella crate
│       └── src/
│           └── lib.rs          # Re-exports + prelude
│
├── examples/
│   ├── hello-agent/        # Smallest complete agent
│   ├── deploy-agent/       # The hello agent made deployable (health, SIGTERM, container)
│   ├── echo-agent/         # Every A2A method over all four bindings
│   ├── incident-response/  # Three cooperating agents: input-required, delegation, cancel
│   ├── agent-team/         # Comprehensive 4-agent dogfood suite (102 E2E tests by default, 87 bare)
│   ├── multi-lang-team/    # Multi-language team example
│   ├── rig-agent/          # Integration with the Rig framework
│   ├── genai-agent/        # Integration with the GenAI framework
│   ├── mcp-agent/          # Tools discovered from an MCP server
│   ├── mcp-bridge/         # A remote A2A agent exposed as an MCP server
│   ├── resilient-agent/
│   └── harness/            # Shared coverage matrix and sweep for the examples
│
├── tck/                    # Technology Compatibility Kit
│
├── docs/
│   └── adr/                # Architecture Decision Records
│
└── fuzz/                   # Fuzz testing targets
```

## Crate Dependencies

```text
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

```rust,no_run
use a2a_protocol_sdk::prelude::*;
```

This gives you:
- Core types: `Task`, `TaskState`, `TaskStatus`, `Message`, `MessageRole`, `Part`, `Artifact`, `ArtifactId`
- ID types: `TaskId`, `ContextId`, `MessageId`
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

No web framework is required by default — the dispatchers work directly with hyper. However, the optional `axum` feature provides an `A2aRouter` for projects that already use Axum.

## Next Steps

- **[Protocol Overview](../concepts/protocol-overview.md)** — Understand the A2A protocol model
- **[The AgentExecutor Trait](../building-agents/executor.md)** — The core of agent implementation
