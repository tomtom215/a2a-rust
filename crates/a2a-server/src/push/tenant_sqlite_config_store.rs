// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tenant-scoped SQLite-backed [`PushConfigStore`] implementation.
//!
//! Adds a `tenant_id` column to the `push_configs` table for full tenant
//! isolation. Uses [`TenantContext`] to scope all operations.
//!
//! Requires the `sqlite` feature flag.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use sqlx::sqlite::SqlitePool;

use super::config_store::PushConfigStore;
use crate::store::tenant::TenantContext;

/// Tenant-scoped SQLite-backed [`PushConfigStore`].
///
/// Each operation is scoped to the tenant from [`TenantContext`].
///
/// # Schema
///
/// ```sql
/// CREATE TABLE IF NOT EXISTS tenant_push_configs (
///     tenant_id TEXT NOT NULL DEFAULT '',
///     task_id   TEXT NOT NULL,
///     id        TEXT NOT NULL,
///     data      TEXT NOT NULL,
///     PRIMARY KEY (tenant_id, task_id, id)
/// );
/// ```
#[derive(Debug, Clone)]
pub struct TenantAwareSqlitePushConfigStore {
    pool: SqlitePool,
}

fn to_a2a_error(e: &sqlx::Error) -> A2aError {
    A2aError::internal(format!("sqlite error: {e}"))
}

impl TenantAwareSqlitePushConfigStore {
    /// Opens (or creates) a `SQLite` database and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or migration fails.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(4)
            .connect(url)
            .await?;
        Self::from_pool(pool).await
    }

    /// Creates a store from an existing connection pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema migration fails.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tenant_push_configs (
                tenant_id TEXT NOT NULL DEFAULT '',
                task_id   TEXT NOT NULL,
                id        TEXT NOT NULL,
                data      TEXT NOT NULL,
                PRIMARY KEY (tenant_id, task_id, id)
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

#[allow(clippy::manual_async_fn)]
impl PushConfigStore for TenantAwareSqlitePushConfigStore {
    fn set<'a>(
        &'a self,
        mut config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let id = config
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            config.id = Some(id.clone());

            let data = serde_json::to_string(&config)
                .map_err(|e| A2aError::internal(format!("serialize: {e}")))?;

