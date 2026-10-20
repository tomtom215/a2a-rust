<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Multi-Tenancy

Serving several customers from one process, with limits that hold and data that
does not leak between them. Three parts, and they are independent: **who the
caller is**, **what they may consume**, and **where their tasks are stored**.

Getting the first one wrong makes the other two decorative, so it comes first.

## Tenant identity has to come from somewhere trustworthy

Every limit and every partition is keyed on the tenant id the handler resolved.
**A caller who can choose their own tenant id can choose their own limits and
read another tenant's partition.** That is not a subtle failure — it is the
whole of multi-tenancy, decided by one function.

Three resolvers ship:

| Resolver | Reads | Trustworthy when |
|---|---|---|
| `BearerTokenTenantResolver` | a claim in a validated bearer token | the token is verified — this is the one to reach for |
| `HeaderTenantResolver` | a request header | a proxy you control sets it and strips any inbound copy |
| `PathSegmentTenantResolver` | a URL path segment | the same, and never on its own |

`PathSegmentTenantResolver` reads the request URL on the JSON-RPC, HTTP+JSON and
WebSocket dispatchers. The gRPC dispatcher and the Axum adapter give it no URL,
so there it resolves no tenant. Before 0.14.1 it fell back to a `path` request
header, which any caller could set (GHSA-hr9h-6jvf-wvg6); upgrade.

A header or a path segment is caller-supplied input. It is a legitimate design
when something in front of the agent authenticates the caller and rewrites the
header — and a vulnerability the moment that is not true, because `curl -H
'X-Tenant: someone-else'` is the entire attack.

Write your own `TenantResolver` when identity comes from somewhere else; the
trait exists for exactly that.

## What a tenant may consume

`PerTenantConfig` is a default `TenantLimits` plus per-tenant overrides. Hand it
to `RequestHandlerBuilder::with_tenant_config`:

```rust
use std::time::Duration;
use a2a_protocol_server::tenant_config::{PerTenantConfig, TenantLimits};

let config = PerTenantConfig::builder()
    .default_limits(TenantLimits::builder()
        .max_concurrent_tasks(100)
        .rate_limit_rps(50)
        .build())
    .with_override("premium-corp", TenantLimits::builder()
        .max_concurrent_tasks(1000)
        .executor_timeout(Duration::from_secs(120))
        .rate_limit_rps(500)
        .build())
    .build();

assert_eq!(config.get("premium-corp").max_concurrent_tasks, Some(1000));
assert_eq!(config.get("unknown").max_concurrent_tasks, Some(100));
```

| Limit | Enforced | On exceeding |
|---|---|---|
| `max_concurrent_tasks` | per-tenant semaphore; the permit is taken **before any side effect** | `ServerError::Overloaded` |
| `executor_timeout` | resolved before the executor is spawned | the task fails, as with the handler-wide timeout |
| `event_queue_capacity` | at queue creation | the stream buffer is that deep |
| `rate_limit_rps` | `RateLimitInterceptor` — **opt-in** | the request is refused |

### The one that can be set and do nothing

Three of the four the handler applies by itself. `rate_limit_rps` is counted in
an interceptor, so it needs wiring:

```rust
# use a2a_protocol_server::{PerTenantConfig, RateLimitInterceptor};
# use a2a_protocol_server::rate_limit::RateLimitConfig;
# fn wire(config: PerTenantConfig) -> Result<RateLimitInterceptor, Box<dyn std::error::Error>> {
let limiter = RateLimitInterceptor::new(RateLimitConfig::default())?
    .with_tenant_config(config);
# Ok(limiter)
# }
```

Without that call the other three still apply and `rate_limit_rps` silently does
nothing. This is the only field in the design that can be set and not take
effect, which is why it is named here rather than left to be discovered in
production.

### Refused, not queued

A tenant at `max_concurrent_tasks` is **refused**. Queueing would convert a
declared bound into unbounded latency and unbounded memory — the limit would
stop meaning anything under exactly the load it exists for. If you want
queueing, build it in front of the agent where you can see its depth.

### Limits bound use; they do not reserve capacity

Process-wide resources — the event-queue manager's `max_concurrent_queues`,
handler sweep thresholds, the tenant-partition cap on the store wrappers — are
**shared pools**. A tenant running inside its own limit can still exhaust one
and cause another tenant's requests to be refused as overloaded.

