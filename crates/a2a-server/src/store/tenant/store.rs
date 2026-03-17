// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tenant-isolated in-memory task store implementation.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};
use tokio::sync::RwLock;

use super::context::TenantContext;
use super::super::task_store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};

// ── TenantAwareInMemoryTaskStore ────────────────────────────────────────────

/// Configuration for [`TenantAwareInMemoryTaskStore`].
#[derive(Debug, Clone)]
pub struct TenantStoreConfig {
    /// Per-tenant store configuration.
    pub per_tenant: TaskStoreConfig,

    /// Maximum number of tenants allowed. Prevents unbounded memory growth
    /// from tenant enumeration attacks. Default: 1000.
    pub max_tenants: usize,
}

impl Default for TenantStoreConfig {
    fn default() -> Self {
        Self {
            per_tenant: TaskStoreConfig::default(),
            max_tenants: 1000,
        }
    }
}

/// Tenant-isolated in-memory [`TaskStore`].
///
/// Maintains a separate [`InMemoryTaskStore`] per tenant, providing full
/// data isolation between tenants. The current tenant is determined from
/// [`TenantContext`].
///
/// # Usage
///
/// ```rust,no_run
/// use a2a_protocol_server::store::tenant::{TenantAwareInMemoryTaskStore, TenantContext};
/// use a2a_protocol_server::store::TaskStore;
/// # use a2a_protocol_types::task::{Task, TaskId, ContextId, TaskState, TaskStatus};
///
/// # async fn example() {
/// let store = TenantAwareInMemoryTaskStore::new();
///
/// // Tenant A saves a task
/// TenantContext::scope("tenant-a", async {
///     let task = Task {
///         id: TaskId::new("task-1"),
///         context_id: ContextId::new("ctx-1"),
///         status: TaskStatus::with_timestamp(TaskState::Submitted),
///         history: None,
///         artifacts: None,
///         metadata: None,
///     };
///     store.save(task).await.unwrap();
/// }).await;
///
/// // Tenant B cannot see tenant A's task
/// TenantContext::scope("tenant-b", async {
///     let result = store.get(&TaskId::new("task-1")).await.unwrap();
///     assert!(result.is_none());
/// }).await;
/// # }
/// ```
#[derive(Debug)]
pub struct TenantAwareInMemoryTaskStore {
    stores: RwLock<HashMap<String, Arc<InMemoryTaskStore>>>,
    config: TenantStoreConfig,
}

