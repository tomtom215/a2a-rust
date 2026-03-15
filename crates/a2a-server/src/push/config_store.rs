// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration storage trait and in-memory implementation.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use a2a_types::error::A2aResult;
use a2a_types::push::TaskPushNotificationConfig;
use a2a_types::task::TaskId;
use tokio::sync::RwLock;

/// Trait for storing push notification configurations.
///
/// Object-safe; used as `Box<dyn PushConfigStore>`.
pub trait PushConfigStore: Send + Sync + 'static {
    /// Stores (creates or updates) a push notification config.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the operation fails.
    fn set<'a>(
        &'a self,
        config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>>;

    /// Retrieves a push notification config by task ID and config ID.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the operation fails.
    fn get<'a>(
        &'a self,
        task_id: &'a TaskId,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>;

    /// Lists all push notification configs for a task.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the operation fails.
    fn list<'a>(
        &'a self,
        task_id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>>;

    /// Deletes a push notification config by task ID and config ID.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the operation fails.
    fn delete<'a>(
        &'a self,
        task_id: &'a TaskId,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}

/// In-memory [`PushConfigStore`] backed by a `HashMap`.
#[derive(Debug, Default)]
pub struct InMemoryPushConfigStore {
    configs: RwLock<HashMap<(TaskId, String), TaskPushNotificationConfig>>,
}

impl InMemoryPushConfigStore {
    /// Creates a new empty in-memory push config store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
                .push_notification_config
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            config.push_notification_config.id = Some(id.clone());

            let key = (config.task_id.clone(), id);
            let mut store = self.configs.write().await;
            store.insert(key, config.clone());
            drop(store);
            Ok(config)
        })
    }

    fn get<'a>(
        &'a self,
        task_id: &'a TaskId,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>
    {
        Box::pin(async move {
            let store = self.configs.read().await;
            let key = (task_id.clone(), id.to_owned());
            let result = store.get(&key).cloned();
            drop(store);
            Ok(result)
        })
    }

    fn list<'a>(
        &'a self,
        task_id: &'a TaskId,
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
        task_id: &'a TaskId,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut store = self.configs.write().await;
            let key = (task_id.clone(), id.to_owned());
            store.remove(&key);
            drop(store);
            Ok(())
        })
    }
}
