# Request Handler & Builder

The `RequestHandler` is the central orchestrator that connects your executor to the protocol. It manages task lifecycle, storage, streaming, push notifications, and interceptors. You build one using `RequestHandlerBuilder`.

## Building a Handler

### Minimal Setup

```rust,ignore
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

```rust,ignore
use a2a_protocol_sdk::server::RequestHandlerBuilder;
use std::time::Duration;

let handler = RequestHandlerBuilder::new(MyExecutor)
    // Agent card for discovery
    .with_agent_card(make_agent_card())

    // Task storage
    .with_task_store_config(
        TaskStoreConfig::default()
            .with_task_ttl(Some(Duration::from_secs(3600))) // 1 hour TTL
            .with_max_capacity(Some(10_000)),               // Max 10k tasks
    )

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
| `with_event_queue_capacity(usize)` | 256 | Bounded channel size per stream |
| `with_max_event_size(usize)` | 16 MiB | Maximum serialized event size |
| `with_max_concurrent_streams(usize)` | 1,024 | Limit concurrent SSE streams (pass `usize::MAX` to disable) |
| `with_handler_limits(HandlerLimits)` | Sensible defaults | Configurable limits (see [HandlerLimits](#handlerlimits) below) |
| `with_task_store_arc(Arc<dyn TaskStore>)` | — | Share a store instance via `Arc` |
| `with_metrics(impl Metrics)` | `NoopMetrics` | Metrics observer for handler activity |
| `with_tenant_resolver(impl TenantResolver)` | None | Multi-tenant tenant extraction |
| `with_tenant_config(PerTenantConfig)` | None | Per-tenant concurrency, executor timeout and queue capacity. `rate_limit_rps` additionally needs the same config on `RateLimitInterceptor::with_tenant_config` |

### HandlerLimits

The `HandlerLimits` struct configures per-handler bounds:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_id_length` | `usize` | 1024 | Maximum allowed length for task/context IDs |
| `max_metadata_size` | `usize` | 1 MiB | Maximum serialized metadata size in bytes |
| `max_cancellation_tokens` | `usize` | 10,000 | Max cancellation token map entries before cleanup |
| `max_token_age` | `Duration` | 1 hour | Maximum age for cancellation tokens |
| `push_delivery_timeout` | `Duration` | 5 seconds | Timeout for individual push webhook deliveries |
| `push_delivery_budget` | `Duration` | 30 seconds | Total push-delivery time per event across every registered config (per request batch on the blocking path); the rest are counted `skipped` |
| `executor_drain_timeout` | `Duration` | 5 seconds | How long a blocking `SendMessage` waits for the event queue to close after the executor finished; then answers with the task as collected |
| `max_artifacts_per_task` | `usize` | 1000 | Maximum artifacts per task (prevents unbounded growth) |
| `max_context_locks` | `usize` | 10,000 | Max per-context locks before cleanup |
| `max_push_configs_per_task` | `usize` | 100 | Maximum push configs per task (uniform across store backends) |
| `max_parts_per_artifact` | `usize` | 10,000 | Maximum parts a single artifact may accumulate |
| `max_total_push_configs` | `usize` | 100,000 | Global push-config ceiling across all tasks |

### Build-Time Validation

`build()` validates:
- If an agent card is provided, it must have at least one `supported_interfaces` entry
- Executor timeout (if set) must not be zero
- `max_id_length` must be greater than zero
- `max_metadata_size` must be greater than zero
- `push_delivery_timeout` must be non-zero

## Calling the Handler Directly

`RequestHandler` is the protocol layer, and it is transport-agnostic: no method
takes an HTTP type. Requests arrive as parsed params plus a plain
`HashMap<String, String>` of headers, which is all the interceptor chain needs
to make access-control decisions. The [dispatchers](./dispatchers.md) are one
way to feed it, not the only way.

That matters if you already own your HTTP surface. An agent framework, an
existing Axum or Actix application, a tower service, a queue consumer, or a
test harness can call these methods directly and keep its own routing,
middleware, and server lifecycle:

| Method | A2A operation |
|--------|---------------|
| `on_send_message(params, streaming, headers)` | `SendMessage`, `SendStreamingMessage` |
| `on_get_task(params, headers)` | `GetTask` |
| `on_list_tasks(params, headers)` | `ListTasks` |
| `on_cancel_task(params, headers)` | `CancelTask` |
| `on_resubscribe(params, headers)` | `TaskSubscription` |
| `on_get_extended_agent_card(params, headers)` | `GetExtendedAgentCard` |
| `on_set_push_config(params, headers)` | `CreateTaskPushNotificationConfig` |
| `on_get_push_config(params, headers)` | `GetTaskPushNotificationConfig` |
| `on_list_push_configs(params, headers)` | `ListTaskPushNotificationConfigs` |
| `on_delete_push_config(params, headers)` | `DeleteTaskPushNotificationConfig` |

Those ten methods are the whole protocol surface. Everything the dispatchers do
on top is decoding the wire format and encoding the reply.

```rust
use std::collections::HashMap;

use a2a_protocol_server::agent_executor;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::{Message, MessageId, MessageRole, Part};

// Your agent. Nothing in it knows how the request arrived.
struct EchoAgent;
agent_executor!(EchoAgent, |_ctx, _queue| async { Ok(()) });

let handler = RequestHandlerBuilder::new(EchoAgent)
    .build()
    .expect("build handler");

// Whatever your framework hands you becomes params plus a header map.
let params = MessageSendParams {
    message: Message {
        id: MessageId::new("msg-1"),
        role: MessageRole::User,
        parts: vec![Part::text("hello")],
        context_id: None,
        task_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    },
    configuration: None,
    metadata: None,
    tenant: None,
};

let mut headers = HashMap::new();
headers.insert("authorization".into(), "Bearer token".into());

let runtime = tokio::runtime::Runtime::new().expect("runtime");
let result = runtime
    .block_on(handler.on_send_message(params, false, Some(&headers)))
    .expect("send should succeed");

// `false` for `streaming` yields a synchronous response; `true` yields a
// reader you drain to produce SSE, a WebSocket feed, or whatever your
// transport emits.
assert!(matches!(result, SendMessageResult::Response(_)));
```

Pass `None` for `headers` when there is nothing to authenticate against — an
in-process call, or a transport that has already authenticated the caller.
Interceptors still run; they simply see an empty header set.

What you give up by skipping the dispatchers is exactly what they implement:
JSON-RPC envelope parsing and error mapping, REST path and query binding, SSE
framing, and the agent-card endpoint. What you keep is every protocol
guarantee — task lifecycle, idempotency, streaming, push delivery,
interceptors, multi-tenancy, and limits — because all of it lives here, below
the transport.

## Sharing the Handler

The handler is wrapped in `Arc` for sharing between dispatchers:

```rust,ignore
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

```rust,ignore
use a2a_protocol_sdk::server::TaskStoreConfig;
use std::time::Duration;

let config = TaskStoreConfig::default()
    .with_task_ttl(Some(Duration::from_secs(3600))) // Tasks expire after 1 hour
    .with_max_capacity(Some(50_000));               // Keep at most 50k tasks

RequestHandlerBuilder::new(executor)
    .with_task_store_config(config)
    .build()
```

When capacity is exceeded, the oldest tasks are evicted. When TTL expires, tasks are cleaned up on the next access.

## Custom Task Stores

For production use, implement the `TaskStore` trait for your database:

```rust,ignore
use a2a_protocol_sdk::server::TaskStore;

struct DynamoDbTaskStore { /* ... */ }

impl TaskStore for DynamoDbTaskStore {
    // Implement save, get, list, insert_if_absent, delete...
}

RequestHandlerBuilder::new(executor)
    .with_task_store(DynamoDbTaskStore::new(client))
    .build()
```

See [Task & Config Stores](./stores.md) for the full trait API.

## Next Steps

- **[Dispatchers](./dispatchers.md)** — HTTP dispatchers for JSON-RPC and REST
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Task & Config Stores](./stores.md)** — Custom storage backends
