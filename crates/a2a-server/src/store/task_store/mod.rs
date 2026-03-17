// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task persistence trait and in-memory implementation.
//!
//! [`TaskStore`] abstracts task persistence so that the server framework can
//! be backed by any storage engine. [`InMemoryTaskStore`] provides a simple
//! `HashMap`-based implementation suitable for testing and single-process
//! deployments.

mod in_memory;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};

pub use in_memory::InMemoryTaskStore;

/// Trait for persisting and retrieving [`Task`] objects.
///
/// All methods return `Pin<Box<dyn Future>>` for object safety — this trait
/// is used as `Box<dyn TaskStore>`.
///
/// # Object safety
///
/// Do not add `async fn` methods; use the explicit `Pin<Box<...>>` form.
///
/// # Example
///
/// ```rust
/// use std::future::Future;
/// use std::pin::Pin;
/// use a2a_protocol_types::error::A2aResult;
/// use a2a_protocol_types::params::ListTasksParams;
/// use a2a_protocol_types::responses::TaskListResponse;
/// use a2a_protocol_types::task::{Task, TaskId};
/// use a2a_protocol_server::store::TaskStore;
///
/// /// A no-op store that rejects all operations (for illustration).
/// struct NullStore;
///
/// impl TaskStore for NullStore {
///     fn save<'a>(&'a self, _task: Task)
///         -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>
///     {
///         Box::pin(async { Ok(()) })
///     }
///
///     fn get<'a>(&'a self, _id: &'a TaskId)
///         -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>
///     {
///         Box::pin(async { Ok(None) })
///     }
///
///     fn list<'a>(&'a self, _params: &'a ListTasksParams)
///         -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>>
///     {
///         Box::pin(async { Ok(TaskListResponse::new(vec![])) })
///     }
///
///     fn insert_if_absent<'a>(&'a self, _task: Task)
///         -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>>
///     {
///         Box::pin(async { Ok(true) })
///     }
///
///     fn delete<'a>(&'a self, _id: &'a TaskId)
///         -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>
///     {
///         Box::pin(async { Ok(()) })
///     }
/// }
/// ```
pub trait TaskStore: Send + Sync + 'static {
    /// Saves (creates or updates) a task.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the store operation fails.
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Retrieves a task by its ID, returning `None` if not found.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the store operation fails.
    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>;

    /// Lists tasks matching the given filter parameters.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the store operation fails.
    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>>;

    /// Atomically inserts a task only if no task with the same ID exists.
    ///
    /// Returns `Ok(true)` if the task was inserted, `Ok(false)` if a task
    /// with the same ID already exists (no modification made).
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the store operation fails.
    fn insert_if_absent<'a>(
        &'a self,
        task: Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>>;

    /// Deletes a task by its ID.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the store operation fails.
    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Returns the total number of tasks in the store.
    ///
    /// Useful for monitoring, metrics, and capacity management. Has a default
    /// implementation that returns `0` so existing implementations are not
    /// broken when this method is added.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if the store operation fails.
    fn count<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
        Box::pin(async { Ok(0) })
    }
}

/// Configuration for [`InMemoryTaskStore`].
#[derive(Debug, Clone)]
pub struct TaskStoreConfig {
    /// Maximum number of tasks to keep in the store. Once exceeded, the oldest
    /// completed/failed tasks are evicted. `None` means no limit.
    pub max_capacity: Option<usize>,

    /// Time-to-live for completed or failed tasks. Tasks in terminal states
    /// older than this duration are evicted on the next write operation.
    /// `None` means no TTL-based eviction.
    pub task_ttl: Option<Duration>,

    /// Number of writes between automatic eviction sweeps. Default: 64.
    ///
    /// Amortizes the O(n) eviction cost so it doesn't run on every single `save()`.
    pub eviction_interval: u64,

    /// Maximum allowed page size for list queries. Default: 1000.
    ///
    /// Larger requested page sizes are clamped to this limit.
    pub max_page_size: u32,
}

impl Default for TaskStoreConfig {
    fn default() -> Self {
        Self {
            max_capacity: Some(10_000),
            task_ttl: Some(Duration::from_secs(3600)), // 1 hour
            eviction_interval: 64,
            max_page_size: 1000,
        }
    }
}
