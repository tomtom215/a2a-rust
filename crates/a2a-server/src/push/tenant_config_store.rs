// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tenant-scoped push notification config store.
//!
//! Mirrors the design of [`crate::store::tenant::TenantAwareInMemoryTaskStore`]:
//! uses [`TenantContext`] to partition push configs by tenant.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use tokio::sync::RwLock;

use super::config_store::{InMemoryPushConfigStore, PushConfigStore};
use crate::store::tenant::TenantContext;

/// Tenant-isolated in-memory [`PushConfigStore`].
///
/// Maintains a separate [`InMemoryPushConfigStore`] per tenant for full
/// data isolation. The current tenant is determined from [`TenantContext`].
///
/// # Example
///
/// ```rust,no_run
/// use a2a_protocol_server::push::tenant_config_store::TenantAwareInMemoryPushConfigStore;
/// use a2a_protocol_server::push::PushConfigStore;
/// use a2a_protocol_server::store::tenant::TenantContext;
///
/// # async fn example() {
/// let store = TenantAwareInMemoryPushConfigStore::new();
///
/// // Scoped to tenant A
/// TenantContext::scope("tenant-a", async {
///     // store.set(config).await;
/// }).await;
/// # }
/// ```
#[derive(Debug)]
pub struct TenantAwareInMemoryPushConfigStore {
    stores: RwLock<HashMap<String, Arc<InMemoryPushConfigStore>>>,
    max_tenants: usize,
    max_configs_per_task: usize,
}

impl Default for TenantAwareInMemoryPushConfigStore {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantAwareInMemoryPushConfigStore {
    /// Creates a new tenant-aware push config store with default limits.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stores: RwLock::new(HashMap::new()),
            max_tenants: 1000,
            max_configs_per_task: 100,
        }
    }

    /// Creates with custom limits.
    #[must_use]
    pub fn with_limits(max_tenants: usize, max_configs_per_task: usize) -> Self {
        Self {
            stores: RwLock::new(HashMap::new()),
            max_tenants,
            max_configs_per_task,
        }
    }

    /// Returns the store for the current tenant, creating if needed.
    async fn get_store(&self) -> A2aResult<Arc<InMemoryPushConfigStore>> {
        let tenant = TenantContext::current();

        {
            let stores = self.stores.read().await;
            if let Some(store) = stores.get(&tenant) {
                return Ok(Arc::clone(store));
            }
        }

        let mut stores = self.stores.write().await;
        if let Some(store) = stores.get(&tenant) {
            return Ok(Arc::clone(store));
        }

        if stores.len() >= self.max_tenants {
            return Err(a2a_protocol_types::error::A2aError::internal(format!(
                "tenant limit exceeded: max {} tenants",
                self.max_tenants
            )));
        }

        let store = Arc::new(InMemoryPushConfigStore::with_max_configs_per_task(
            self.max_configs_per_task,
        ));
        stores.insert(tenant, Arc::clone(&store));
        Ok(store)
    }

    /// Returns the number of active tenant partitions.
    pub async fn tenant_count(&self) -> usize {
        self.stores.read().await.len()
    }
}

#[allow(clippy::manual_async_fn)]
impl PushConfigStore for TenantAwareInMemoryPushConfigStore {
    fn set<'a>(
        &'a self,
        config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.set(config).await
        })
    }

    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>
    {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.get(task_id, id).await
        })
    }

    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.list(task_id).await
        })
    }

    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.get_store().await?;
            store.delete(task_id, id).await
        })
    }
}
