# API Quick Reference

A condensed overview of all public types, traits, and functions across the a2a-rust crates.

## Wire Types (`a2a-protocol-types`)

### Core Types

| Type | Description |
|------|-------------|
| `Task` | Unit of work with ID, status, history, artifacts |
| `TaskId` | Newtype wrapper for task identifiers |
| `TaskState` | Enum: Unspecified, Submitted, Working, InputRequired, AuthRequired, Completed, Failed, Canceled, Rejected |
| `TaskStatus` | State + optional message + timestamp |
| `TaskVersion` | Monotonically increasing version number |
| `ContextId` | Conversation context identifier |

### Messages

| Type | Description |
|------|-------------|
| `Message` | Structured payload with ID, role, parts |
| `MessageId` | Newtype wrapper for message identifiers |
| `MessageRole` | Enum: Unspecified, User, Agent |
| `Part` | Content unit: text, file, or data |
| `PartContent` | Enum: Text, File, Data |

### Artifacts

| Type | Description |
|------|-------------|
| `Artifact` | Result produced by an agent |
| `ArtifactId` | Newtype wrapper for artifact identifiers |

### Events

| Type | Description |
|------|-------------|
| `StreamResponse` | Enum: Task, Message, StatusUpdate, ArtifactUpdate |
| `TaskStatusUpdateEvent` | Status change notification |
| `TaskArtifactUpdateEvent` | Artifact delivery notification |

### Agent Card

| Type | Description |
|------|-------------|
| `AgentCard` | Root discovery document |
| `AgentInterface` | Transport endpoint descriptor |
| `AgentCapabilities` | Capability flags (streaming, push, extended card) |
| `AgentSkill` | Discrete agent capability |
| `AgentProvider` | Organization info |

### Parameters

| Type | Description |
|------|-------------|
| `MessageSendParams` | SendMessage / SendStreamingMessage input |
| `SendMessageConfiguration` | Output modes, history, push config |
| `TaskQueryParams` | GetTask input |
| `ListTasksParams` | ListTasks input with filters and pagination |
| `CancelTaskParams` | CancelTask input |
| `TaskIdParams` | SubscribeToTask input |
| `GetPushConfigParams` | GetTaskPushNotificationConfig input |
| `DeletePushConfigParams` | DeleteTaskPushNotificationConfig input |
| `ListPushConfigsParams` | ListTaskPushNotificationConfigs input |
| `GetExtendedAgentCardParams` | GetExtendedAgentCard input |

### Push Notifications

| Type | Description |
|------|-------------|
| `TaskPushNotificationConfig` | Webhook registration |
| `AuthenticationInfo` | Webhook auth credentials |

### Responses

| Type | Description |
|------|-------------|
| `SendMessageResponse` | Enum: Task or Message |
| `TaskListResponse` | Paginated task list |
| `ListPushConfigsResponse` | Paginated push config list |
| `AuthenticatedExtendedCardResponse` | Type alias for `AgentCard` |

### Errors

| Type | Description |
|------|-------------|
| `A2aError` | Protocol-level error |
| `ErrorCode` | Standard error codes |
| `A2aResult<T>` | Alias for `Result<T, A2aError>` |

### JSON-RPC

| Type | Description |
|------|-------------|
| `JsonRpcRequest` | JSON-RPC 2.0 request envelope |
| `JsonRpcError` | JSON-RPC error object |
| `JsonRpcVersion` | Version marker (`"2.0"`) |

## Client (`a2a-protocol-client`)

### Core Types

| Type | Description |
|------|-------------|
| `A2aClient` | Main client for calling remote agents |
| `ClientBuilder` | Fluent builder for client configuration |
| `EventStream` | Async SSE event stream |
| `RetryPolicy` | Configurable retry with exponential backoff |

### Client Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `send_message(params)` | `SendMessageResponse` | Synchronous send |
| `stream_message(params)` | `EventStream` | Streaming send |
| `get_task(params)` | `Task` | Retrieve task by ID |
| `list_tasks(params)` | `TaskListResponse` | Query tasks |
| `cancel_task(id)` | `Task` | Cancel a running task |
| `subscribe_to_task(id)` | `EventStream` | Re-subscribe to task events |
| `set_push_config(config)` | `TaskPushNotificationConfig` | Create push config |
| `get_push_config(task_id, id)` | `TaskPushNotificationConfig` | Get push config |
| `list_push_configs(params)` | `ListPushConfigsResponse` | List push configs |
| `delete_push_config(task_id, id)` | `()` | Delete push config |
| `get_extended_agent_card()` | `AuthenticatedExtendedCardResponse` | Get extended card |

### Interceptors

| Type | Description |
|------|-------------|
| `CallInterceptor` | Request/response hook trait |
| `InterceptorChain` | Ordered interceptor sequence |

### Transport

| Type | Description |
|------|-------------|
| `Transport` | Pluggable transport trait |
| `JsonRpcTransport` | JSON-RPC 2.0 transport |
| `RestTransport` | REST/HTTP transport |
| `WebSocketTransport` | WebSocket transport (`websocket` feature) |
| `GrpcTransport` | gRPC transport (`grpc` feature) |

## Server (`a2a-protocol-server`)

### Core Types

| Type | Description |
|------|-------------|
| `RequestHandler` | Central protocol orchestrator |
| `RequestHandlerBuilder` | Fluent builder for handler configuration |
| `RequestContext` | Per-execution context (task ID, message, etc.) |
| `CallContext` | Per-request metadata (request ID, headers, tenant) |
| `HandlerLimits` | Configurable validation limits |

