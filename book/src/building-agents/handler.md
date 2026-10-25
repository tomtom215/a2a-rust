# Request Handler & Builder

The `RequestHandler` is the central orchestrator that connects your executor to the protocol. It manages task lifecycle, storage, streaming, push notifications, and interceptors. You build one using `RequestHandlerBuilder`.

## Building a Handler

### Minimal Setup

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
use a2a_protocol_sdk::server::RequestHandlerBuilder;

let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_agent_card(card())
    .build()
    .expect("build handler");
```

This gives you sensible defaults:
- In-memory task store
- In-memory push config store
- No push sender (webhooks disabled)
- No interceptors
- 1-hour executor timeout (`without_executor_timeout()` removes it)

The agent card has no default. An A2A server must publish one (spec §8.1), so
`build()` refuses a handler without it unless you call `without_agent_card()`,
which is for a card published elsewhere or a handler that is not a public A2A
server, such as one in a test.

### Full Configuration

```rust
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# use a2a_protocol_sdk::server::{HttpPushSender, TaskStoreConfig};
# fn make_agent_card() -> AgentCard { AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000")).with_description("An example agent").with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#     // Spec §3.3.4: the push sender below needs a card that declares push.
#     .with_capabilities(a2a_protocol_types::agent_card::AgentCapabilities::none().with_push_notifications(true))
#     // Spec §13.3: the scheme the bearer interceptor below checks.
#     .with_security_schemes(std::collections::HashMap::from([("bearer".to_owned(), a2a_protocol_types::security::SecurityScheme::Http(a2a_protocol_types::security::HttpAuthSecurityScheme { scheme: "Bearer".to_owned(), description: None, bearer_format: None }))])) }
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

    // Push notifications (the card must declare `pushNotifications`)
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
| `with_agent_card(AgentCard)` | Discovery card for `/.well-known/agent-card.json`; the handler checks requests against what it declares. `build()` refuses a handler with no card (spec §8.1) unless you call `without_agent_card()` |

### Optional