            sqlx::query(
                "INSERT INTO tenant_push_configs (tenant_id, task_id, id, data)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(tenant_id, task_id, id) DO UPDATE SET data = excluded.data",
            )
            .bind(&tenant)
            .bind(&config.task_id)
            .bind(&id)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            Ok(config)
        })
    }

    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>
    {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT data FROM tenant_push_configs WHERE tenant_id = ?1 AND task_id = ?2 AND id = ?3",
            )
            .bind(&tenant)
            .bind(task_id)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            match row {
                Some((data,)) => {
                    let config: TaskPushNotificationConfig = serde_json::from_str(&data)
                        .map_err(|e| A2aError::internal(format!("deserialize: {e}")))?;
                    Ok(Some(config))
                }
                None => Ok(None),
            }
        })
    }

    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT data FROM tenant_push_configs WHERE tenant_id = ?1 AND task_id = ?2",
            )
            .bind(&tenant)
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            rows.into_iter()
                .map(|(data,)| {
                    serde_json::from_str(&data)
                        .map_err(|e| A2aError::internal(format!("deserialize: {e}")))
                })
                .collect()
        })
    }

    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            sqlx::query(
                "DELETE FROM tenant_push_configs WHERE tenant_id = ?1 AND task_id = ?2 AND id = ?3",
            )
            .bind(&tenant)
            .bind(task_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::push::TaskPushNotificationConfig;

    async fn make_store() -> TenantAwareSqlitePushConfigStore {
        TenantAwareSqlitePushConfigStore::new("sqlite::memory:")
            .await
            .expect("failed to create in-memory tenant push config store")
    }

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
    async fn set_and_get_within_tenant() {
        let store = make_store().await;
        TenantContext::scope("acme", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://example.com"))
                .await
                .unwrap();
            let config = store.get("task-1", "cfg-1").await.unwrap();
            assert!(
                config.is_some(),
                "config should be retrievable within its tenant"
            );
            assert_eq!(config.unwrap().url, "https://example.com");
        })
        .await;
    }

    #[tokio::test]
    async fn tenant_isolation_get() {
        let store = make_store().await;
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("tenant-b", async {
            let result = store.get("task-1", "cfg-1").await.unwrap();
            assert!(
                result.is_none(),
                "tenant-b should not see tenant-a's config"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn tenant_isolation_list() {
        let store = make_store().await;
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("task-1", Some("c1"), "https://a.com/1"))
                .await
                .unwrap();
            store
                .set(make_config("task-1", Some("c2"), "https://a.com/2"))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("tenant-b", async {
            store
                .set(make_config("task-1", Some("c3"), "https://b.com/1"))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("tenant-a", async {
            let configs = store.list("task-1").await.unwrap();
            assert_eq!(configs.len(), 2, "tenant-a should see only its 2 configs");
        })
        .await;

        TenantContext::scope("tenant-b", async {
            let configs = store.list("task-1").await.unwrap();
            assert_eq!(configs.len(), 1, "tenant-b should see only its 1 config");
        })
        .await;
    }

    #[tokio::test]
    async fn tenant_isolation_delete() {
        let store = make_store().await;
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;

        // Delete from tenant-b should not affect tenant-a's config
        TenantContext::scope("tenant-b", async {
            store.delete("task-1", "cfg-1").await.unwrap();
        })
        .await;

        TenantContext::scope("tenant-a", async {
            let config = store.get("task-1", "cfg-1").await.unwrap();
            assert!(
                config.is_some(),
                "tenant-a's config should survive tenant-b's delete"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn same_keys_different_tenants() {
        let store = make_store().await;
        TenantContext::scope("tenant-a", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://a.com"))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("tenant-b", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://b.com"))
                .await
                .unwrap();
        })
        .await;

        TenantContext::scope("tenant-a", async {
            let config = store.get("task-1", "cfg-1").await.unwrap().unwrap();
            assert_eq!(
                config.url, "https://a.com",
                "tenant-a should get its own config"
            );
        })
        .await;

        TenantContext::scope("tenant-b", async {
            let config = store.get("task-1", "cfg-1").await.unwrap().unwrap();
            assert_eq!(
                config.url, "https://b.com",
                "tenant-b should get its own config"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn overwrite_within_tenant() {
        let store = make_store().await;
        TenantContext::scope("acme", async {
            store
                .set(make_config("task-1", Some("cfg-1"), "https://old.com"))
                .await
                .unwrap();
            store
                .set(make_config("task-1", Some("cfg-1"), "https://new.com"))
                .await
                .unwrap();

            let config = store.get("task-1", "cfg-1").await.unwrap().unwrap();
            assert_eq!(
                config.url, "https://new.com",
                "overwrite should update the URL"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn set_assigns_id_when_none() {
        let store = make_store().await;
        TenantContext::scope("acme", async {
            let config = make_config("task-1", None, "https://example.com");
            let result = store.set(config).await.unwrap();
            assert!(
                result.id.is_some(),
                "set should assign an id when none is provided"
            );
        })
        .await;
    }

    #[tokio::test]
    async fn delete_nonexistent_is_ok() {
        let store = make_store().await;
        TenantContext::scope("acme", async {
            let result = store.delete("no-task", "no-id").await;
            assert!(
                result.is_ok(),
                "deleting a nonexistent config should not error"
            );
        })
        .await;
    }

    /// Covers lines 41-43 (to_a2a_error conversion).
    #[test]
    fn to_a2a_error_formats_message() {
        let sqlite_err = sqlx::Error::RowNotFound;
        let a2a_err = to_a2a_error(&sqlite_err);
        let msg = format!("{a2a_err}");
        assert!(
            msg.contains("sqlite error"),
            "error message should contain 'sqlite error': {msg}"
        );
    }

    #[tokio::test]
    async fn default_tenant_context_uses_empty_string() {
        let store = make_store().await;
        // No TenantContext::scope - should use "" as tenant
        store
            .set(make_config("task-1", Some("cfg-1"), "https://default.com"))
            .await
            .unwrap();
        let config = store.get("task-1", "cfg-1").await.unwrap();
        assert!(config.is_some(), "default (empty) tenant should work");
    }
}
