// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task persistence trait and in-memory implementation.
//!
//! [`TaskStore`] abstracts task persistence so that the server framework can
//! be backed by any storage engine. [`InMemoryTaskStore`] provides a simple
//! `HashMap`-based implementation suitable for testing and single-process
//! deployments.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use a2a_types::error::A2aResult;
use a2a_types::params::ListTasksParams;
use a2a_types::responses::TaskListResponse;
use a2a_types::task::{Task, TaskId};
use tokio::sync::RwLock;

/// Trait for persisting and retrieving [`Task`] objects.
///
/// All methods return `Pin<Box<dyn Future>>` for object safety — this trait
/// is used as `Box<dyn TaskStore>`.
///
/// # Object safety
///
/// Do not add `async fn` methods; use the explicit `Pin<Box<...>>` form.
pub trait TaskStore: Send + Sync + 'static {
    /// Saves (creates or updates) a task.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the store operation fails.
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Retrieves a task by its ID, returning `None` if not found.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the store operation fails.
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>;

    /// Lists tasks matching the given filter parameters.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the store operation fails.
    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>>;

    /// Deletes a task by its ID.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if the store operation fails.
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}

/// In-memory [`TaskStore`] backed by a [`HashMap`] under a [`RwLock`].
///
/// Suitable for testing and single-process deployments. Data is lost when the
/// process exits.
#[derive(Debug, Default)]
pub struct InMemoryTaskStore {
    tasks: RwLock<HashMap<TaskId, Task>>,
}

impl InMemoryTaskStore {
    /// Creates a new empty in-memory task store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for InMemoryTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            trace_debug!(task_id = %task.id, state = ?task.status.state, "saving task");
            let mut store = self.tasks.write().await;
            store.insert(task.id.clone(), task);
            drop(store);
            Ok(())
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        Box::pin(async move {
            trace_debug!(task_id = %id, "fetching task");
            let store = self.tasks.read().await;
            let result = store.get(id).cloned();
            drop(store);
            Ok(result)
        })
    }

    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.tasks.read().await;
            let mut tasks: Vec<Task> = store
                .values()
                .filter(|t| {
                    if let Some(ref ctx) = params.context_id {
                        if t.context_id.0 != *ctx {
                            return false;
                        }
                    }
                    if let Some(ref status) = params.status {
                        if t.status.state != *status {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();
            drop(store);

            // Sort by task ID for deterministic output.
            tasks.sort_by(|a, b| a.id.0.cmp(&b.id.0));

            let page_size = params.page_size.unwrap_or(50) as usize;
            tasks.truncate(page_size);

            Ok(TaskListResponse::new(tasks))
        })
    }

    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut store = self.tasks.write().await;
            store.remove(id);
            drop(store);
            Ok(())
        })
    }
}
