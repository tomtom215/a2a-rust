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
        drop(stores);
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

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::push::TaskPushNotificationConfig;

    fn make_config(task_id: &str, id: Option<&str>, url: &str) -> TaskPushNotificationConfig {
        TaskPushNotificationConfig {
            tenant: None,
            id: id.map(String::from),
            task_id: task_id.to_string(),
            url: url.to_string(),
            token: None,
            authentication: None,
        }
    }

    #[tokio::test]
    async fn new_store_has_zero_tenants() {
        let store = TenantAwareInMemoryPushConfigStore::new();
        assert_eq!(
            store.tenant_count().await,
            0,
            "new store should have no tenants"
        );
    }

    #[tokio::test]
    async fn set_and_get_within_tenant_scope() {
        let store = TenantAwareInMemoryPushConfigStore::new();
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://a.com/hook"))
                .await
                .expect("set should succeed");

            let config = store
                .get("task-1", "cfg-1")
                .await
                .expect("get should succeed")
                .expect("config should exist");
            assert_eq!(config.url, "https://a.com/hook");
        })
        .await;
    }

    #[tokio::test]
    async fn tenant_isolation() {
        let store = TenantAwareInMemoryPushConfigStore::new();

        // Insert config under tenant-a
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;

        // tenant-b should not see tenant-a's config
        TenantContext::scope("tenant-b", async {
            let result = store.get("task-1", "cfg-1").await.unwrap();
            assert!(
                result.is_none(),
                "tenant-b should not see tenant-a's config"
            );
        })
        .await;

        // tenant-a should still see it
        TenantContext::scope("tenant-a", async {
            let result = store.get("task-1", "cfg-1").await.unwrap();
            assert!(
                result.is_some(),
                "tenant-a should still see its own config"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn tenant_count_tracks_distinct_tenants() {
        let store = TenantAwareInMemoryPushConfigStore::new();

        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("t1", Some("c1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(store.tenant_count().await, 1);

        TenantContext::scope("tenant-b", async {
            store
                .set(make_config("t1", Some("c1"), "https://b.com"))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(store.tenant_count().await, 2);

        // Re-using tenant-a should not increase count
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("t2", Some("c2"), "https://a2.com"))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(
            store.tenant_count().await,
            2,
            "re-using an existing tenant should not increase count"
        );
    }

    #[tokio::test]
    async fn with_limits_enforces_max_tenants() {
        let store = TenantAwareInMemoryPushConfigStore::with_limits(1, 100);

        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("t1", Some("c1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;

        let err = TenantContext::scope("tenant-b", async {
            store
                .set(make_config("t1", Some("c1"), "https://b.com"))
                .await
        })
        .await
        .expect_err("second tenant should exceed max_tenants limit");

        let msg = format!("{err}");
        assert!(
            msg.contains("tenant limit exceeded"),
            "error should mention tenant limit, got: {msg}"
        );
    }

    #[tokio::test]
    async fn with_limits_enforces_per_task_config_limit() {
        let store = TenantAwareInMemoryPushConfigStore::with_limits(100, 1);

        let err = TenantContext::scope("tenant-a", async {
            store
                .set(make_config("t1", Some("c1"), "https://a.com"))
                .await
                .unwrap();
            store
                .set(make_config("t1", Some("c2"), "https://b.com"))
                .await
        })
        .await
        .expect_err("second config should exceed per-task limit");

        let msg = format!("{err}");
        assert!(
            msg.contains("limit exceeded"),
            "error should mention limit exceeded, got: {msg}"
        );
    }

    #[tokio::test]
    async fn list_scoped_to_tenant() {
        let store = TenantAwareInMemoryPushConfigStore::new();

        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("t1", Some("c1"), "https://a.com/1"))
                .await
                .unwrap();
            store
                .set(make_config("t1", Some("c2"), "https://a.com/2"))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("tenant-b", async {
            store
                .set(make_config("t1", Some("c3"), "https://b.com/1"))
                .await
                .unwrap();
        })
        .await;

        let a_list = TenantContext::scope("tenant-a", async {
            store.list("t1").await.unwrap()
        })
        .await;
        assert_eq!(
            a_list.len(),
            2,
            "tenant-a should see 2 configs for task t1"
        );

        let b_list = TenantContext::scope("tenant-b", async {
            store.list("t1").await.unwrap()
        })
        .await;
        assert_eq!(
            b_list.len(),
            1,
            "tenant-b should see 1 config for task t1"
        );
    }

    #[tokio::test]
    async fn delete_scoped_to_tenant() {
        let store = TenantAwareInMemoryPushConfigStore::new();

        // Both tenants store same task_id/config_id
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("t1", Some("c1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;
        TenantContext::scope("tenant-b", async {
            store
                .set(make_config("t1", Some("c1"), "https://b.com"))
                .await
                .unwrap();
        })
        .await;

        // Delete from tenant-a only
        TenantContext::scope("tenant-a", async {
            store.delete("t1", "c1").await.unwrap();
        })
        .await;

        // tenant-a's config is gone
        let a_result = TenantContext::scope("tenant-a", async {
            store.get("t1", "c1").await.unwrap()
        })
        .await;
        assert!(a_result.is_none(), "tenant-a config should be deleted");

        // tenant-b's config is untouched
        let b_result = TenantContext::scope("tenant-b", async {
            store.get("t1", "c1").await.unwrap()
        })
        .await;
        assert!(
            b_result.is_some(),
            "tenant-b config should be unaffected by tenant-a's delete"
        );
    }

    #[tokio::test]
    async fn default_is_same_as_new() {
        let store = TenantAwareInMemoryPushConfigStore::default();
        assert_eq!(store.tenant_count().await, 0);
        // Just verify it works
        TenantContext::scope("t", async {
            store
                .set(make_config("t1", Some("c1"), "https://x.com"))
                .await
                .unwrap();
        })
        .await;
        assert_eq!(store.tenant_count().await, 1);
    }
}