| Method | Default | Description |
|--------|---------|-------------|
| `without_agent_card()` | — | Build without a card, which `build()` otherwise refuses (spec §8.1): for a card published elsewhere, or a handler that is not a public A2A server |
| `with_extended_agent_card(AgentCard)` | none | What `GetExtendedAgentCard` serves; without it a card declaring `extendedAgentCard` answers `ExtendedAgentCardNotConfiguredError` |
| `with_extended_card_provider(impl ExtendedCardProvider)` | none | Chooses each caller's extended card from the one set with `with_extended_agent_card` |
| `with_task_store(impl TaskStore)` | `InMemoryTaskStore`; `TenantAwareInMemoryTaskStore` when tasks are isolated by caller | Custom task storage backend |
| `with_task_store_config(TaskStoreConfig)` | 1hr TTL, 10k capacity | TTL and capacity for the default store; per caller partition when tasks are isolated by caller, so the store holds up to 1,000 times the capacity |
| `with_push_config_store(impl PushConfigStore)` | `InMemoryPushConfigStore`; the tenant-aware one when the task store partitions by tenant | Custom push config storage |
| `with_push_sender(impl PushSender)` | None | Webhook delivery implementation. The push-config operations need it and a card that declares `capabilities.pushNotifications: true`; `build()` refuses a card that declares push with no sender (spec §3.3.4) |
| `with_interceptor(impl ServerInterceptor)` | Empty chain | Add a server interceptor |
| `with_executor_timeout(Duration)` | 1 hour | Timeout for executor completion |
| `without_executor_timeout()` | — | Remove the 1-hour ceiling (an executor that never returns then pins its task, queue and cancellation token) |
| `with_event_queue_capacity(usize)` | 256 | Bounded channel size per stream |
| `with_max_event_size(usize)` | 16 MiB | Maximum serialized event size |
| `with_max_concurrent_streams(usize)` | 1,024 | Limit concurrent event queues: one per task being run, whether its send blocks or streams; a send that would open one more is refused as overloaded (pass `usize::MAX` to disable) |
| `with_handler_limits(HandlerLimits)` | Sensible defaults | Configurable limits (see [HandlerLimits](#handlerlimits) below) |
| `with_task_store_arc(Arc<dyn TaskStore>)` | — | Share a store instance via `Arc` |
| `with_metrics(impl Metrics)` | `OtelMetrics` on the global meter provider (`otel`, on by default); `NoopMetrics` without it | Metrics observer for handler activity; replaces the default |
| `with_additional_metrics(impl Metrics)` | — | Also report to this observer, keeping the default (`with_metrics` replaces it) |
| `with_tenant_resolver(impl TenantResolver)` | None | Multi-tenant tenant extraction |
| `with_tenant_config(PerTenantConfig)` | None | Per-tenant concurrency, executor timeout and queue capacity. `rate_limit_rps` additionally needs the same config on `RateLimitInterceptor::with_tenant_config` |
| `require_resolved_tenant()` | off | Refuse a request the configured tenant resolver cannot place, instead of using the shared default partition |
| `isolate_tasks_by_caller()` | on with an authenticating interceptor | A partition per caller within a tenant (spec §13.1). On by default for the callers an authenticating interceptor identifies; calling it also refuses a request with no identity. See [Multi-tenancy](../deployment/multi-tenancy.md#callers-within-one-tenant) |
| `share_tasks_across_callers()` | off | Every caller in a tenant sees its tasks, and `build()` accepts task and push-config stores that do not partition by tenant (a request that names a tenant is still refused over them unless `accept_unisolated_tenants()` is also called) |
| `accept_unisolated_tenants()` | off | Serve requests that name a tenant even though the stores do not partition by tenant; by default they are refused with `UnsupportedOperation`. See [Multi-tenancy](../deployment/multi-tenancy.md#a-store-that-cannot-isolate-refuses-tenants) |
| `with_inbound_trace_policy(InboundTracePolicy)` | `Continue` | What to do with a `traceparent` sent by a not-yet-authenticated peer (`Continue`, `Restart` or `Drop`) |
| `allow_unauthenticated_extended_card()` | off | Serve `GetExtendedAgentCard` even when no authenticating interceptor is registered (spec §13.3 requires one) |
| `authenticated_upstream()` | off | Accept a card whose `securityRequirements` demand a credential although no interceptor here authenticates, because a gateway in front does (spec §3.3.2, §7.4) |
| `allow_undeclared_input_modes()` | off (enforced) | When the card declares `defaultInputModes` or any skill's `inputModes`, `SendMessage` refuses a part whose explicit `mediaType` is outside them with `ContentTypeNotSupportedError` (-32005, HTTP 400). Parts without a `mediaType`, and cards declaring no modes, are not checked. This turns the check off |
| `refuse_private_file_urls()` | off | Refuse a message whose file `url` points into a private, loopback, link-local or internal network |

### HandlerLimits

The `HandlerLimits` struct configures per-handler bounds. It is
`#[non_exhaustive]`: start from `HandlerLimits::default()` and set fields with
its `with_*` methods (for example
`HandlerLimits::default().with_max_id_length(2048)`), then pass it to
`with_handler_limits`:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_id_length` | `usize` | 1024 | Maximum allowed length for task/context IDs; `message.id` is bounded by the same value but never below 36 |
| `max_metadata_size` | `usize` | 1 MiB | Maximum serialized metadata size in bytes |
| `max_cancellation_tokens` | `usize` | 10,000 | Max cancellation token map entries before cleanup |
| `max_token_age` | `Duration` | 1 hour | Maximum age for cancellation tokens |
| `push_delivery_timeout` | `Duration` | 30 seconds | Timeout for one webhook delivery, retries included |
| `push_delivery_budget` | `Duration` | 30 seconds | Target push-delivery time per event (per request batch on the blocking path). Every config is attempted; the budget sets how many deliveries run at once |
| `executor_drain_timeout` | `Duration` | 5 seconds | How long a blocking `SendMessage` waits for the event queue to close after the executor finished; then answers with the task as collected. Also how long a continuation waits for a parked (`input-required`/`auth-required`) turn to finish: its executor to return and its events to be stored. A turn still running after that refuses the continuation with `UnsupportedOperation` |
| `max_artifacts_per_task` | `usize` | 1000 | Maximum artifacts per task (prevents unbounded growth) |
| `max_context_locks` | `usize` | 10,000 | Max per-context locks before cleanup |
| `max_push_configs_per_task` | `usize` | 100 | Maximum push configs per task (uniform across store backends) |
| `max_parts_per_artifact` | `usize` | 10,000 | Maximum parts a single artifact may accumulate |
| `max_total_push_configs` | `usize` | 100,000 | Global push-config ceiling across all tasks |
| `subscribe_reattach_interval` | `Duration` | 250 ms | How often a `SubscribeToTask` stream between turns checks the store: for the task finishing, the next turn's queue, or events logged meanwhile, which it replays |
| `subscribe_max_idle` | `Option<Duration>` | none | If set, how long a `SubscribeToTask` stream waits for a parked task to make progress before ending without a terminal state; the client resubscribes (§3.5.2). Unset, the stream runs until the terminal state or until the client goes (§3.1.6) |
| `subscribe_replay_limit` | `usize` | 1,000 | Maximum logged events one read of the event log returns; a longer replay (a lag, a missed turn, a `Last-Event-ID` resumption) is read a page at a time |
| `final_task_snapshot` | `bool` | `false` | Whether a stream sends the finished task once more, as a final `Task`, after its terminal state (§11.7) |
| `subscribe_replay_catchup` | `Duration` | 2 seconds | How long a resuming `SubscribeToTask` waits for the event log to catch up with what was already broadcast; zero disables the wait |

### Build-Time Validation

`build()` refuses, with `ServerError::InvalidParams` naming the change to make:
- No agent card, unless you call `without_agent_card()` (spec §8.1)
- A card, or an extended card, that fails `AgentCard::validate` or `validate_security_schemes`: a non-empty `name`, `description` and `version`; at least one interface, default input mode, default output mode and skill; each skill with an `id`, `name`, `description` and a tag; and every other string `a2a.proto` marks REQUIRED (spec §5.7)
- A card with a credential in a URL
- A card that declares `capabilities.pushNotifications` with no `with_push_sender` (spec §3.3.4)
- A card whose `securityRequirements` (or a skill's) ask for a credential when no interceptor authenticates, unless you call `authenticated_upstream()` (spec §3.3.2, §7.4)
- An authenticating interceptor when the public card declares no `securitySchemes`, a security scheme only the extended card declares, or an authenticator that checks none of the declared schemes (spec §13.3)
- An extended card when the public card does not declare `capabilities.extendedAgentCard`, or `with_extended_card_provider` without `with_extended_agent_card`
- Tasks isolated by caller (the default with an authenticating interceptor) over a task or push-config store that does not partition by tenant, unless you call `share_tasks_across_callers()`; and `isolate_tasks_by_caller()` with no authenticating interceptor
- An approval gate no one could satisfy: a quorum of zero or above the approvers it names; and, with tasks isolated by caller, a quorum above one or the default of a second approver (call `share_tasks_across_callers()`, or `ApprovalGate::allowing_self_approval()` for the latter)
- A signed card (or extended card) that does not already declare every extension the builder would add, because adding one would invalidate the signature: the failure and budget extensions (`https://a2a-rust.com/extensions/failure/v1` and `https://a2a-rust.com/extensions/budget/v1`), the idempotency extension when the task store supports keys, and the lease and approval extensions when a lease store or approval gate is set
- A zero executor timeout, `max_id_length`, `max_metadata_size` or `push_delivery_timeout`
- A lease store whose longest lease (`with_max_lease_ttl`) is under one second, or a halt store with a zero `with_halt_poll_interval`
- A `Profile` whose requirements are not met

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
| `on_get_extended_agent_card(headers)` | `GetExtendedAgentCard` |
| `on_set_push_config(config, headers)` | `CreateTaskPushNotificationConfig` |
| `on_get_push_config(params, headers)` | `GetTaskPushNotificationConfig` |
| `on_list_push_configs_page(&params, headers)` | `ListTaskPushNotificationConfigs`, with `pageSize` and `pageToken`; what every dispatcher calls |
| `on_list_push_configs(task_id, tenant, headers)` | `ListTaskPushNotificationConfigs`, unpaged: every config of the task |
| `on_delete_push_config(params, headers)` | `DeleteTaskPushNotificationConfig` |

Those eleven methods are the whole protocol surface. Everything the
dispatchers do on top is decoding the wire format and encoding the reply.

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
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
    .with_agent_card(card())
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

Pass `None` for `headers` for an in-process call on a handler with no
authenticating interceptor. Interceptors still run; they simply see an empty
header set, so an authenticating one refuses the call for want of a
credential. A transport that has already authenticated the caller passes the
credential headers on, or the handler is built without the authenticating
interceptor (with `authenticated_upstream()` if its card demands a
credential). Pass header names in lowercase, as the handler looks them up.

What you give up by skipping the dispatchers is exactly what they implement:
JSON-RPC envelope parsing and error mapping, REST path and query binding, SSE
framing, the agent-card endpoint, `A2A-Version` negotiation, the body-size
and read-time limits of `DispatchConfig` and the `Content-Type` check, CORS,
the `A2A-Extensions` response header, and the connection facts they add to
the header map: the `:path` entry a `PathSegmentTenantResolver` reads, and
the peer address a `RateLimitInterceptor` keys unidentified callers by. What
you keep is every protocol guarantee — task lifecycle, idempotency, streaming, push delivery,
interceptors, multi-tenancy, and limits — because all of it lives here, below
the transport.

## Sharing the Handler

The handler is wrapped in `Arc` for sharing between dispatchers:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
use std::sync::Arc;

let handler = Arc::new(
    RequestHandlerBuilder::new(MyExecutor)
        .with_agent_card(card())
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
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# fn f(executor: MyExecutor) -> ServerResult<RequestHandler> {
use a2a_protocol_sdk::server::TaskStoreConfig;
use std::time::Duration;

let config = TaskStoreConfig::default()
    .with_task_ttl(Some(Duration::from_secs(3600))) // Finished tasks expire after 1 hour
    .with_max_capacity(Some(50_000));               // Keep at most 50k tasks

RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_task_store_config(config)
    .build()
# }
```

When capacity is exceeded, the oldest terminal tasks are evicted first, then non-terminal ones if that is not enough. Terminal tasks older than the TTL are removed by a sweep that runs every `eviction_interval` writes (64 by default).

## Custom Task Stores

For production use, implement the `TaskStore` trait for your database:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
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
    .with_agent_card(card())
    .with_task_store(MyTaskStore { /* ... */ })
    .build()
# }
```

See [Task & Config Stores](./stores.md) for the full trait API.

## Next Steps

- **[Dispatchers](./dispatchers.md)** — HTTP dispatchers for JSON-RPC and REST
- **[Interceptors & Middleware](./interceptors.md)** — Request/response hooks
- **[Task & Config Stores](./stores.md)** — Custom storage backends
