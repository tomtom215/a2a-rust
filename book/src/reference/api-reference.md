# API Quick Reference

A condensed overview of the public types, traits, and functions across the
a2a-rust crates.

This page is a **curated selection**, kept short enough to scan. For the
exhaustive, always-current listing — every item, every signature, generated
from the code on each deploy — see the [generated API documentation](/api/).

Every name below is checked against the crates by
`scripts/check_api_reference.py` in CI, so a type that gets renamed cannot leave
a stale entry here. The same script checks the other direction at the crate
roots: every item a crate re-exports from its `lib.rs` — the names you reach as
`use a2a_protocol_types::X` — must have a row on this page. Deeper module items
are rustdoc's job.

## Wire Types (`a2a-protocol-types`)

### Modules

| Module | Contents |
|--------|----------|
| `agent_card` | Agent card and capability discovery types |
| `artifact` | Artifact types for the A2A protocol |
| `error` | A2A protocol error types |
| `events` | Server-sent event types for A2A streaming |
| `extensions` | Agent extension and card-signature types |
| `jsonrpc` | JSON-RPC 2.0 envelope types |
| `message` | Message types for the A2A protocol |
| `method` | The A2A v1.0 service methods, mirrored from the ratified specification |
| `params` | JSON-RPC method parameter types |
| `proto` | Canonical A2A protobuf message types (`lf.a2a.v1`) and conversions (`proto` feature) |
| `push` | Push notification configuration types |
| `responses` | RPC method response types |
| `security` | Security scheme types for A2A agent authentication |
| `serde_helpers` | Serialization helpers for reducing allocation overhead |
| `signing` | Agent card signing and verification (spec §10) (`signing` feature) |
| `task` | Task types for the A2A protocol |

### Protocol Constants

| Constant | Description |
|----------|-------------|
| `A2A_VERSION` | A2A protocol version string, in the `Major.Minor` wire form (`"1.0"`) |
| `A2A_CONTENT_TYPE` | The registered A2A media type (spec §14.1.1), accepted on ingress by the HTTP bindings alongside `JSON_CONTENT_TYPE` |
| `JSON_CONTENT_TYPE` | Content type emitted by the JSON-RPC and REST bindings (`application/json`) |
| `A2A_VERSION_HEADER` | HTTP header name for the A2A protocol version (`A2A-Version`) |
| `A2A_EXTENSIONS_HEADER` | HTTP header name for extension activation (spec §14.2.2) |
| `WEBSOCKET_BINDING_URI` | This project's identifier for its §12 WebSocket binding, as `AgentInterface::protocol_binding` |

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
| `Part` | Content unit: text, raw, url, or data |
| `PartContent` | Enum: Text, Raw, Url, Data |
| `FileContent` | Content of a file part. Deprecated: exists for backward compatibility with v0.3; in v1.0 use `Part::raw` or `Part::url` |

### Artifacts

| Type | Description |
|------|-------------|
| `Artifact` | Result produced by an agent. Call `validate()` to check non-empty parts. |
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

### Extensions

| Type | Description |
|------|-------------|
| `AgentExtension` | Describes an optional extension that an agent supports |
| `AgentCardSignature` | A cryptographic signature over an `AgentCard` |

### Security Schemes

| Type | Description |
|------|-------------|
| `SecurityScheme` | A security scheme supported by an agent |
| `NamedSecuritySchemes` | A map from security scheme name to its definition, as used in `AgentCard.securitySchemes` |
| `SecurityRequirement` | A security requirement object mapping scheme names to their required scopes |
| `StringList` | A list of strings used within a `SecurityRequirement` map value |
| `ApiKeySecurityScheme` | API key security scheme: a token sent in a header, query parameter, or cookie |
| `ApiKeyLocation` | Where an API key is placed in the request |
| `HttpAuthSecurityScheme` | HTTP authentication security scheme (Bearer, Basic, etc.) |
| `OAuth2SecurityScheme` | OAuth 2.0 security scheme |
| `OAuthFlows` | Available OAuth 2.0 flows for an `OAuth2SecurityScheme` |
| `AuthorizationCodeFlow` | OAuth 2.0 authorization code flow |
| `ClientCredentialsFlow` | OAuth 2.0 client credentials flow |
| `DeviceCodeFlow` | OAuth 2.0 device authorization flow (RFC 8628) |
| `ImplicitFlow` | OAuth 2.0 implicit flow (deprecated; retained for compatibility) |
| `PasswordOAuthFlow` | OAuth 2.0 resource owner password credentials flow (deprecated but in spec) |
| `OpenIdConnectSecurityScheme` | OpenID Connect security scheme |
| `MutualTlsSecurityScheme` | Mutual TLS security scheme |

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
| `AcceptedFields` | Trait: the JSON keys a request type accepts, in both protobuf spellings |

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