impl Default for TenantAwareInMemoryTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantAwareInMemoryTaskStore {
    /// Creates a new tenant-aware store with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stores: RwLock::new(HashMap::new()),
            config: TenantStoreConfig::default(),
        }
    }

    /// Creates a new tenant-aware store with custom configuration.
    #[must_use]
    pub fn with_config(config: TenantStoreConfig) -> Self {
        Self {
            stores: RwLock::new(HashMap::new()),
            config,
        }
    }

    /// Returns the store for the current tenant, creating it if needed.
    async fn get_store(&self) -> A2aResult<Arc<InMemoryTaskStore>> {
        let tenant = TenantContext::current();

        // Fast path: check if store already exists.
        {
            let stores = self.stores.read().await;
            if let Some(store) = stores.get(&tenant) {
                return Ok(Arc::clone(store));
            }
        }

        // Slow path: create a new store for this tenant.
        let mut stores = self.stores.write().await;
        // Double-check after acquiring write lock.
        if let Some(store) = stores.get(&tenant) {
            return Ok(Arc::clone(store));
        }

        if stores.len() >= self.config.max_tenants {
            return Err(a2a_protocol_types::error::A2aError::internal(format!(
                "tenant limit exceeded: max {} tenants",
                self.config.max_tenants
            )));
        }

        let store = Arc::new(InMemoryTaskStore::with_config(
            self.config.per_tenant.clone(),
        ));
        stores.insert(tenant, Arc::clone(&store));
        drop(stores);
        Ok(store)
    }

    /// Returns the number of active tenant partitions.
    pub async fn tenant_count(&self) -> usize {
        self.stores.read().await.len()
    }

    /// Runs eviction on all tenant stores.
    ///
    /// Call periodically to clean up terminal tasks in idle tenants.
    pub async fn run_eviction_all(&self) {
        let stores = self.stores.read().await;
        for store in stores.values() {
            store.run_eviction().await;
        }
    }

    /// Removes empty tenant partitions to reclaim memory.
    ///
    /// A partition is considered empty when its task count is zero.
    pub async fn prune_empty_tenants(&self) {
        let mut stores = self.stores.write().await;
        let mut empty_tenants = Vec::new();
        for (tenant, store) in stores.iter() {
            if store.count().await.unwrap_or(0) == 0 {
                empty_tenants.push(tenant.clone());
            }
        }
        for tenant in empty_tenants {
            stores.remove(&tenant);
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for TenantAwareInMemoryTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.save(task).await
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.get(id).await
        })
    }

    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.list(params).await
        })
    }

    fn insert_if_absent<'a>(
        &'a self,
        task: Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.insert_if_absent(task).await
        })
    }

    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.delete(id).await
        })
    }

    fn count<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.count().await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

    /// Helper to create a task with the given ID and state.
    fn make_task(id: &str, state: TaskState) -> Task {
        Task {
            id: TaskId::new(id),
            context_id: ContextId::new("ctx-default"),
            status: TaskStatus::new(state),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    // ── TenantContext ────────────────────────────────────────────────────

    #[tokio::test]
    async fn tenant_context_default_is_empty_string() {
        // Outside any scope, current() should return "".
        let tenant = TenantContext::current();
        assert_eq!(tenant, "", "default tenant should be empty string");
    }

    #[tokio::test]
    async fn tenant_context_scope_sets_and_restores() {
        let before = TenantContext::current();
        assert_eq!(before, "");

        let inside = TenantContext::scope("acme", async { TenantContext::current() }).await;
        assert_eq!(inside, "acme", "scope should set the tenant");

        let after = TenantContext::current();
        assert_eq!(after, "", "tenant should revert after scope exits");
    }

    #[tokio::test]
    async fn tenant_context_nested_scopes() {
        TenantContext::scope("outer", async {
            assert_eq!(TenantContext::current(), "outer");
            TenantContext::scope("inner", async {
                assert_eq!(TenantContext::current(), "inner");
            })
            .await;
            assert_eq!(
                TenantContext::current(),
                "outer",
                "should restore outer tenant after inner scope"
            );
        })
        .await;
    }

    // ── TenantAwareInMemoryTaskStore isolation ──────────────────────────

    #[tokio::test]
    async fn tenant_isolation_save_and_get() {
        let store = TenantAwareInMemoryTaskStore::new();

        // Tenant A saves a task.
        TenantContext::scope("tenant-a", async {
            store
                .save(make_task("t1", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;

        // Tenant A can retrieve it.
        let found = TenantContext::scope("tenant-a", async {
            store.get(&TaskId::new("t1")).await.unwrap()
        })
        .await;
        assert!(found.is_some(), "tenant-a should see its own task");

        // Tenant B cannot see it.
        let not_found = TenantContext::scope("tenant-b", async {
            store.get(&TaskId::new("t1")).await.unwrap()
        })
        .await;
        assert!(
            not_found.is_none(),
            "tenant-b should not see tenant-a's task"
        );
    }

    #[tokio::test]
    async fn tenant_isolation_list() {
        let store = TenantAwareInMemoryTaskStore::new();

        TenantContext::scope("alpha", async {
            store
                .save(make_task("a1", TaskState::Submitted))
                .await
                .unwrap();
            store
                .save(make_task("a2", TaskState::Working))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("beta", async {
            store
                .save(make_task("b1", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;

        let alpha_list = TenantContext::scope("alpha", async {
            let params = ListTasksParams::default();
            store.list(&params).await.unwrap()
        })
        .await;
        assert_eq!(
            alpha_list.tasks.len(),
            2,
            "alpha should see only its 2 tasks"
        );

        let beta_list = TenantContext::scope("beta", async {
            let params = ListTasksParams::default();
            store.list(&params).await.unwrap()
        })
        .await;
        assert_eq!(beta_list.tasks.len(), 1, "beta should see only its 1 task");
    }

    #[tokio::test]
    async fn tenant_isolation_delete() {
        let store = TenantAwareInMemoryTaskStore::new();

        TenantContext::scope("tenant-a", async {
            store
                .save(make_task("t1", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;

        // Tenant B deleting "t1" should not affect tenant A.
        TenantContext::scope("tenant-b", async {
            store.delete(&TaskId::new("t1")).await.unwrap();
        })
        .await;

        let still_exists = TenantContext::scope("tenant-a", async {
            store.get(&TaskId::new("t1")).await.unwrap()
        })
        .await;
        assert!(
            still_exists.is_some(),
            "tenant-a's task should survive tenant-b's delete"
        );
    }

    #[tokio::test]
    async fn tenant_isolation_insert_if_absent() {
        let store = TenantAwareInMemoryTaskStore::new();

        // Same task ID in different tenants should both succeed.
        let inserted_a = TenantContext::scope("tenant-a", async {
            store
                .insert_if_absent(make_task("shared-id", TaskState::Submitted))
                .await
                .unwrap()
        })
        .await;
        assert!(inserted_a, "tenant-a insert should succeed");

        let inserted_b = TenantContext::scope("tenant-b", async {
            store
                .insert_if_absent(make_task("shared-id", TaskState::Working))
                .await
                .unwrap()
        })
        .await;
        assert!(
            inserted_b,
            "tenant-b insert of same ID should also succeed (different partition)"
        );
    }

    #[tokio::test]
    async fn tenant_isolation_count() {
        let store = TenantAwareInMemoryTaskStore::new();

        TenantContext::scope("x", async {
            store
                .save(make_task("t1", TaskState::Submitted))
                .await
                .unwrap();
            store
                .save(make_task("t2", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("y", async {
            store
                .save(make_task("t3", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;

        let count_x = TenantContext::scope("x", async { store.count().await.unwrap() }).await;
        assert_eq!(count_x, 2, "tenant x should have 2 tasks");

        let count_y = TenantContext::scope("y", async { store.count().await.unwrap() }).await;
        assert_eq!(count_y, 1, "tenant y should have 1 task");
    }

    // ── tenant_count and max_tenants ─────────────────────────────────────

    #[tokio::test]
    async fn tenant_count_reflects_active_tenants() {
        let store = TenantAwareInMemoryTaskStore::new();
        assert_eq!(store.tenant_count().await, 0);

        TenantContext::scope("a", async {
            store
                .save(make_task("t1", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(store.tenant_count().await, 1);

        TenantContext::scope("b", async {
            store
                .save(make_task("t2", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(store.tenant_count().await, 2);
    }

    #[tokio::test]
    async fn max_tenants_limit_enforced() {
        let config = TenantStoreConfig {
            per_tenant: TaskStoreConfig::default(),
            max_tenants: 2,
        };
        let store = TenantAwareInMemoryTaskStore::with_config(config);

        // Fill up to the limit.
        TenantContext::scope("t1", async {
            store
                .save(make_task("task-a", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;
        TenantContext::scope("t2", async {
            store
                .save(make_task("task-b", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;

        // Third tenant should be rejected.
        let result = TenantContext::scope("t3", async {
            store.save(make_task("task-c", TaskState::Submitted)).await
        })
        .await;
        assert!(
            result.is_err(),
            "exceeding max_tenants should return an error"
        );
    }

    #[tokio::test]
    async fn existing_tenant_does_not_count_against_limit() {
        let config = TenantStoreConfig {
            per_tenant: TaskStoreConfig::default(),
            max_tenants: 1,
        };
        let store = TenantAwareInMemoryTaskStore::with_config(config);

        TenantContext::scope("only", async {
            store
                .save(make_task("t1", TaskState::Submitted))
                .await
                .unwrap();
            // Second save to existing tenant should work fine.
            store
                .save(make_task("t2", TaskState::Working))
                .await
                .unwrap();
        })
        .await;

        let count = TenantContext::scope("only", async { store.count().await.unwrap() }).await;
        assert_eq!(count, 2, "existing tenant can add more tasks");
    }

    // ── Default tenant (empty string) ────────────────────────────────────

    #[tokio::test]
    async fn no_tenant_context_uses_default_partition() {
        let store = TenantAwareInMemoryTaskStore::new();

        // No TenantContext::scope — should use "" as tenant.
        store
            .save(make_task("default-task", TaskState::Submitted))
            .await
            .unwrap();

        let fetched = store.get(&TaskId::new("default-task")).await.unwrap();
        assert!(
            fetched.is_some(),
            "task saved without tenant context should be retrievable without context"
        );

        // Should NOT be visible to a named tenant.
        let not_found = TenantContext::scope("other", async {
            store.get(&TaskId::new("default-task")).await.unwrap()
        })
        .await;
        assert!(
            not_found.is_none(),
            "default partition task should not leak to named tenants"
        );
    }

    // ── prune_empty_tenants ──────────────────────────────────────────────

    #[tokio::test]
    async fn prune_empty_tenants_removes_empty_partitions() {
        let store = TenantAwareInMemoryTaskStore::new();

        TenantContext::scope("keep", async {
            store
                .save(make_task("t1", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;
        TenantContext::scope("remove", async {
            store
                .save(make_task("t2", TaskState::Submitted))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(store.tenant_count().await, 2);

        // Delete all tasks from the "remove" tenant.
        TenantContext::scope("remove", async {
            store.delete(&TaskId::new("t2")).await.unwrap();
        })
        .await;

        store.prune_empty_tenants().await;
        assert_eq!(
            store.tenant_count().await,
            1,
            "empty tenant partition should be pruned"
        );
    }

    // ── Config defaults ──────────────────────────────────────────────────

    #[test]
    fn default_tenant_store_config() {
        let cfg = TenantStoreConfig::default();
        assert_eq!(cfg.max_tenants, 1000);
    }
}
