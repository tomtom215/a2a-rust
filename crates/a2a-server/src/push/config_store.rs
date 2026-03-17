// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration storage trait and in-memory implementation.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use tokio::sync::RwLock;

/// Trait for storing push notification configurations.
///
/// Object-safe; used as `Box<dyn PushConfigStore>`.
pub trait PushConfigStore: Send + Sync + 'static {
    /// Stores (creates or updates) a push notification config.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the operation fails.
    fn set<'a>(
        &'a self,
        config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>>;

    /// Retrieves a push notification config by task ID and config ID.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the operation fails.
    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>;

    /// Lists all push notification configs for a task.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the operation fails.
    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>>;

    /// Deletes a push notification config by task ID and config ID.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the operation fails.
    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}

/// Default maximum number of push notification configs allowed per task.
const DEFAULT_MAX_PUSH_CONFIGS_PER_TASK: usize = 100;

/// Default global maximum number of push notification configs across all tasks.
/// Prevents unbounded memory growth when many tasks register configs.
const DEFAULT_MAX_TOTAL_PUSH_CONFIGS: usize = 100_000;

/// In-memory [`PushConfigStore`] backed by a `HashMap`.
#[derive(Debug)]
pub struct InMemoryPushConfigStore {
    configs: RwLock<HashMap<(String, String), TaskPushNotificationConfig>>,
    /// Maximum number of push configs allowed per task.
    max_configs_per_task: usize,
    /// Global maximum number of push configs across all tasks.
    max_total_configs: usize,
}

impl Default for InMemoryPushConfigStore {
    fn default() -> Self {
        Self {
            configs: RwLock::new(HashMap::new()),
            max_configs_per_task: DEFAULT_MAX_PUSH_CONFIGS_PER_TASK,
            max_total_configs: DEFAULT_MAX_TOTAL_PUSH_CONFIGS,
        }
    }
}

impl InMemoryPushConfigStore {
    /// Creates a new empty in-memory push config store with default limits.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new push config store with a custom per-task config limit.
    #[must_use]
    pub fn with_max_configs_per_task(max: usize) -> Self {
        Self {
            configs: RwLock::new(HashMap::new()),
            max_configs_per_task: max,
            max_total_configs: DEFAULT_MAX_TOTAL_PUSH_CONFIGS,
        }
    }

    /// Sets the global maximum number of push configs across all tasks.
    ///
    /// Prevents unbounded memory growth when many tasks register configs.
    /// Default: 100,000.
    #[must_use]
    pub const fn with_max_total_configs(mut self, max: usize) -> Self {
        self.max_total_configs = max;
        self
    }
}