### Serialization Helpers

| Type | Description |
|------|-------------|
| `SerBuffer` | Thread-local reusable serialization buffer (2.3x less small-payload overhead) |
| `deser_from_str` | Borrowed deserialization from `&str` (~15-25% fewer allocations) |
| `deser_from_slice` | Borrowed deserialization from `&[u8]` (~15-25% fewer allocations) |

### Timestamps

| Function | Description |
|----------|-------------|
| `utc_now_iso8601()` | Returns the current UTC time as an ISO 8601 string with millisecond precision |
| `unix_millis_to_iso8601(millis)` | Formats Unix-epoch milliseconds as an ISO 8601 UTC string with millisecond precision; pre-epoch clamps to the epoch |
| `parse_iso8601_to_unix_millis(s)` | Parses an ISO 8601 / RFC 3339 timestamp into milliseconds since the Unix epoch; `None` for anything structurally invalid |

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
| `JsonRpcResponse` | JSON-RPC 2.0 response: either a success with a `result` or an error with an `error` object |
| `JsonRpcSuccessResponse` | A successful JSON-RPC 2.0 response |
| `JsonRpcErrorResponse` | An error JSON-RPC 2.0 response |
| `JsonRpcRequestId` | A JSON-RPC 2.0 request identifier with three distinct states |
| `JsonRpcId` | A JSON-RPC 2.0 response identifier |

## Client (`a2a-protocol-client`)

### Modules

| Module | Contents |
|--------|----------|
| `auth` | Authentication interceptor and credential storage |
| `builder` | Fluent builder for `A2aClient` |
| `client` | The `A2aClient` itself |
| `config` | Client configuration types |
| `discovery` | Agent card discovery with HTTP caching |
| `error` | Client error types |
| `interceptor` | Request/response interceptor infrastructure |
| `methods` | Per-method client helpers |
| `retry` | Configurable retry policy for transient client errors |
| `streaming` | SSE client-side streaming support |
| `tls` | TLS connector via rustls (`tls-rustls` feature) |
| `token_provider` | Token acquisition: `TokenProvider`, OAuth 2.0 client-credentials, and OIDC discovery |
| `transport` | Transport abstraction for A2A client requests |

### Core Types

| Type | Description |
|------|-------------|
| `A2aClient` | Main client for calling remote agents |
| `ClientBuilder` | Fluent builder for client configuration |
| `ClientConfig` | Configuration for an `A2aClient` instance |
| `EventStream` | Async SSE event stream |
| `RetryPolicy` | Configurable retry with exponential backoff |
| `ClientError` | Errors that can occur during A2A client operations |
| `ClientResult<T>` | Alias for `Result<T, ClientError>` |

### Discovery

| Function | Description |
|----------|-------------|
| `resolve_agent_card(base_url)` | `async` — fetches the `AgentCard` from the standard well-known path |

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
| `ClientRequest` | A logical A2A request as seen by interceptors |
| `ClientResponse` | A logical A2A response as seen by interceptors |

### Authentication

| Type | Description |
|------|-------------|
| `AuthInterceptor` | A `CallInterceptor` that injects Authorization headers from a `CredentialsStore` |
| `CredentialsStore` | Persistent storage for auth credentials, keyed by session + scheme |
| `InMemoryCredentialsStore` | An in-memory `CredentialsStore` backed by an `RwLock<HashMap>` |
| `SessionId` | Opaque identifier for a client authentication session |
| `TokenProvider` | A source of bearer access tokens |
| `StaticTokenProvider` | A `TokenProvider` that always returns the same fixed token |
| `OAuth2ClientCredentials` | A `TokenProvider` implementing the OAuth 2.0 client credentials grant (RFC 6749 §4.4) with caching and proactive refresh |
| `BearerAuthInterceptor` | A `CallInterceptor` that injects a bearer token from a `TokenProvider` before every request |

