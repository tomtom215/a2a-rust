# Request Handler & Builder

The `RequestHandler` is the central orchestrator that connects your executor to the protocol. It manages task lifecycle, storage, streaming, push notifications, and interceptors. You build one using `RequestHandlerBuilder`.

## Building a Handler

### Minimal Setup

```rust
use a2a_protocol_sdk::server::RequestHandlerBuilder;

let handler = RequestHandlerBuilder::new(MyExecutor)
    .build()
    .expect("build handler");
```

This gives you sensible defaults:
- In-memory task store
- In-memory push config store
- No push sender (webhooks disabled)
- No interceptors
- No agent card
- No executor timeout

### Full Configuration

```rust
use a2a_protocol_sdk::server::RequestHandlerBuilder;
use std::time::Duration;

let handler = RequestHandlerBuilder::new(MyExecutor)
    // Agent card for discovery
    .with_agent_card(make_agent_card())

    // Task storage
    .with_task_store_config(TaskStoreConfig {
        task_ttl: Some(Duration::from_secs(3600)),  // 1 hour TTL
        max_capacity: Some(10_000),                 // Max 10k tasks
        ..Default::default()
    })

    // Push notifications
    .with_push_sender(HttpPushSender::new())

    // Interceptors
    .with_interceptor(AuthInterceptor::new())
    .with_interceptor(LoggingInterceptor::new())

    // Executor limits
    .with_executor_timeout(Duration::from_secs(300))

    // Streaming limits
    .with_event_queue_capacity(128)
    .with_max_event_size(8 * 1024 * 1024)    // 8 MiB
    .with_max_concurrent_streams(1000)

    .build()
    .expect("build handler");
```

## Builder Methods Reference

### Required

| Method | Description |
|--------|-------------|
| `new(executor)` | Set the agent executor (type-erased to `Arc<dyn AgentExecutor>`) |

### Optional

| Method | Default | Description |
|--------|---------|-------------|
| `with_agent_card(AgentCard)` | None | Discovery card for `/.well-known/agent-card.json` |
| `with_task_store(impl TaskStore)` | `InMemoryTaskStore` | Custom task storage backend |
| `with_task_store_config(TaskStoreConfig)` | 1hr TTL, 10k capacity | TTL and capacity for the default store |
| `with_push_config_store(impl PushConfigStore)` | `InMemoryPushConfigStore` | Custom push config storage |
| `with_push_sender(impl PushSender)` | None | Webhook delivery implementation |
| `with_interceptor(impl ServerInterceptor)` | Empty chain | Add a server interceptor |
| `with_executor_timeout(Duration)` | None | Timeout for executor completion |
| `with_event_queue_capacity(usize)` | 64 | Bounded channel size per stream |
| `with_max_event_size(usize)` | 16 MiB | Maximum serialized event size |
| `with_max_concurrent_streams(usize)` | Unbounded | Limit concurrent SSE streams |
| `with_event_queue_write_timeout(Duration)` | 5 seconds | Prevents executor blocking on slow clients |
| `with_handler_limits(HandlerLimits)` | Sensible defaults | Configurable limits (see [HandlerLimits](#handlerlimits) below) |
| `with_task_store_arc(Arc<dyn TaskStore>)` | — | Share a store instance via `Arc` |
| `with_metrics(impl Metrics)` | `NoopMetrics` | Metrics observer for handler activity |
| `with_tenant_resolver(impl TenantResolver)` | None | Multi-tenant tenant extraction |
| `with_tenant_config(PerTenantConfig)` | None | Per-tenant differentiated limits |

### HandlerLimits

The `HandlerLimits` struct configures per-handler bounds:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_id_length` | `usize` | 1024 | Maximum allowed length for task/context IDs |
| `max_metadata_size` | `usize` | 1 MiB | Maximum serialized metadata size in bytes |
| `max_cancellation_tokens` | `usize` | 10,000 | Max cancellation token map entries before cleanup |
| `max_token_age` | `Duration` | 1 hour | Maximum age for cancellation tokens |
| `push_delivery_timeout` | `Duration` | 5 seconds | Timeout for individual push webhook deliveries |
| `max_artifacts_per_task` | `usize` | 1000 | Maximum artifacts per task (prevents unbounded growth) |

### Build-Time Validation

`build()` validates:
- If an agent card is provided, it must have at least one `supported_interfaces` entry
- Executor timeout (if set) must not be zero
- `max_id_length` must be greater than zero
- `max_metadata_size` must be greater than zero
- `push_delivery_timeout` must be non-zero

## Sharing the Handler

The handler is wrapped in `Arc` for sharing between dispatchers:

```rust
use std::sync::Arc;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .build()
        .unwrap()
);

// Both dispatchers share the same handler
let jsonrpc = JsonRpcDispatcher::new(Arc::clone(&handler));
let rest = RestDispatcher::new(handler);
```

This means JSON-RPC and REST clients share the same task store, push configs, and executor.

## Task Store Configuration

The default `InMemoryTaskStore` supports TTL and capacity limits:

```rust
use a2a_protocol_sdk::server::TaskStoreConfig;
use std::time::Duration;

let config = TaskStoreConfig {
    task_ttl: Some(Duration::from_secs(3600)),  // Tasks expire after 1 hour
    max_capacity: Some(50_000),                 // Keep at most 50k tasks
    ..Default::default()
};

RequestHandlerBuilder::new(executor)
    .with_task_store_config(config)
    .build()
```

When capacity is exceeded, the oldest tasks are evicted. When TTL expires, tasks are cleaned up on the next access.

## Custom Task Stores

For production use, implement the `TaskStore` trait for your database:

```rust
use a2a_protocol_sdk::server::TaskStore;

struct PostgresTaskStore { /* ... */ }

impl TaskStore for PostgresTaskStore {
    // Implement get, put, list, delete...
}

RequestHandlerBuilder::new(executor)
    .with_task_store(PostgresTaskStore::new(pool))
    .build()
```

See [Task & Config Stores](./stores.md) for the full trait API.

## Next Steps

- **[Dispatchers](./dispatchers.md)** — HTTP dispatchers for JSON-RPC and REST
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Task & Config Stores](./stores.md)** — Custom storage backends