### Traits

| Trait | Description |
|-------|-------------|
| `Metrics` | Pluggable metrics observer (requests, latency, errors) |
| `TenantResolver` | Extracts tenant from request context |

### Core Traits

| Trait | Description |
|-------|-------------|
| `AgentExecutor` | Agent logic entry point |
| `TaskStore` | Task persistence backend |
| `PushConfigStore` | Push config persistence |
| `PushSender` | Webhook delivery |
| `ServerInterceptor` | Server-side middleware |
| `AgentCardProducer` | Dynamic agent card generation |
| `Dispatcher` | HTTP dispatch trait (for `serve()`) |

### Dispatchers

| Type | Description |
|------|-------------|
| `JsonRpcDispatcher` | JSON-RPC 2.0 HTTP dispatcher (implements `Dispatcher`) |
| `RestDispatcher` | RESTful HTTP dispatcher (implements `Dispatcher`) |
| `WebSocketDispatcher` | WebSocket dispatcher (`websocket` feature) |
| `GrpcDispatcher` | gRPC dispatcher (`grpc` feature) |
| `A2aRouter` | Axum framework adapter (`axum` feature) |

### Server Startup

| Function | Description |
|----------|-------------|
| `serve(addr, dispatcher)` | Bind + accept loop (blocking) |
| `serve_with_addr(addr, dispatcher)` | Bind + spawn, returns `SocketAddr` |

### Built-in Implementations

| Type | Description |
|------|-------------|
| `InMemoryTaskStore` | In-memory task store with TTL |
| `InMemoryPushConfigStore` | In-memory push config store |
| `HttpPushSender` | HTTP webhook delivery with SSRF protection |
| `SqliteTaskStore` | SQLite task store (`sqlite` feature) |
| `SqlitePushConfigStore` | SQLite push config store (`sqlite` feature) |
| `TenantAwareInMemoryTaskStore` | Multi-tenant in-memory task store |
| `TenantAwareInMemoryPushConfigStore` | Multi-tenant in-memory push config store |
| `TenantAwareSqliteTaskStore` | Multi-tenant SQLite task store (`sqlite` feature) |
| `TenantAwareSqlitePushConfigStore` | Multi-tenant SQLite push config store (`sqlite` feature) |
| `PostgresTaskStore` | PostgreSQL task store (`postgres` feature) |
| `PostgresPushConfigStore` | PostgreSQL push config store (`postgres` feature) |
| `TenantAwarePostgresTaskStore` | Multi-tenant PostgreSQL task store (`postgres` feature) |
| `TenantAwarePostgresPushConfigStore` | Multi-tenant PostgreSQL push config store (`postgres` feature) |
| `PgMigrationRunner` | PostgreSQL migration runner (`postgres` feature) |
| `StaticAgentCardHandler` | Static agent card with HTTP caching |
| `DynamicAgentCardHandler` | Dynamic agent card with producer |
| `HotReloadAgentCardHandler` | Agent card with live reloading |
| `RateLimitInterceptor` | Per-caller rate limiting interceptor |
| `NoopMetrics` | No-op metrics implementation (default) |
| `OtelMetrics` | OpenTelemetry metrics (`otel` feature) |

### Streaming

| Type | Description |
|------|-------------|
| `EventEmitter` | Ergonomic event emission helper |
| `EventQueueWriter` | Write events to a stream |
| `EventQueueReader` | Read events from a stream |
| `EventQueueManager` | Manages stream lifecycle |
| `InMemoryQueueWriter` | Bounded channel writer |
| `InMemoryQueueReader` | Bounded channel reader |

### Configuration

| Type | Description |
|------|-------------|
| `CorsConfig` | Cross-origin policy |
| `TaskStoreConfig` | TTL and capacity for in-memory store |
| `ServerError` | Server-level error type |
| `ServerResult<T>` | Alias for `Result<T, ServerError>` |

## SDK (`a2a-protocol-sdk`)

### Modules

| Module | Re-exports |
|--------|-----------|
| `a2a_protocol_sdk::types` | All `a2a-protocol-types` exports |
| `a2a_protocol_sdk::client` | All `a2a-protocol-client` exports |
| `a2a_protocol_sdk::server` | All `a2a-protocol-server` exports |
| `a2a_protocol_sdk::prelude` | Most commonly used types |

### Prelude Contents

The prelude includes the most commonly used types from all three crates — see [Project Structure](../getting-started/project-structure.md#the-prelude) for the full list.

## Constructors Cheatsheet

```rust
// Task status
TaskStatus::new(TaskState::Working)
TaskStatus::with_timestamp(TaskState::Completed)

// Messages and parts
Part::text("hello")
Part::file_bytes(base64_string)
Part::file_uri("https://...")
Part::raw(base64_string)   // backward-compat alias for file_bytes
Part::url("https://...")   // backward-compat alias for file_uri

// Artifacts
Artifact::new("artifact-id", vec![Part::text("content")])

// IDs
TaskId::new("task-123")
ContextId::new("ctx-456")
MessageId::new("msg-789")

// Capabilities (non_exhaustive — use builder)
AgentCapabilities::none()
    .with_streaming(true)
    .with_push_notifications(false)

// Push configs
TaskPushNotificationConfig::new("task-id", "https://webhook.url")
```

## Next Steps

- **[Changelog](./changelog.md)** — Version history
- **[Configuration Reference](./configuration.md)** — All tunable parameters
