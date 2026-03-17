// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! In-memory task store backed by a `HashMap` under a `RwLock`.
//!
//! # Module structure
//!
//! | Module | Responsibility |
//! |---|---|
//! | (this file) | Core CRUD operations and `TaskStore` trait impl |
//! | [`eviction`] | TTL and capacity-based eviction logic |

mod eviction;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Instant;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};
use tokio::sync::RwLock;

use super::{TaskStore, TaskStoreConfig};

/// Entry in the in-memory task store, tracking creation time for TTL eviction.
#[derive(Debug, Clone)]
pub(super) struct TaskEntry {
    /// The stored task.
    pub(super) task: Task,
    /// When this entry was last written (for TTL-based eviction).
    pub(super) last_updated: Instant,
}

/// In-memory [`TaskStore`] backed by a [`HashMap`] under a [`RwLock`].
///
/// Suitable for testing and single-process deployments. Data is lost when the
/// process exits.
///
/// Supports TTL-based eviction of terminal tasks and a maximum capacity limit
/// to prevent unbounded memory growth.
///
/// # Eviction behavior
///
/// Eviction runs as a background task every N writes (configurable via
/// [`TaskStoreConfig::eviction_interval`]) and whenever the store exceeds
/// `max_capacity`. The eviction sweep is decoupled from the `save()` write
/// lock so that writers are not blocked during the O(n) cleanup. However,
/// if the system goes idle (no `save()` calls), completed tasks may persist
/// in memory longer than their TTL.
///
/// **Operators should call [`run_eviction()`](Self::run_eviction) periodically**
/// (e.g. every 60 seconds via `tokio::time::interval`) to ensure timely
/// cleanup of terminal tasks during idle periods.
///
/// # Concurrency
///
/// For high-concurrency production deployments, consider `SqliteTaskStore`
/// which uses a connection pool and row-level locking. The in-memory store
/// uses a single `RwLock` and is optimized for testing and moderate load.
#[derive(Debug)]
pub struct InMemoryTaskStore {
    pub(super) entries: RwLock<HashMap<TaskId, TaskEntry>>,
    pub(super) config: TaskStoreConfig,
    /// Counter for amortized eviction (only run every `EVICTION_INTERVAL` writes).
    pub(super) write_count: std::sync::atomic::AtomicU64,
    /// Prevents multiple concurrent eviction sweeps.
    pub(super) eviction_in_progress: std::sync::atomic::AtomicBool,
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
            eviction_in_progress: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Creates a new in-memory task store with custom configuration.
    #[must_use]
    pub fn with_config(config: TaskStoreConfig) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            config,
            write_count: std::sync::atomic::AtomicU64::new(0),
            eviction_in_progress: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for InMemoryTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            trace_debug!(task_id = %task.id, state = ?task.status.state, "saving task");

            // Insert under write lock, then release immediately.
            let needs_eviction = {
                let mut store = self.entries.write().await;
                store.insert(
                    task.id.clone(),
                    TaskEntry {
                        task,
                        last_updated: Instant::now(),
                    },
                );
                let len = store.len();
                drop(store);
                self.should_evict(len)
            };

            // Run eviction outside the write lock to reduce contention.
            if needs_eviction {
                self.maybe_evict().await;
            }

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

