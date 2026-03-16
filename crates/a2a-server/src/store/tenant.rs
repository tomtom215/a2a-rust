// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tenant-scoped task store implementations.
//!
//! Provides [`TenantContext`] for threading tenant identity through store
//! operations and [`TenantAwareInMemoryTaskStore`] for full tenant isolation
//! without changing the [`TaskStore`] trait signature.
//!
//! # Architecture
//!
//! The A2A `TaskStore` trait doesn't accept a tenant parameter — tenant
//! identity flows through request params at the handler level. To achieve
//! tenant isolation without a breaking trait change, we use `tokio::task_local!`
//! to propagate the current tenant ID into store operations.
//!
//! The handler sets the tenant context before calling store methods:
//!
//! ```rust,no_run
//! use a2a_protocol_server::store::tenant::TenantContext;
//!
//! # async fn example(store: &impl a2a_protocol_server::store::TaskStore) {
//! let tenant = "acme-corp";
//! TenantContext::scope(tenant, async {
//!     // All store operations here are scoped to "acme-corp"
//!     // store.save(task).await;
//! }).await;
//! # }
//! ```
//!
//! # Isolation guarantees
//!
//! - Each tenant's tasks live in a separate `InMemoryTaskStore` instance
//! - Tenant A cannot read, list, or delete tenant B's tasks
//! - Per-tenant eviction configuration is supported
//! - Operations without a tenant context use the `""` (default) partition

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};
use tokio::sync::RwLock;

use super::task_store::{InMemoryTaskStore, TaskStore, TaskStoreConfig};

// ── Tenant context ──────────────────────────────────────────────────────────

tokio::task_local! {
    static CURRENT_TENANT: String;
}

/// Thread-safe tenant context for scoping store operations.
///
/// Uses `tokio::task_local!` to propagate tenant identity into store calls
/// without changing the `TaskStore` trait.
pub struct TenantContext;

impl TenantContext {
    /// Runs a future within a tenant scope.
    ///
    /// All `TenantAwareInMemoryTaskStore` operations executed within `f`
    /// will be scoped to the given tenant.
    pub async fn scope<F, R>(tenant: impl Into<String>, f: F) -> R
    where
        F: Future<Output = R>,
    {
        CURRENT_TENANT.scope(tenant.into(), f).await
    }

    /// Returns the current tenant ID, or `""` if no tenant context is set.
    pub fn current() -> String {
        CURRENT_TENANT
            .try_with(|t| t.clone())
            .unwrap_or_default()
    }
}

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