### Transport

| Type | Description |
|------|-------------|
| `Transport` | Pluggable transport trait |
| `JsonRpcTransport` | JSON-RPC 2.0 transport |
| `RestTransport` | REST/HTTP transport |
| `WebSocketTransport` | WebSocket transport (`websocket` feature) |
| `WebSocketTransportConfig` | Configuration for `WebSocketTransport::connect_with_config` (`websocket` feature) |
| `GrpcTransport` | gRPC transport (`grpc` feature); dials `host:port` targets and `http(s)://` URLs |
| `GrpcBareAddressScheme` | How a bare `host:port` gRPC target is dialled: TLS except loopback (default), always TLS, or always plaintext |

## Server (`a2a-protocol-server`)

### Modules

| Module | Contents |
|--------|----------|
| `agent_card` | Agent card HTTP handlers (static, dynamic, and caching utilities) |
| `auth` | Server-side authentication interceptors |
| `builder` | Builder for `RequestHandler` |
| `call_context` | Call context for server-side interceptors |
| `dispatch` | HTTP dispatch layer — JSON-RPC and REST routing |
| `error` | Server-specific error types |
| `executor` | Agent executor trait |
| `executor_helpers` | Ergonomic helpers for implementing `AgentExecutor` |
| `handler` | Core request handler — protocol logic layer |
| `interceptor` | Server-side interceptor chain |
| `metrics` | Metrics hooks for observing handler activity |
| `otel` | OpenTelemetry integration for the A2A server (`otel` feature) |
| `push` | Push notification configuration storage and delivery |
| `rate_limit` | Fixed-window rate limiter as a `ServerInterceptor` |
| `request_context` | Request context passed to the `AgentExecutor` |
| `serve` | `serve()`, `serve_with_addr`, `Dispatcher` |
| `store` | Task storage backend |
| `streaming` | Streaming infrastructure for SSE responses and event queues |
| `tenant_config` | Per-tenant resource limits for multi-tenant A2A servers |
| `tenant_resolver` | Tenant resolution for multi-tenant A2A servers |

### Constants

| Constant | Description |
|----------|-------------|
| `CORS_ALLOW_ALL` | CORS Access-Control-Allow-Origin header value for public agent cards |
| `A2A_VERSION_METADATA_KEY` | The service parameter naming the A2A protocol version, spelled the way a non-HTTP binding carries it |

### Core Types

| Type | Description |
|------|-------------|
| `RequestHandler` | Central protocol orchestrator |
| `RequestHandlerBuilder` | Fluent builder for handler configuration |
| `RequestContext` | Per-execution context (task ID, message, etc.) |
| `CallContext` | Per-request metadata (request ID, headers, tenant) |
| `HandlerLimits` | Configurable validation limits |
| `SendMessageResult` | Result of `RequestHandler::on_send_message`: a synchronous response or a streaming reader |
| `ShutdownReport` | What a shutdown actually managed to do (queues force-destroyed, whether executor cleanup completed) |
| `ConnectionPoolStats` | Statistics about the HTTP connection pool |

### Traits

| Trait | Description |
|-------|-------------|
| `AgentExecutor` | Agent logic entry point |
| `TaskStore` | Task persistence backend |
| `PushConfigStore` | Push config persistence |
| `PushSender` | Webhook delivery |
| `ServerInterceptor` | Server-side middleware |
| `AgentCardProducer` | Dynamic agent card generation |
| `Dispatcher` | HTTP dispatch trait (for `serve()`) |
| `Metrics` | Pluggable metrics observer (requests, latency, errors) |
| `TenantResolver` | Extracts tenant from request context |
| `RateLimitCounter` | A request counter every replica shares |

### Dispatchers

| Type | Description |
|------|-------------|
| `JsonRpcDispatcher` | JSON-RPC 2.0 HTTP dispatcher (implements `Dispatcher`) |
| `RestDispatcher` | RESTful HTTP dispatcher (implements `Dispatcher`) |
| `WebSocketDispatcher` | WebSocket dispatcher (`websocket` feature) |
| `GrpcDispatcher` | gRPC dispatcher (`grpc` feature) |
| `A2aRouter` | Axum framework adapter (`axum` feature) |