Size per-tenant `max_concurrent_tasks` so the sum across active tenants stays
within the process-wide caps. Data isolation does not depend on any of this,
and is not affected by it.

## Where their tasks are stored

Tenant-aware wrappers partition by tenant id, for both task and push-config
stores:

```text
tasks          TenantAwareInMemoryTaskStore
               TenantAwareSqliteTaskStore          (feature: sqlite)
               TenantAwarePostgresTaskStore        (feature: postgres)

push configs   TenantAwareInMemoryPushConfigStore
               TenantAwareSqlitePushConfigStore    (feature: sqlite)
               TenantAwarePostgresPushConfigStore  (feature: postgres)
```

A task written under one tenant is not readable, listable or cancellable under
another. That holds regardless of the limits above being set.

### A store that cannot isolate refuses tenants

The default stores (`InMemoryTaskStore`, `SqliteTaskStore`,
`PostgresTaskStore` and their push-config counterparts) do not partition by
tenant. A request that resolves to a non-empty tenant — named by the client
or derived by a resolver — is therefore **refused** with
`UnsupportedOperation` (`-32004` on JSON-RPC, `400` on HTTP+JSON) unless both
the task store and the push-config store are tenant-aware. Requests without
a tenant are unaffected.

Before this, such a request was served from the shared records, so tenant B
could get, list, subscribe to and cancel tenant A's tasks. Configuring a
tenant-aware task store is enough: an unset push-config store then defaults
to `TenantAwareInMemoryPushConfigStore`. A store you write yourself declares
isolation by returning `true` from `TaskStore::isolates_tenants` (and
`PushConfigStore::isolates_tenants`).

Where tenants key only limits over records every caller is meant to share,
`RequestHandlerBuilder::accept_unisolated_tenants()` serves them anyway.
That is an explicit statement that tenants share data, not a default.

### The fifth limit, and why it is not on `TenantLimits`

A per-tenant cap on *stored tasks* lives on the store —
`TenantAwareInMemoryTaskStore::with_tenant_override`, which gives a named tenant
its own `TaskStoreConfig` and so its own `max_capacity`, `task_ttl`,
`eviction_interval` and `max_page_size`.

It has to live there for a structural reason: a store is constructed
independently and handed to the builder, so it never sees a `PerTenantConfig`.
`TenantLimits::max_stored_tasks` is the **deprecated** field that tried to be
this limit from the config side and that nothing could ever read. It is
deprecated rather than removed because removing a public field is a semver
break; every use site now gets a compiler warning naming the replacement.

If you set `max_stored_tasks` and wondered why nothing was capped: that is why.

### A total across every tenant

Each partition's `max_capacity` bounds that tenant, so the default store's
bound overall is `max_tenants × max_capacity`: ten million tasks at the
defaults. `TenantStoreConfig::with_max_total_tasks(n)` (since 0.15) caps the
sum. A write that would add a new task at the total first evicts the
writer's own partition's oldest finished task; when that partition holds
none, the write is refused as temporary (HTTP `503`, gRPC `UNAVAILABLE`). A
write to a task already held is never refused, another tenant's tasks are
never evicted to make room, and a running task never is.
`TenantAwareInMemoryTaskStore::total_tasks()` reports the count. The SQL
tenant stores have no total; size their tables with the database.

```rust
use a2a_protocol_server::{TenantAwareInMemoryTaskStore, TenantStoreConfig};

let store = TenantAwareInMemoryTaskStore::with_config(
    TenantStoreConfig::default().with_max_total_tasks(100_000),
);
assert_eq!(store.total_tasks(), Some(0));
```

## Callers within one tenant

