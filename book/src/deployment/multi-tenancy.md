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
