// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! SQLite-backed [`PushConfigStore`] implementation.
//!
//! Requires the `sqlite` feature flag. Uses `sqlx` for async `SQLite` access.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use sqlx::sqlite::SqlitePool;

use super::config_store::PushConfigStore;

/// SQLite-backed [`PushConfigStore`].
///
/// Stores push notification configs as JSON blobs in a `push_configs` table.
///
/// # Schema
///
/// ```sql
/// CREATE TABLE IF NOT EXISTS push_configs (
///     task_id TEXT NOT NULL,
///     id      TEXT NOT NULL,
///     data    TEXT NOT NULL,
///     PRIMARY KEY (task_id, id)
/// );
/// ```
#[derive(Debug, Clone)]
pub struct SqlitePushConfigStore {
    pool: SqlitePool,
}

/// Converts a `sqlx::Error` to an `A2aError`.
#[allow(clippy::needless_pass_by_value)]
fn to_a2a_error(e: sqlx::Error) -> A2aError {
    A2aError::internal(format!("sqlite error: {e}"))
}

impl SqlitePushConfigStore {
    /// Opens (or creates) a `SQLite` database and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or the schema migration fails.
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
            "CREATE TABLE IF NOT EXISTS push_configs (
                task_id TEXT NOT NULL,
                id      TEXT NOT NULL,
                data    TEXT NOT NULL,
                PRIMARY KEY (task_id, id)
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

#[allow(clippy::manual_async_fn)]
impl PushConfigStore for SqlitePushConfigStore {
    fn set<'a>(
        &'a self,
        mut config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        Box::pin(async move {
            let id = config
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            config.id = Some(id.clone());

            let data = serde_json::to_string(&config)
                .map_err(|e| A2aError::internal(format!("serialize: {e}")))?;

            sqlx::query(
                "INSERT INTO push_configs (task_id, id, data)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(task_id, id) DO UPDATE SET data = excluded.data",
            )
            .bind(&config.task_id)
            .bind(&id)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(to_a2a_error)?;

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
            let row: Option<(String,)> =
                sqlx::query_as("SELECT data FROM push_configs WHERE task_id = ?1 AND id = ?2")
                    .bind(task_id)
                    .bind(id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(to_a2a_error)?;

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
            let rows: Vec<(String,)> =
                sqlx::query_as("SELECT data FROM push_configs WHERE task_id = ?1")
                    .bind(task_id)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(to_a2a_error)?;

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
            sqlx::query("DELETE FROM push_configs WHERE task_id = ?1 AND id = ?2")
                .bind(task_id)
                .bind(id)
                .execute(&self.pool)
                .await
                .map_err(to_a2a_error)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::push::TaskPushNotificationConfig;

    async fn make_store() -> SqlitePushConfigStore {
        SqlitePushConfigStore::new("sqlite::memory:")
            .await
            .expect("failed to create in-memory push config store")
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
    async fn set_assigns_id_when_none() {
        let store = make_store().await;
        let config = make_config("task-1", None, "https://example.com/hook");
        let result = store.set(config).await.expect("set should succeed");
        assert!(
            result.id.is_some(),
            "set should assign an id when none is provided"
        );
    }

    #[tokio::test]
    async fn set_preserves_explicit_id() {
        let store = make_store().await;
        let config = make_config("task-1", Some("my-id"), "https://example.com/hook");
        let result = store.set(config).await.expect("set should succeed");
        assert_eq!(
            result.id.as_deref(),
            Some("my-id"),
            "set should preserve the explicit id"
        );
    }

    #[tokio::test]
    async fn set_then_get_round_trip() {
        let store = make_store().await;
        let config = make_config("task-1", Some("cfg-1"), "https://example.com/hook");
        store.set(config).await.unwrap();

        let retrieved = store.get("task-1", "cfg-1").await.unwrap();
        let retrieved = retrieved.expect("config should exist after set");
        assert_eq!(retrieved.task_id, "task-1");
        assert_eq!(retrieved.url, "https://example.com/hook");
        assert_eq!(retrieved.id.as_deref(), Some("cfg-1"));
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_config() {
        let store = make_store().await;
        let result = store
            .get("no-task", "no-id")
            .await
            .expect("get should succeed");
        assert!(
            result.is_none(),
            "get should return None for a missing config"
        );
    }

    #[tokio::test]
    async fn overwrite_existing_config() {
        let store = make_store().await;
        store
            .set(make_config(
                "task-1",
                Some("cfg-1"),
                "https://example.com/v1",
            ))
            .await
            .unwrap();
        store
            .set(make_config(
                "task-1",
                Some("cfg-1"),
                "https://example.com/v2",
            ))
            .await
            .unwrap();

        let retrieved = store.get("task-1", "cfg-1").await.unwrap().unwrap();
        assert_eq!(
            retrieved.url, "https://example.com/v2",
            "overwrite should update the URL"
        );
    }

    #[tokio::test]
    async fn list_returns_empty_for_unknown_task() {
        let store = make_store().await;
        let configs = store.list("no-such-task").await.unwrap();
        assert!(
            configs.is_empty(),
            "list should return empty vec for unknown task"
        );
    }

    #[tokio::test]
    async fn list_returns_only_configs_for_given_task() {
        let store = make_store().await;
        store
            .set(make_config("task-a", Some("c1"), "https://a.com/1"))
            .await
            .unwrap();
        store
            .set(make_config("task-a", Some("c2"), "https://a.com/2"))
            .await
            .unwrap();
        store
            .set(make_config("task-b", Some("c3"), "https://b.com/1"))
            .await
            .unwrap();

        let a_configs = store.list("task-a").await.unwrap();
        assert_eq!(a_configs.len(), 2, "task-a should have exactly 2 configs");

        let b_configs = store.list("task-b").await.unwrap();
        assert_eq!(b_configs.len(), 1, "task-b should have exactly 1 config");
    }

    #[tokio::test]
    async fn delete_removes_config() {
        let store = make_store().await;
        store
            .set(make_config("task-1", Some("cfg-1"), "https://example.com"))
            .await
            .unwrap();

        store
            .delete("task-1", "cfg-1")
            .await
            .expect("delete should succeed");

        let result = store.get("task-1", "cfg-1").await.unwrap();
        assert!(result.is_none(), "config should be gone after delete");
    }

    #[tokio::test]
    async fn delete_nonexistent_is_ok() {
        let store = make_store().await;
        let result = store.delete("no-task", "no-id").await;
        assert!(
            result.is_ok(),
            "deleting a nonexistent config should not error"
        );
    }

    #[tokio::test]
    async fn delete_does_not_affect_other_configs() {
        let store = make_store().await;
        store
            .set(make_config("task-1", Some("c1"), "https://a.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-1", Some("c2"), "https://b.com"))
            .await
            .unwrap();

        store.delete("task-1", "c1").await.unwrap();

        let remaining = store.list("task-1").await.unwrap();
        assert_eq!(
            remaining.len(),
            1,
            "only the deleted config should be removed"
        );
        assert_eq!(remaining[0].id.as_deref(), Some("c2"));
    }

    /// Covers lines 38-40 (`to_a2a_error` conversion).
    #[test]
    fn to_a2a_error_formats_message() {
        let sqlite_err = sqlx::Error::RowNotFound;
        let a2a_err = to_a2a_error(sqlite_err);
        let msg = format!("{a2a_err}");
        assert!(
            msg.contains("sqlite error"),
            "error message should contain 'sqlite error': {msg}"
        );
    }

    #[tokio::test]
    async fn multiple_tasks_independent_configs() {
        let store = make_store().await;
        // Same config id for different tasks should coexist
        store
            .set(make_config("task-a", Some("cfg-1"), "https://a.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-b", Some("cfg-1"), "https://b.com"))
            .await
            .unwrap();

        let a = store.get("task-a", "cfg-1").await.unwrap().unwrap();
        assert_eq!(a.url, "https://a.com");

        let b = store.get("task-b", "cfg-1").await.unwrap().unwrap();
        assert_eq!(b.url, "https://b.com");
    }
}