A tenant is a boundary between organisations, not between the people in one.
Spec §13.1 requires results scoped to the caller's authorization boundaries,
so **a handler with an authenticating interceptor gives each caller it
identifies a partition of its own by default** (since 0.15; before it, every
caller in a tenant saw the same tasks, and Bob could read, list, cancel and
subscribe to Alice's, and register a webhook on them):

* The partition is chosen by the identity the authenticating interceptor sets
  (`BearerTokenAuthInterceptor::with_labelled_tokens` as `bearer:<label>`,
  `ApiKeyAuthInterceptor::with_labelled_keys` as `api-key:<label>`, the JWT
  interceptor as `jwt:<issuer>:<sub>`, since 0.15, so one `sub` from two
  issuers is two callers; a custom interceptor calls
  `CallContext::set_caller_identity`). Another
  caller's task is `TaskNotFound`, the same answer as an id that does not
  exist, so its existence is not revealed, and `ListTasks` returns only the
  caller's own.
* A request the interceptor lets through without naming anyone (an unlabelled
  token or key) keeps the tenant's shared records, as before. A JWT without
  `sub` is refused with `PermissionDenied` instead, since 0.15. Those records are never an identified caller's, and an identified
  caller's are never theirs.
* The halt switch, the tenant limits above, audit chains and metrics still see
  one tenant. Each caller's partition counts against the store's tenant limit:
  the in-memory store refuses new partitions past `max_tenants` (1,000 by
  default) until old ones empty; the SQL tenant stores have no such limit.
  Each partition also gets the whole per-tenant capacity (10,000 tasks by
  default, or what `with_task_store_config` sets), so the default in-memory
  store holds up to 1,000 times that. To bound it otherwise, supply a
  `TenantAwareInMemoryTaskStore` with your own `TenantStoreConfig`, or use a
  SQL store.
* Isolation needs stores that partition by tenant. Left unset they default to
  the tenant-aware in-memory stores; `build()` refuses a store you supplied
  that does not partition, naming the two ways out.
* `isolate_tasks_by_caller()` says it explicitly and requires every caller to
  be identified: a request with no identity is refused, and `build()` refuses
  a chain with no authenticating interceptor.
* An [approval gate](./oversight.md) that wants an approver other than the
  requester cannot work while tasks are isolated, since the approver could
  never reach the requester's task; `build()` refuses the pair and names the
  two ways out (share tasks, or allow self-approval).
* Callers who are meant to share tasks call `share_tasks_across_callers()`.
  Where the sharing group is known (a team, a project), make the group the
  tenant instead, with a resolver that reads it from the verified identity,
  so other groups stay out.
* A handler with no authenticating interceptor has no identities to keep apart
  and shares, as before.

### Erasing a caller

Spec §13.4 says agents SHOULD let users ask for their data to be deleted.
With callers isolated, a caller's data is exactly its partitions, so every
tenant-aware store has `erase_caller(identity)`: it removes that caller's
partitions in every tenant, with their tasks, logged events and idempotency
keys (task stores) or webhooks (push-config stores), in one transaction on
the SQL stores, and reports how many partitions and rows went. The identity
is the one the interceptor set, such as `bearer:alice` or
`jwt:<issuer>:<sub>` (`auth::caller_id` builds them). Keep a handle to each
store (`with_task_store_arc`, `with_push_config_store_arc`) to call it.

```rust
# async fn run() {
use std::sync::Arc;
use a2a_protocol_server::push::TenantAwareInMemoryPushConfigStore;
use a2a_protocol_server::store::TenantAwareInMemoryTaskStore;

let tasks = Arc::new(TenantAwareInMemoryTaskStore::new());
let webhooks = Arc::new(TenantAwareInMemoryPushConfigStore::new());
// ... builder.with_task_store_arc(tasks.clone()).with_push_config_store_arc(webhooks.clone())

let caller = a2a_protocol_server::auth::caller_id::bearer("alice");
let removed = tasks.erase_caller(&caller).await;
let _ = webhooks.erase_caller(&caller).await;
println!("{} partitions, {} rows", removed.partitions, removed.rows);
# }
```

It does not reach records the caller left in the tenant's shared partition
(an unlabelled credential, or `share_tasks_across_callers`), which name no
one; audit records, which `AuditLog::erase_subject` handles; or copies in
backups and logs.

## What is not isolated

* **CPU and memory are shared.** A tenant inside its task limit can still run
  expensive executors. `executor_timeout` bounds duration, not cost.
* **The process is shared.** A panic in one tenant's executor is caught, but
  memory exhaustion is not something the limits above prevent.
* **There is no per-tenant metric dimension.** A tenant hitting its ceiling
  shows up as `Overloaded` in the aggregate — see
  [Observability](./observability.md).
* **Agent cards are not per-tenant.** One process serves one card.

See also [Security](./security.md) for the identity half of this, and
[Running More Than One Replica](./horizontal-scaling.md) for the store choices
when partitions must be shared across processes.
