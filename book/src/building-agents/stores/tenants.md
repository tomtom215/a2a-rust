<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Tenant-aware stores

A tenant-aware store keeps one partition per tenant, and per caller when the handler isolates callers. Behind an authenticating interceptor the handler requires one.

## Stores behind an authenticating interceptor

Behind an authenticating interceptor the handler gives each identified
caller its own partition (spec §13.1), and `build()` refuses a task store and
push-config store that do not partition by tenant. `SqliteTaskStore` and
`PostgresTaskStore` do not, so use `TenantAwareSqliteTaskStore` or
`TenantAwarePostgresTaskStore` with the matching `TenantAware…PushConfigStore`,
or call `RequestHandlerBuilder::share_tasks_across_callers()` to keep one set
of records per tenant. Left unset, both default to the tenant-aware in-memory
stores, whose per-partition capacity (`with_task_store_config`, 10,000 tasks by
default) applies per caller. See
[Multi-Tenancy](../../deployment/multi-tenancy.md#callers-within-one-tenant).

## `TenantAwareInMemoryTaskStore`

For multi-tenant deployments, use `TenantAwareInMemoryTaskStore` which provides full tenant isolation using `tokio::task_local!`:

```rust
# use a2a_protocol_sdk::types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
# #[tokio::main]
# async fn main() -> () {
# let task_id = TaskId::new("task-1");
# let task = Task { id: task_id.clone(), context_id: ContextId::new("ctx-1"),
#     status: TaskStatus::new(TaskState::Submitted), history: None, artifacts: None, metadata: None };
use a2a_protocol_server::store::{TaskStore, TenantAwareInMemoryTaskStore, TenantContext};
use std::sync::Arc;

let store = Arc::new(TenantAwareInMemoryTaskStore::new());

// Each tenant gets an independent store instance.
// Use TenantContext::scope() to set the active tenant:
TenantContext::scope("tenant-alpha".to_string(), {
    let store = store.clone();
    async move {
        store.save(&task).await.unwrap();
    }
}).await;

// Tasks saved under one tenant are invisible to others:
TenantContext::scope("tenant-beta".to_string(), {
    let store = store.clone();
    async move {
        let result = store.get(&task_id).await.unwrap();
        assert!(result.is_none()); // tenant-beta can't see tenant-alpha's task
    }
}).await;

// Track tenant count for capacity monitoring:
let count = store.tenant_count().await;
# }
```

The `TenantContext::scope()` pattern uses `tokio::task_local!` to thread the tenant ID through the async call stack without passing it as a parameter. The `RequestHandler` sets the tenant scope for every request from `params.tenant`, or from the `TenantResolver` when one is configured (the resolver's tenant wins, and a different `params.tenant` is refused).

## `TenantAwareSqliteTaskStore` (`sqlite` feature)

For persistent multi-tenant storage, enable the `sqlite` feature:

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::TenantAwareSqliteTaskStore;

let store = TenantAwareSqliteTaskStore::new("sqlite:tasks.db").await?;
# Ok(())
# }
```

This variant reads the tenant from the same `TenantContext` and stores it in a `tenant_id` column (under caller isolation, the tenant and the caller), so its data lives in the database rather than in process memory, which suits production deployments where tenants may span multiple server instances.

> **Note:** Corresponding `TenantAwareInMemoryPushConfigStore`, `TenantAwareSqlitePushConfigStore` and `TenantAwarePostgresPushConfigStore` variants exist for push notification config storage.

## Partitioning a custom store

Tenant isolation uses `tokio::task_local!` via `TenantContext::scope()`, not method parameters. For in-memory stores, `TenantAwareInMemoryTaskStore` automatically partitions data by tenant. For SQL stores, `TenantAwareSqliteTaskStore` and `TenantAwarePostgresTaskStore` partition by a `tenant_id` column. If you implement a custom store that partitions by tenant, key by `TenantContext::partition()` (the tenant, plus the caller when the handler isolates callers), and override `isolates_tenants()` to return `true` on both `TaskStore` and `PushConfigStore`. A store that leaves it `false` makes the handler refuse every request that names a tenant (unless the builder was told `accept_unisolated_tenants()`), and `build()` refuse the store behind an authenticating interceptor. A wrapper store must forward it.