#[allow(clippy::manual_async_fn)]
impl PushConfigStore for InMemoryPushConfigStore {
    fn set<'a>(
        &'a self,
        mut config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        Box::pin(async move {
            // Assign an ID if not present.
            let id = config
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            config.id = Some(id.clone());

            let key = (config.task_id.clone(), id);
            let mut store = self.configs.write().await;

            // Reject if this is a new config and limits are reached.
            if !store.contains_key(&key) {
                // Global limit: prevent unbounded memory growth.
                let total = store.len();
                if total >= self.max_total_configs {
                    drop(store);
                    return Err(a2a_protocol_types::error::A2aError::invalid_params(
                        format!(
                            "global push config limit exceeded: {total} configs (max {})",
                            self.max_total_configs,
                        ),
                    ));
                }
                // Per-task limit.
                let task_id = &config.task_id;
                let count = store.keys().filter(|(tid, _)| tid == task_id).count();
                let max = self.max_configs_per_task;
                if count >= max {
                    drop(store);
                    return Err(a2a_protocol_types::error::A2aError::invalid_params(format!(
                        "push config limit exceeded: task {task_id} already has {count} configs (max {max})"
                    )));
                }
            }

            store.insert(key, config.clone());
            drop(store);
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
            let store = self.configs.read().await;
            let key = (task_id.to_owned(), id.to_owned());
            let result = store.get(&key).cloned();
            drop(store);
            Ok(result)
        })
    }

    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.configs.read().await;
            let configs: Vec<_> = store
                .iter()
                .filter(|((tid, _), _)| tid == task_id)
                .map(|(_, v)| v.clone())
                .collect();
            drop(store);
            Ok(configs)
        })
    }

    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut store = self.configs.write().await;
            let key = (task_id.to_owned(), id.to_owned());
            store.remove(&key);
            drop(store);
            Ok(())
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
    async fn set_assigns_id_when_none() {
        let store = InMemoryPushConfigStore::new();
        let config = make_config("task-1", None, "https://example.com/hook");
        let result = store.set(config).await.expect("set should succeed");
        assert!(
            result.id.is_some(),
            "set should assign an id when none is provided"
        );
    }

    #[tokio::test]
    async fn set_preserves_explicit_id() {
        let store = InMemoryPushConfigStore::new();
        let config = make_config("task-1", Some("my-id"), "https://example.com/hook");
        let result = store.set(config).await.expect("set should succeed");
        assert_eq!(
            result.id.as_deref(),
            Some("my-id"),
            "set should preserve the explicitly provided id"
        );
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_config() {
        let store = InMemoryPushConfigStore::new();
        let result = store
            .get("no-task", "no-id")
            .await
            .expect("get should succeed");
        assert!(
            result.is_none(),
            "get should return None for a non-existent config"
        );
    }

    #[tokio::test]
    async fn set_then_get_round_trip() {
        let store = InMemoryPushConfigStore::new();
        let config = make_config("task-1", Some("cfg-1"), "https://example.com/hook");
        store.set(config).await.expect("set should succeed");

        let retrieved = store
            .get("task-1", "cfg-1")
            .await
            .expect("get should succeed")
            .expect("config should exist after set");
        assert_eq!(retrieved.task_id, "task-1");
        assert_eq!(retrieved.url, "https://example.com/hook");
    }

    #[tokio::test]
    async fn overwrite_existing_config() {
        let store = InMemoryPushConfigStore::new();
        let config1 = make_config("task-1", Some("cfg-1"), "https://example.com/v1");
        store.set(config1).await.expect("first set should succeed");

        let config2 = make_config("task-1", Some("cfg-1"), "https://example.com/v2");
        store
            .set(config2)
            .await
            .expect("overwrite set should succeed");

        let retrieved = store
            .get("task-1", "cfg-1")
            .await
            .expect("get should succeed")
            .expect("config should exist");
        assert_eq!(
            retrieved.url, "https://example.com/v2",
            "overwrite should update the URL"
        );
    }

    #[tokio::test]
    async fn list_returns_empty_for_unknown_task() {
        let store = InMemoryPushConfigStore::new();
        let configs = store
            .list("no-such-task")
            .await
            .expect("list should succeed");
        assert!(
            configs.is_empty(),
            "list should return empty vec for unknown task"
        );
    }

    #[tokio::test]
    async fn list_returns_only_configs_for_given_task() {
        let store = InMemoryPushConfigStore::new();
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

        let a_configs = store.list("task-a").await.expect("list should succeed");
        assert_eq!(a_configs.len(), 2, "task-a should have exactly 2 configs");

        let b_configs = store.list("task-b").await.expect("list should succeed");
        assert_eq!(b_configs.len(), 1, "task-b should have exactly 1 config");
    }

    #[tokio::test]
    async fn delete_removes_config() {
        let store = InMemoryPushConfigStore::new();
        store
            .set(make_config("task-1", Some("cfg-1"), "https://example.com"))
            .await
            .unwrap();

        store
            .delete("task-1", "cfg-1")
            .await
            .expect("delete should succeed");

        let result = store.get("task-1", "cfg-1").await.unwrap();
        assert!(
            result.is_none(),
            "config should be gone after delete"
        );
    }

    #[tokio::test]
    async fn delete_nonexistent_is_ok() {
        let store = InMemoryPushConfigStore::new();
        let result = store.delete("no-task", "no-id").await;
        assert!(
            result.is_ok(),
            "deleting a non-existent config should not error"
        );
    }

    #[tokio::test]
    async fn max_configs_per_task_limit_enforced() {
        let store = InMemoryPushConfigStore::with_max_configs_per_task(2);
        store
            .set(make_config("task-1", Some("c1"), "https://a.com"))
            .await
            .unwrap();
        store
            .set(make_config("task-1", Some("c2"), "https://b.com"))
            .await
            .unwrap();

        let err = store
            .set(make_config("task-1", Some("c3"), "https://c.com"))
            .await
            .expect_err("third config should exceed per-task limit");
        let msg = format!("{err}");
        assert!(
            msg.contains("limit exceeded"),
            "error message should mention limit exceeded, got: {msg}"
        );
    }

    #[tokio::test]
    async fn per_task_limit_does_not_block_other_tasks() {
        let store = InMemoryPushConfigStore::with_max_configs_per_task(1);
        store
            .set(make_config("task-1", Some("c1"), "https://a.com"))
            .await
            .unwrap();

        // Different task should still be allowed
        let result = store
            .set(make_config("task-2", Some("c1"), "https://b.com"))
            .await;
        assert!(
            result.is_ok(),
            "per-task limit should not block a different task"
        );
    }

    #[tokio::test]
    async fn overwrite_does_not_count_toward_per_task_limit() {
        let store = InMemoryPushConfigStore::with_max_configs_per_task(1);
        store
            .set(make_config("task-1", Some("c1"), "https://a.com"))
            .await
            .unwrap();

        // Overwriting the same config should succeed even though limit is 1
        let result = store
            .set(make_config("task-1", Some("c1"), "https://b.com"))
            .await;
        assert!(
            result.is_ok(),
            "overwriting an existing config should not count toward the limit"
        );
    }

    #[tokio::test]
    async fn max_total_configs_limit_enforced() {
        let store =
            InMemoryPushConfigStore::with_max_configs_per_task(100).with_max_total_configs(2);
        store
            .set(make_config("t1", Some("c1"), "https://a.com"))
            .await
            .unwrap();
        store
            .set(make_config("t2", Some("c2"), "https://b.com"))
            .await
            .unwrap();

        let err = store
            .set(make_config("t3", Some("c3"), "https://c.com"))
            .await
            .expect_err("third config should exceed global limit");
        let msg = format!("{err}");
        assert!(
            msg.contains("global push config limit exceeded"),
            "error should mention global limit, got: {msg}"
        );
    }

    #[tokio::test]
    async fn overwrite_does_not_count_toward_global_limit() {
        let store =
            InMemoryPushConfigStore::with_max_configs_per_task(100).with_max_total_configs(1);
        store
            .set(make_config("t1", Some("c1"), "https://a.com"))
            .await
            .unwrap();

        // Overwriting should succeed even at global limit
        let result = store
            .set(make_config("t1", Some("c1"), "https://b.com"))
            .await;
        assert!(
            result.is_ok(),
            "overwriting should not count toward global limit"
        );
    }
}
