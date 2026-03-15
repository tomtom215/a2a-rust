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
use std::time::{Duration, Instant};

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

/// Entry in the in-memory task store, tracking creation time for TTL eviction.
#[derive(Debug, Clone)]
struct TaskEntry {
    /// The stored task.
    task: Task,
    /// When this entry was last written (for TTL-based eviction).
    last_updated: Instant,
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
}

impl Default for TaskStoreConfig {
    fn default() -> Self {
        Self {
            max_capacity: Some(10_000),
            task_ttl: Some(Duration::from_secs(3600)), // 1 hour
        }
    }
}

/// Number of writes between automatic eviction sweeps.
///
/// Amortizes the O(n) eviction cost so it doesn't run on every single `save()`.
const EVICTION_INTERVAL: u64 = 64;

/// In-memory [`TaskStore`] backed by a [`HashMap`] under a [`RwLock`].
///
/// Suitable for testing and single-process deployments. Data is lost when the
/// process exits.
///
/// Supports TTL-based eviction of terminal tasks and a maximum capacity limit
/// to prevent unbounded memory growth.
#[derive(Debug)]
pub struct InMemoryTaskStore {
    entries: RwLock<HashMap<TaskId, TaskEntry>>,
    config: TaskStoreConfig,
    /// Counter for amortized eviction (only run every `EVICTION_INTERVAL` writes).
    write_count: std::sync::atomic::AtomicU64,
}

impl Default for InMemoryTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryTaskStore {
    /// Creates a new empty in-memory task store with default configuration.
    ///
    /// Default: max 10,000 tasks, 1-hour TTL for terminal tasks.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            config: TaskStoreConfig::default(),
            write_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Creates a new in-memory task store with custom configuration.
    #[must_use]
    pub fn with_config(config: TaskStoreConfig) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            config,
            write_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Runs background eviction of expired and over-capacity entries.
    ///
    /// Call this periodically (e.g. every 60 seconds) to clean up terminal
    /// tasks that would otherwise persist until the next `save()` call.
    pub async fn run_eviction(&self) {
        let mut store = self.entries.write().await;
        Self::evict(&mut store, &self.config);
    }

    /// Evicts expired and over-capacity entries (must be called with write lock held).
    fn evict(store: &mut HashMap<TaskId, TaskEntry>, config: &TaskStoreConfig) {
        let now = Instant::now();

        // TTL eviction: remove terminal tasks older than the TTL.
        if let Some(ttl) = config.task_ttl {
            store.retain(|_, entry| {
                if entry.task.status.state.is_terminal() {
                    now.duration_since(entry.last_updated) < ttl
                } else {
                    true
                }
            });
        }

        // Capacity eviction: remove oldest terminal tasks if over capacity.
        if let Some(max) = config.max_capacity {
            if store.len() > max {
                let overflow = store.len() - max;
                // Collect terminal tasks sorted by age (oldest first).
                let mut terminal: Vec<(TaskId, Instant)> = store
                    .iter()
                    .filter(|(_, e)| e.task.status.state.is_terminal())
                    .map(|(id, e)| (id.clone(), e.last_updated))
                    .collect();
                terminal.sort_by_key(|(_, t)| *t);

                for (id, _) in terminal.into_iter().take(overflow) {
                    store.remove(&id);
                }
            }
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for InMemoryTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            trace_debug!(task_id = %task.id, state = ?task.status.state, "saving task");
            let mut store = self.entries.write().await;

            store.insert(
                task.id.clone(),
                TaskEntry {
                    task,
                    last_updated: Instant::now(),
                },
            );

            // Amortized eviction: only run every EVICTION_INTERVAL writes,
            // or immediately if the store exceeds max capacity.
            let count = self
                .write_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let over_capacity = self
                .config
                .max_capacity
                .is_some_and(|max| store.len() > max);
            if count.is_multiple_of(EVICTION_INTERVAL) || over_capacity {
                Self::evict(&mut store, &self.config);
            }

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
            let store = self.entries.read().await;
            let result = store.get(id).map(|e| e.task.clone());
            drop(store);
            Ok(result)
        })
    }

    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.entries.read().await;
            let mut tasks: Vec<Task> = store
                .values()
                .filter(|e| {
                    if let Some(ref ctx) = params.context_id {
                        if e.task.context_id.0 != *ctx {
                            return false;
                        }
                    }
                    if let Some(ref status) = params.status {
                        if e.task.status.state != *status {
                            return false;
                        }
                    }
                    true
                })
                .map(|e| e.task.clone())
                .collect();
            drop(store);

            // Sort by task ID for deterministic output.
            tasks.sort_by(|a, b| a.id.0.cmp(&b.id.0));

            // Apply cursor-based pagination via page_token.
            // The page_token is the last task ID from the previous page.
            if let Some(ref token) = params.page_token {
                if let Some(pos) = tasks.iter().position(|t| t.id.0 == *token) {
                    // Skip up to and including the cursor task.
                    tasks = tasks.split_off(pos + 1);
                } else {
                    // Token refers to a non-existent task — return empty page.
                    tasks.clear();
                }
            }

            // Treat page_size of 0 as "use default" per defensive convention.
            let page_size = match params.page_size {
                Some(0) | None => 50_usize,
                Some(n) => n as usize,
            };
            let next_page_token = if tasks.len() > page_size {
                tasks
                    .get(page_size.saturating_sub(1))
                    .map(|t| t.id.0.clone())
            } else {
                None
            };
            tasks.truncate(page_size);

            let mut response = TaskListResponse::new(tasks);
            response.next_page_token = next_page_token;
            Ok(response)
        })
    }

    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let mut store = self.entries.write().await;
            store.remove(id);
            drop(store);
            Ok(())
        })
    }
}