            // Treat page_size of 0 as "use default"; clamp to MAX_PAGE_SIZE.
            let page_size = match params.page_size {
                Some(0) | None => 50_usize,
                Some(n) => (n.min(self.config.max_page_size)) as usize,
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

    fn insert_if_absent<'a>(
        &'a self,
        task: Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async move {
            let (inserted, needs_eviction) = {
                let mut store = self.entries.write().await;
                if store.contains_key(&task.id) {
                    return Ok(false);
                }
                store.insert(
                    task.id.clone(),
                    TaskEntry {
                        task,
                        last_updated: Instant::now(),
                    },
                );
                let len = store.len();
                drop(store);
                (true, self.should_evict(len))
            };

            if needs_eviction {
                self.maybe_evict().await;
            }
            Ok(inserted)
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

    fn count<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
        Box::pin(async move {
            let store = self.entries.read().await;
            Ok(store.len() as u64)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};
    use std::time::Duration;

    /// Helper to create a task with the given ID and state.
    fn make_task(id: &str, state: TaskState) -> Task {
        Task {
            id: TaskId::new(id),
            context_id: ContextId::new("ctx-default"),
            status: TaskStatus::new(state),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    /// Helper to create a task with a specific context ID.
    fn make_task_with_ctx(id: &str, ctx: &str, state: TaskState) -> Task {
        Task {
            id: TaskId::new(id),
            context_id: ContextId::new(ctx),
            status: TaskStatus::new(state),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    // ── CRUD basics ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn save_and_get_returns_task() {
        let store = InMemoryTaskStore::new();
        let task = make_task("t1", TaskState::Submitted);
        store.save(task.clone()).await.unwrap();

        let fetched = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(fetched.is_some(), "saved task should be retrievable");
        assert_eq!(fetched.unwrap().id, task.id);
    }

    #[tokio::test]
    async fn get_nonexistent_returns_none() {
        let store = InMemoryTaskStore::new();
        let result = store.get(&TaskId::new("no-such-task")).await.unwrap();
        assert!(result.is_none(), "missing task should return None");
    }

    #[tokio::test]
    async fn save_overwrites_existing_task() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t1", TaskState::Working))
            .await
            .unwrap();

        let fetched = store.get(&TaskId::new("t1")).await.unwrap().unwrap();
        assert_eq!(
            fetched.status.state,
            TaskState::Working,
            "save should overwrite existing task"
        );
    }

    #[tokio::test]
    async fn delete_removes_task() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();
        store.delete(&TaskId::new("t1")).await.unwrap();

        let result = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(result.is_none(), "deleted task should no longer exist");
    }

    #[tokio::test]
    async fn delete_nonexistent_is_ok() {
        let store = InMemoryTaskStore::new();
        // Should not error even though the task does not exist.
        store.delete(&TaskId::new("ghost")).await.unwrap();
    }

    // ── insert_if_absent ─────────────────────────────────────────────────

    #[tokio::test]
    async fn insert_if_absent_inserts_new_task() {
        let store = InMemoryTaskStore::new();
        let inserted = store
            .insert_if_absent(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();
        assert!(inserted, "first insert should succeed");

        let fetched = store.get(&TaskId::new("t1")).await.unwrap();
        assert!(fetched.is_some());
    }

    #[tokio::test]
    async fn insert_if_absent_rejects_duplicate() {
        let store = InMemoryTaskStore::new();
        store
            .insert_if_absent(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();

        let second = store
            .insert_if_absent(make_task("t1", TaskState::Working))
            .await
            .unwrap();
        assert!(!second, "duplicate insert should return false");

        // Original task should be unchanged.
        let fetched = store.get(&TaskId::new("t1")).await.unwrap().unwrap();
        assert_eq!(
            fetched.status.state,
            TaskState::Submitted,
            "original task should not be overwritten by insert_if_absent"
        );
    }

    // ── count ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn count_empty_store() {
        let store = InMemoryTaskStore::new();
        assert_eq!(store.count().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn count_reflects_saves_and_deletes() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t2", TaskState::Working))
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 2);

        store.delete(&TaskId::new("t1")).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 1);
    }

    // ── list with pagination ─────────────────────────────────────────────

    #[tokio::test]
    async fn list_empty_store_returns_empty() {
        let store = InMemoryTaskStore::new();
        let params = ListTasksParams::default();
        let response = store.list(&params).await.unwrap();
        assert!(response.tasks.is_empty());
        assert!(response.next_page_token.is_none());
    }

    #[tokio::test]
    async fn list_returns_all_tasks_sorted_by_id() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task("c", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("a", TaskState::Working))
            .await
            .unwrap();
        store
            .save(make_task("b", TaskState::Completed))
            .await
            .unwrap();

        let params = ListTasksParams::default();
        let response = store.list(&params).await.unwrap();
        let ids: Vec<&str> = response.tasks.iter().map(|t| t.id.0.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"], "tasks should be sorted by ID");
    }

    #[tokio::test]
    async fn list_filters_by_context_id() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task_with_ctx("t1", "ctx-a", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task_with_ctx("t2", "ctx-b", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task_with_ctx("t3", "ctx-a", TaskState::Working))
            .await
            .unwrap();

        let params = ListTasksParams {
            context_id: Some("ctx-a".to_string()),
            ..Default::default()
        };
        let response = store.list(&params).await.unwrap();
        assert_eq!(response.tasks.len(), 2);
        assert!(response.tasks.iter().all(|t| t.context_id.0 == "ctx-a"));
    }

    #[tokio::test]
    async fn list_filters_by_status() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();
        store
            .save(make_task("t2", TaskState::Working))
            .await
            .unwrap();
        store
            .save(make_task("t3", TaskState::Submitted))
            .await
            .unwrap();

        let params = ListTasksParams {
            status: Some(TaskState::Submitted),
            ..Default::default()
        };
        let response = store.list(&params).await.unwrap();
        assert_eq!(response.tasks.len(), 2);
    }

    #[tokio::test]
    async fn list_pagination_page_size() {
        let store = InMemoryTaskStore::new();
        for i in 0..5 {
            store
                .save(make_task(&format!("t{i:02}"), TaskState::Submitted))
                .await
                .unwrap();
        }

        let params = ListTasksParams {
            page_size: Some(2),
            ..Default::default()
        };
        let page1 = store.list(&params).await.unwrap();
        assert_eq!(page1.tasks.len(), 2, "first page should have 2 tasks");
        assert!(
            page1.next_page_token.is_some(),
            "should have next_page_token when more results exist"
        );

        // Fetch second page using the cursor.
        let params2 = ListTasksParams {
            page_size: Some(2),
            page_token: page1.next_page_token,
            ..Default::default()
        };
        let page2 = store.list(&params2).await.unwrap();
        assert_eq!(page2.tasks.len(), 2, "second page should have 2 tasks");

        // Fetch third page (should have 1 remaining task).
        let params3 = ListTasksParams {
            page_size: Some(2),
            page_token: page2.next_page_token,
            ..Default::default()
        };
        let page3 = store.list(&params3).await.unwrap();
        assert_eq!(page3.tasks.len(), 1, "third page should have 1 task");
        assert!(
            page3.next_page_token.is_none(),
            "no more pages after the last task"
        );
    }

    #[tokio::test]
    async fn list_invalid_page_token_returns_empty() {
        let store = InMemoryTaskStore::new();
        store
            .save(make_task("t1", TaskState::Submitted))
            .await
            .unwrap();

        let params = ListTasksParams {
            page_token: Some("nonexistent-cursor".to_string()),
            ..Default::default()
        };
        let response = store.list(&params).await.unwrap();
        assert!(
            response.tasks.is_empty(),
            "invalid page_token should yield empty results"
        );
    }

    #[tokio::test]
    async fn list_page_size_zero_uses_default() {
        let store = InMemoryTaskStore::new();
        for i in 0..60 {
            store
                .save(make_task(&format!("t{i:03}"), TaskState::Submitted))
                .await
                .unwrap();
        }

        let params = ListTasksParams {
            page_size: Some(0),
            ..Default::default()
        };
        let response = store.list(&params).await.unwrap();
        // Default page size is 50.
        assert_eq!(
            response.tasks.len(),
            50,
            "page_size=0 should use the default of 50"
        );
    }

    // ── TTL eviction ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn ttl_eviction_removes_expired_terminal_tasks() {
        let config = TaskStoreConfig {
            max_capacity: None,
            task_ttl: Some(Duration::from_millis(1)),
            eviction_interval: 1,
            max_page_size: 100,
        };
        let store = InMemoryTaskStore::with_config(config);

        // Save a completed (terminal) task.
        store
            .save(make_task("terminal", TaskState::Completed))
            .await
            .unwrap();
        // Save a non-terminal task.
        store
            .save(make_task("active", TaskState::Working))
            .await
            .unwrap();

        // Wait for TTL to expire.
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Trigger eviction via run_eviction.
        store.run_eviction().await;

        assert!(
            store.get(&TaskId::new("terminal")).await.unwrap().is_none(),
            "expired terminal task should be evicted"
        );
        assert!(
            store.get(&TaskId::new("active")).await.unwrap().is_some(),
            "non-terminal task should survive TTL eviction"
        );
    }

    #[tokio::test]
    async fn ttl_eviction_keeps_fresh_terminal_tasks() {
        let config = TaskStoreConfig {
            max_capacity: None,
            task_ttl: Some(Duration::from_secs(3600)),
            eviction_interval: 1,
            max_page_size: 100,
        };
        let store = InMemoryTaskStore::with_config(config);

        store
            .save(make_task("t1", TaskState::Completed))
            .await
            .unwrap();
        store.run_eviction().await;

        assert!(
            store.get(&TaskId::new("t1")).await.unwrap().is_some(),
            "fresh terminal task should not be evicted"
        );
    }

    // ── max capacity eviction ────────────────────────────────────────────

    #[tokio::test]
    async fn max_capacity_eviction_removes_oldest_terminal_tasks() {
        let config = TaskStoreConfig {
            max_capacity: Some(2),
            task_ttl: None,
            eviction_interval: 1,
            max_page_size: 100,
        };
        let store = InMemoryTaskStore::with_config(config);

        // Save 3 completed tasks; the oldest should be evicted when capacity is exceeded.
        store
            .save(make_task("oldest", TaskState::Completed))
            .await
            .unwrap();
        // Small sleep to ensure ordering by last_updated.
        tokio::time::sleep(Duration::from_millis(2)).await;
        store
            .save(make_task("middle", TaskState::Completed))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        store
            .save(make_task("newest", TaskState::Completed))
            .await
            .unwrap();

        // The third save triggers should_evict (over max_capacity).
        // Give the maybe_evict background task a moment to complete.
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(
            store.get(&TaskId::new("oldest")).await.unwrap().is_none(),
            "oldest terminal task should be evicted when over capacity"
        );
        assert_eq!(
            store.count().await.unwrap(),
            2,
            "store should be back at max capacity"
        );
    }

    #[tokio::test]
    async fn capacity_eviction_prefers_terminal_tasks() {
        let config = TaskStoreConfig {
            max_capacity: Some(2),
            task_ttl: None,
            eviction_interval: 1,
            max_page_size: 100,
        };
        let store = InMemoryTaskStore::with_config(config);

        // 1 active + 1 terminal, then add a third.
        store
            .save(make_task("active", TaskState::Working))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        store
            .save(make_task("done", TaskState::Completed))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        store
            .save(make_task("new", TaskState::Submitted))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(
            store.get(&TaskId::new("active")).await.unwrap().is_some(),
            "non-terminal task should survive capacity eviction"
        );
        assert!(
            store.get(&TaskId::new("done")).await.unwrap().is_none(),
            "terminal task should be evicted first"
        );
    }

    // ── Config defaults ──────────────────────────────────────────────────

    #[test]
    fn default_config_has_expected_values() {
        let cfg = TaskStoreConfig::default();
        assert_eq!(cfg.max_capacity, Some(10_000));
        assert_eq!(cfg.task_ttl, Some(Duration::from_secs(3600)));
        assert_eq!(cfg.eviction_interval, 64);
        assert_eq!(cfg.max_page_size, 1000);
    }
}