### Server Startup

| Name | Description |
|------|-------------|
| `serve(addr, dispatcher) -> io::Result<()>` | `async` — binds and drives the accept loop until the future is dropped |
| `serve_with_addr(addr, dispatcher) -> io::Result<SocketAddr>` | `async` — binds, spawns the accept loop, returns the bound `SocketAddr` (useful for port-0 in tests) |
| `Server` | A bound listener that has not started accepting yet; binding is separated from serving so the caller can learn the address |
| `ServeConfig` | Limits applied to a `Server` |
| `ServeReport` | What the socket layer did, and whether it finished |
| `DispatchConfig` | Configuration for dispatch-layer limits shared by both JSON-RPC and REST dispatchers |
| `GrpcConfig` | Configuration for the gRPC dispatcher (`grpc` feature) |
| `validate_version_metadata(metadata, required)` | Validates the A2A version carried in a binding's request metadata |

### Executor Helpers

| Name | Description |
|------|-------------|
| `agent_executor!` | Macro: generates an `AgentExecutor` implementation from a closure-like syntax |
| `boxed_future` | Wraps an async expression into a pinned, boxed, `Send` future |

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
| `HeaderTenantResolver` | `TenantResolver` that reads a configurable request header |
| `BearerTokenTenantResolver` | `TenantResolver` that extracts tenant claims from a JWT bearer token |
| `PathSegmentTenantResolver` | `TenantResolver` that parses tenant from a configurable path segment |
| `RateLimitInterceptor` | Per-caller rate limiting interceptor |
| `PostgresRateLimitCounter` | A `RateLimitCounter` backed by a PostgreSQL table (`postgres` feature) |
| `ApiKeyAuthInterceptor` | Rejects requests whose API-key header is absent or not in the allowed set |
| `BearerTokenAuthInterceptor` | Rejects requests whose bearer token is absent or not in the allowed set |
| `ServerInterceptorChain` | An ordered chain of `ServerInterceptor` instances |
| `Migration` | A single SQLite schema migration (`sqlite` feature) |
| `MigrationRunner` | Runs schema migrations against a SQLite database (`sqlite` feature) |
| `PgMigration` | A single PostgreSQL schema migration (`postgres` feature) |
| `NoopMetrics` | No-op metrics implementation (default) |
| `OtelMetrics` | OpenTelemetry metrics (`otel` feature) |

### Streaming

| Name | Kind | Description |
|------|------|-------------|
| `EventEmitter` | struct | Ergonomic event emission helper (wraps an `EventQueueWriter`) |
| `EventQueueWriter` | trait | Write events to a task's event stream |
| `EventQueueReader` | trait | Read events from a task's event stream |
| `EventQueueManager` | struct | Per-task queue lifecycle manager (create / lookup / destroy) |
| `InMemoryQueueWriter` | struct | Bounded-channel `EventQueueWriter` implementation |
| `InMemoryQueueReader` | struct | Bounded-channel `EventQueueReader` implementation |

### Configuration

| Type | Description |
|------|-------------|
| `CorsConfig` | Cross-origin policy |
| `TaskStoreConfig` | TTL and capacity for in-memory store |
| `TenantStoreConfig` | Configuration for `TenantAwareInMemoryTaskStore` |
| `TenantContext` | Thread-safe tenant context for scoping store operations |
| `PerTenantConfig` | Per-tenant configuration for timeouts, capacity limits, and executor selection |
| `TenantLimits` | Resource limits declared for a single tenant |
| `RateLimitConfig` | Configuration for `RateLimitInterceptor` |
| `PushRetryPolicy` | Retry policy for push notification delivery |
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

```rust,ignore
// Task status
TaskStatus::new(TaskState::Working)
TaskStatus::with_timestamp(TaskState::Completed)

// Messages and parts (v1.0 wire format: flat oneof)
Part::text("hello")                 // → {"text": "hello"}
Part::raw(base64_string)            // → {"raw": "aGVsbG8="}
Part::url("https://...")            // → {"url": "https://..."}
Part::data(serde_json::json!({..})) // → {"data": {...}}
Part::file_bytes(base64_string)     // backward-compat alias for raw()
Part::file_uri("https://...")       // backward-compat alias for url()

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
