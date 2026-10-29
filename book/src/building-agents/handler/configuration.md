<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Configuring the handler

`RequestHandlerBuilder` has a method for each thing the handler can be told:
where tasks are stored, who may call, how long a run may take, how much a
stream may buffer. This page shows a full configuration, says what each
method is for, and lists what `build()` refuses. The defaults are in one
place, the [Configuration Reference](../../reference/configuration.md#requesthandlerbuilder);
this page links there rather than repeating them.

## A full configuration

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

## What each builder method is for

Defaults for every method below are in the
[Configuration Reference](../../reference/configuration.md#requesthandlerbuilder).

### Discovery

| Method | What it is for |
|--------|----------------|
| `new(executor)` | The agent executor (type-erased to `Arc<dyn AgentExecutor>`) |
| `with_agent_card(AgentCard)` | Discovery card for `/.well-known/agent-card.json`; the handler checks requests against what it declares. Required: `build()` refuses a handler with no card (spec §8.1) unless you call `without_agent_card()` |
| `without_agent_card()` | Build without a card: for a card published elsewhere, or a handler that is not a public A2A server, such as one in a test |
| `with_extended_agent_card(AgentCard)` | What `GetExtendedAgentCard` serves; without it a card declaring `extendedAgentCard` answers `ExtendedAgentCardNotConfiguredError` |
| `with_extended_card_provider(impl ExtendedCardProvider)` | Chooses each caller's extended card from the one set with `with_extended_agent_card` |
| `allow_unauthenticated_extended_card()` | Serve `GetExtendedAgentCard` even when no authenticating interceptor is registered (spec §13.3 requires one) |

### Storage and push

| Method | What it is for |
|--------|----------------|
| `with_task_store(impl TaskStore)` | A custom task storage backend ([Task & Config Stores](../stores.md)) |
| `with_task_store_arc(Arc<dyn TaskStore>)` | The same, sharing a store instance you hold elsewhere |
| `with_task_store_config(TaskStoreConfig)` | TTL and capacity for the default in-memory store ([below](#task-storage)); per caller partition when tasks are isolated by caller |
| `with_push_config_store(impl PushConfigStore)` | Custom push config storage |
| `with_push_sender(impl PushSender)` | Webhook delivery. The push-config operations need it and a card that declares `capabilities.pushNotifications: true`; `build()` refuses a card that declares push with no sender (spec §3.3.4) |

### Who may call, and whose tasks they see

| Method | What it is for |
|--------|----------------|
| `with_interceptor(impl ServerInterceptor)` | Add a server interceptor to the chain ([Interceptors & Middleware](../interceptors.md)) |
| `authenticated_upstream()` | Accept a card whose `securityRequirements` demand a credential although no interceptor here authenticates, because a gateway in front does (spec §3.3.2, §7.4) |
| `with_tenant_resolver(impl TenantResolver)` | Derive each request's tenant |
| `with_tenant_config(PerTenantConfig)` | Per-tenant concurrency, executor timeout and queue capacity. `rate_limit_rps` additionally needs the same config on `RateLimitInterceptor::with_tenant_config` |
| `require_resolved_tenant()` | Refuse a request the configured tenant resolver cannot place, instead of using the shared default partition |
| `isolate_tasks_by_caller()` | A partition per caller within a tenant (spec §13.1). On by default for the callers an authenticating interceptor identifies; calling it also refuses a request with no identity. See [Multi-tenancy](../../deployment/multi-tenancy.md#callers-within-one-tenant) |
| `share_tasks_across_callers()` | Every caller in a tenant sees its tasks, and `build()` accepts task and push-config stores that do not partition by tenant (a request that names a tenant is still refused over them unless `accept_unisolated_tenants()` is also called) |
| `accept_unisolated_tenants()` | Serve requests that name a tenant even though the stores do not partition by tenant; by default they are refused with `UnsupportedOperation`. See [Multi-tenancy](../../deployment/multi-tenancy.md#a-store-that-cannot-isolate-refuses-tenants) |

### Limits and timeouts

| Method | What it is for |
|--------|----------------|
| `with_executor_timeout(Duration)` | Bound each executor run; a run past it fails the task (see [Executor timeout](../executor/patterns.md#executor-timeout)) |
| `without_executor_timeout()` | Remove the bound (an executor that never returns then pins its task, queue and cancellation token) |
| `with_event_queue_capacity(usize)` | Bounded channel size per event queue |
| `with_max_event_size(usize)` | Maximum serialized event size |
| `with_max_concurrent_streams(usize)` | Limit concurrent event queues: one per task being run, whether its send blocks or streams; a send that would open one more is refused as overloaded (pass `usize::MAX` to disable) |
| `with_handler_limits(HandlerLimits)` | Validation and housekeeping bounds ([below](#handlerlimits)) |

### What a message may carry

| Method | What it is for |
|--------|----------------|
| `allow_undeclared_input_modes()` | When the card declares `defaultInputModes` or any skill's `inputModes`, `SendMessage` refuses a part whose explicit `mediaType` is outside them with `ContentTypeNotSupportedError` (-32005, HTTP 400). Parts without a `mediaType`, and cards declaring no modes, are not checked. This turns the check off |
| `refuse_private_file_urls()` | Refuse a message whose file `url` points into a private, loopback, link-local or internal network |

### Metrics and traces

| Method | What it is for |
|--------|----------------|
| `with_metrics(impl Metrics)` | Metrics observer for handler activity; replaces the default |
| `with_additional_metrics(impl Metrics)` | Also report to this observer, keeping the default |
| `with_inbound_trace_policy(InboundTracePolicy)` | What to do with a `traceparent` sent by a not-yet-authenticated peer: `Continue` joins its trace, `Restart` mints a new one, `Drop` traces nothing |
| `with_agent_span_conventions(bool)` | Whether the executor's span is the `GenAI` conventions' `invoke_agent {name}` (on) or `a2a.execute` (off). Turn it off when a framework inside the executor reports its own `invoke_agent` span |
| `with_span_content_capture(bool)` | Whether the executor's span records the message and replies as `gen_ai.input.messages` / `gen_ai.output.messages` (off by default: it copies what users say into your traces) |

See [Observability](../../deployment/observability.md) for what these record.

### Features documented elsewhere

| Methods | What they attach | Page |
|---------|------------------|------|
| `with_approval_gate` | Checks approval decisions before the executor sees them | [Human Oversight](../../deployment/oversight.md) |
| `with_halt_store`, `with_halt_poll_interval` | A halt shared across replicas and kept across restarts | [Human Oversight](../../deployment/oversight.md) |
| `with_audit`, `with_profile` | A hash-chained audit log; a deployment profile and its requirements | [Audit Trail](../../deployment/audit.md), [Production Hardening](../../deployment/production.md) |
| `with_content_signer`, `with_content_keys`, `require_signed_content` | Signing what the agent sends; verifying what it is sent | [Provenance and Signatures](../../deployment/provenance.md) |
| `with_lease_store`, `with_max_lease_ttl` | Leases on delegated tasks | [Horizontal Scaling](../../deployment/horizontal-scaling.md) |
| `with_turn_claims`, `without_turn_claims`, `with_turn_claim_timing`, `with_replica_name` | One replica at a time runs a task | [Horizontal Scaling](../../deployment/horizontal-scaling.md) |

## HandlerLimits

The `HandlerLimits` struct configures per-handler bounds. It is
`#[non_exhaustive]`: start from `HandlerLimits::default()` and set fields with
its `with_*` methods (for example
`HandlerLimits::default().with_max_id_length(2048)`), then pass it to
`with_handler_limits`. The full field list, with defaults, is in the
[Configuration Reference](../../reference/configuration.md#handlerlimits).

The one with the most visible effect is `executor_drain_timeout`. It bounds
how long a blocking `SendMessage` waits for the event queue to close after the
executor finished, and how long a continuation waits for a parked
(`input-required` / `auth-required`) turn to finish. A turn still running
after that refuses the continuation with `UnsupportedOperation`; see
[Asking for authorization in the task](../executor/interrupted.md#asking-for-authorization-in-the-task).

## Build-time validation

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

## Task storage

### The default store's TTL and capacity

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

### Custom task stores

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

See [Task & Config Stores](../stores/custom.md) for the full trait API.
