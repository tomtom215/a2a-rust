# Request Handler & Builder

The `RequestHandler` is the central orchestrator that connects your executor to the protocol. It manages task lifecycle, storage, streaming, push notifications, and interceptors. You build one using `RequestHandlerBuilder`.

## Building a Handler

### Minimal Setup

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
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
- 1-hour executor timeout (`without_executor_timeout()` removes it)

### Full Configuration

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# use a2a_protocol_sdk::server::{HttpPushSender, TaskStoreConfig};
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")) }
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
    .with_interceptor(BearerTokenAuthInterceptor::new(["service-token"]))
    .with_interceptor(RateLimitInterceptor::new(RateLimitConfig::default()).expect("rate limit"))

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
| `with_executor_timeout(Duration)` | 1 hour | Timeout for executor completion |
| `without_executor_timeout()` | — | Remove the 1-hour ceiling (an executor that never returns then pins its task, queue and cancellation token) |
| `with_event_queue_capacity(usize)` | 256 | Bounded channel size per stream |
| `with_max_event_size(usize)` | 16 MiB | Maximum serialized event size |
| `with_max_concurrent_streams(usize)` | 1,024 | Limit concurrent SSE streams (pass `usize::MAX` to disable) |
| `with_handler_limits(HandlerLimits)` | Sensible defaults | Configurable limits (see [HandlerLimits](#handlerlimits) below) |
| `with_task_store_arc(Arc<dyn TaskStore>)` | — | Share a store instance via `Arc` |
| `with_metrics(impl Metrics)` | `NoopMetrics` | Metrics observer for handler activity |
| `with_tenant_resolver(impl TenantResolver)` | None | Multi-tenant tenant extraction |
| `with_tenant_config(PerTenantConfig)` | None | Per-tenant concurrency, executor timeout and queue capacity. `rate_limit_rps` additionally needs the same config on `RateLimitInterceptor::with_tenant_config` |
| `require_resolved_tenant()` | off | Refuse a request the configured tenant resolver cannot place, instead of using the shared default partition |
| `with_inbound_trace_policy(InboundTracePolicy)` | `Continue` | What to do with a `traceparent` sent by a not-yet-authenticated peer (`Continue`, `Restart` or `Drop`) |
| `allow_unauthenticated_extended_card()` | off | Serve `GetExtendedAgentCard` even when no authenticating interceptor is registered (spec §13.3 requires one) |
| `allow_undeclared_input_modes()` | off (enforced) | When the card declares `defaultInputModes` or any skill's `inputModes`, `SendMessage` refuses a part whose explicit `mediaType` is outside them with `ContentTypeNotSupportedError` (-32005, HTTP 400). Parts without a `mediaType`, and cards declaring no modes, are not checked. This turns the check off |

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
| `subscribe_reattach_interval` | `Duration` | 250 ms | How often a `SubscribeToTask` stream re-checks whether its task has finished once the current turn's queue has closed |
| `subscribe_max_idle` | `Duration` | 5 minutes | How long a `SubscribeToTask` stream waits for a parked task to make progress before ending; the client resubscribes (§3.5.2) |
| `subscribe_replay_limit` | `usize` | 1,000 | Maximum logged events replayed to a client resuming with `Last-Event-ID` |
| `subscribe_replay_catchup` | `Duration` | 2 seconds | How long a resuming `SubscribeToTask` waits for the event log to catch up with what was already broadcast; zero disables the wait |

### Build-Time Validation

`build()` validates:
- If an agent card is provided, it must have at least one `supported_interfaces` entry
- Executor timeout (if set) must not be zero
- `max_id_length` must be greater than zero
- `max_metadata_size` must be greater than zero
- `push_delivery_timeout` must be non-zero
- A signed agent card must already declare every extension the handler advertises (adding one would invalidate the signature)

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
| `on_resubscribe(params, headers)` | `SubscribeToTask` |
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
let params = MessageSendParams::new(Message::user_text("msg-1", "hello"));

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

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
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
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# fn f(executor: MyExecutor) -> ServerResult<RequestHandler> {
use a2a_protocol_sdk::server::TaskStoreConfig;
use std::time::Duration;

let config = TaskStoreConfig::default()
    .with_task_ttl(Some(Duration::from_secs(3600))) // Tasks expire after 1 hour
    .with_max_capacity(Some(50_000));               // Keep at most 50k tasks

RequestHandlerBuilder::new(executor)
    .with_task_store_config(config)
    .build()
# }
```

When capacity is exceeded, the oldest terminal tasks are evicted first, then non-terminal ones if that is not enough. Terminal tasks older than the TTL are removed by a sweep that runs every `eviction_interval` writes (64 by default).

## Custom Task Stores

For production use, implement the `TaskStore` trait for your database:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# use a2a_protocol_sdk::types::task::TaskId;
use a2a_protocol_sdk::server::TaskStore;

struct MyTaskStore { /* ... */ }

impl TaskStore for MyTaskStore {
    // Implement save, get, list, insert_if_absent, delete...
#     fn save<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
#     fn get<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> { unimplemented!() }
#     fn list<'a>(&'a self, _: &'a ListTasksParams) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> { unimplemented!() }
#     fn insert_if_absent<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> { unimplemented!() }
#     fn delete<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
}

# fn f(executor: MyExecutor) -> ServerResult<RequestHandler> {
RequestHandlerBuilder::new(executor)
    .with_task_store(MyTaskStore { /* ... */ })
    .build()
# }
```

See [Task & Config Stores](./stores.md) for the full trait API.

## Next Steps

- **[Dispatchers](./dispatchers.md)** — HTTP dispatchers for JSON-RPC and REST
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Task & Config Stores](./stores.md)** — Custom storage backends
