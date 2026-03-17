// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Event queue for server-side streaming.
//!
//! The executor writes [`StreamResponse`] events to an [`EventQueueWriter`];
//! the HTTP layer reads them from an [`EventQueueReader`] and serializes them
//! as SSE frames.
//!
//! [`InMemoryQueueWriter`] and [`InMemoryQueueReader`] are backed by a
//! `tokio::sync::broadcast` channel, enabling multiple concurrent readers
//! (fan-out) for the same event stream. This allows `SubscribeToTask`
//! (resubscribe) to work even when another SSE stream is already active.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::TaskId;
use tokio::sync::{broadcast, RwLock};

use crate::metrics::Metrics;

/// A zero-allocation writer that counts bytes written without storing them.
///
/// Used by [`InMemoryQueueWriter::write`] to measure serialized event size
/// without performing a full allocation — avoiding the "double serialization"
/// penalty (serialize once here for size, then again in the SSE layer).
struct CountingWriter(usize);

impl std::io::Write for CountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Default channel capacity for event queues.
pub const DEFAULT_QUEUE_CAPACITY: usize = 64;

/// Default maximum event size in bytes (16 MiB).
pub const DEFAULT_MAX_EVENT_SIZE: usize = 16 * 1024 * 1024;

/// Default write timeout for event queue sends (5 seconds).
///
/// Retained for API compatibility. Broadcast sends are non-blocking, so
/// this value is not actively used for backpressure. It may be used by
/// future queue implementations.
pub const DEFAULT_WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

// ── EventQueueWriter ─────────────────────────────────────────────────────────

/// Trait for writing streaming events.
///
/// Object-safe; used as `&dyn EventQueueWriter` in the executor.
pub trait EventQueueWriter: Send + Sync + 'static {
    /// Writes a streaming event to the queue.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`] if no receivers are active.
    fn write<'a>(
        &'a self,
        event: StreamResponse,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Signals that no more events will be written.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`] if closing fails.
    fn close<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}

// ── EventQueueReader ─────────────────────────────────────────────────────────

/// Trait for reading streaming events.
///
/// NOT object-safe (used as a concrete type internally). The `async fn` is
/// fine because this trait is never used behind `dyn`.
pub trait EventQueueReader: Send + 'static {
    /// Reads the next event, returning `None` when the stream is closed.
    fn read(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Option<A2aResult<StreamResponse>>> + Send + '_>>;
}

// ── InMemoryQueueWriter ──────────────────────────────────────────────────────

/// In-memory [`EventQueueWriter`] backed by a `broadcast` channel sender.
///
/// Supports multiple concurrent readers (fan-out) via [`subscribe()`](Self::subscribe).
/// Enforces a maximum serialized event size to prevent OOM from oversized
/// events written by executors.
///
/// Broadcast sends are non-blocking: if a reader falls behind, it will
/// receive a lagged notification and skip missed events rather than blocking
/// the writer.
#[derive(Debug, Clone)]
pub struct InMemoryQueueWriter {
    tx: broadcast::Sender<A2aResult<StreamResponse>>,
    /// Maximum serialized event size in bytes.
    max_event_size: usize,
    /// Retained for API compatibility with `new_in_memory_queue_with_options`.
    #[allow(dead_code)]
    write_timeout: std::time::Duration,
}

impl InMemoryQueueWriter {
    /// Creates a new reader that will receive all future events from this writer.
    ///
    /// This enables fan-out: multiple SSE streams can subscribe to the same
    /// event queue, which is required for `SubscribeToTask` (resubscribe).
    #[must_use]
    pub fn subscribe(&self) -> InMemoryQueueReader {
        InMemoryQueueReader {
            rx: self.tx.subscribe(),
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl EventQueueWriter for InMemoryQueueWriter {
    fn write<'a>(
        &'a self,
        event: StreamResponse,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Check serialized event size to prevent OOM from oversized events.
            // Uses a zero-allocation CountingWriter instead of `to_string()` to
            // avoid allocating a full String just for size measurement — the event
            // will be serialized again in the SSE layer.
            let serialized_size = {
                let mut counter = CountingWriter(0);
                serde_json::to_writer(&mut counter, &event)
                    .map(|()| counter.0)
                    .unwrap_or(0)
            };
            if serialized_size > self.max_event_size {
                return Err(A2aError::internal(format!(
                    "event size {} bytes exceeds maximum {} bytes",
                    serialized_size, self.max_event_size
                )));
            }
            self.tx
                .send(Ok(event))
                .map(|_| ())
                .map_err(|_| A2aError::internal("event queue: no active receivers"))
        })
    }

    fn close<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Dropping all sender clones closes the channel. The spawned
            // executor task will drop its writer, causing readers to see EOF.
            Ok(())
        })
    }
}

// ── InMemoryQueueReader ──────────────────────────────────────────────────────

/// In-memory [`EventQueueReader`] backed by a `broadcast` channel receiver.
///
/// If the reader falls behind (slower than the writer), missed events are
/// silently skipped and the reader continues with the next available event.
#[derive(Debug)]
pub struct InMemoryQueueReader {
    rx: broadcast::Receiver<A2aResult<StreamResponse>>,
}

impl EventQueueReader for InMemoryQueueReader {
    fn read(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Option<A2aResult<StreamResponse>>> + Send + '_>> {
        Box::pin(async move {
            loop {
                match self.rx.recv().await {
                    Ok(event) => return Some(event),
                    Err(broadcast::error::RecvError::Lagged(_n)) => {
                        trace_warn!(
                            lagged = _n,
                            "event queue reader lagged, skipping missed events"
                        );
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        })
    }
}

// ── Constructor ──────────────────────────────────────────────────────────────

/// Creates a new in-memory event queue pair with the default capacity,
/// default max event size, and default write timeout.
#[must_use]
pub fn new_in_memory_queue() -> (InMemoryQueueWriter, InMemoryQueueReader) {
    new_in_memory_queue_with_options(
        DEFAULT_QUEUE_CAPACITY,
        DEFAULT_MAX_EVENT_SIZE,
        DEFAULT_WRITE_TIMEOUT,
    )
}

/// Creates a new in-memory event queue pair with the specified capacity
/// and default max event size / write timeout.
#[must_use]
pub fn new_in_memory_queue_with_capacity(
    capacity: usize,
) -> (InMemoryQueueWriter, InMemoryQueueReader) {
    new_in_memory_queue_with_options(capacity, DEFAULT_MAX_EVENT_SIZE, DEFAULT_WRITE_TIMEOUT)
}

/// Creates a new in-memory event queue pair with the specified capacity,
/// maximum event size, and write timeout.
#[must_use]
pub fn new_in_memory_queue_with_options(
    capacity: usize,
    max_event_size: usize,
    write_timeout: std::time::Duration,
) -> (InMemoryQueueWriter, InMemoryQueueReader) {
    let (tx, rx) = broadcast::channel(capacity);
    (
        InMemoryQueueWriter {
            tx,
            max_event_size,
            write_timeout,
        },
        InMemoryQueueReader { rx },
    )
}

// ── EventQueueManager ────────────────────────────────────────────────────────

/// Manages event queues for active tasks.
///
/// Each task can have at most one active writer. Multiple readers can
/// subscribe to the same writer concurrently (fan-out), enabling
/// `SubscribeToTask` to work even when another SSE stream is active.
#[derive(Clone)]
pub struct EventQueueManager {
    writers: Arc<RwLock<HashMap<TaskId, Arc<InMemoryQueueWriter>>>>,
    /// Channel capacity for new event queues.
    capacity: usize,
    /// Maximum serialized event size in bytes.
    max_event_size: usize,
    /// Write timeout for event queue sends.
    write_timeout: std::time::Duration,
    /// Maximum number of concurrent event queues. `None` means no limit.
    max_concurrent_queues: Option<usize>,
    /// Optional metrics hook for reporting queue depth changes.
    metrics: Option<Arc<dyn Metrics>>,
}

impl std::fmt::Debug for EventQueueManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventQueueManager")
            .field("writers", &"<RwLock<HashMap<...>>>")
            .field("capacity", &self.capacity)
            .field("max_event_size", &self.max_event_size)
            .field("write_timeout", &self.write_timeout)
            .field("max_concurrent_queues", &self.max_concurrent_queues)
            .field("metrics", &self.metrics.is_some())
            .finish()
    }
}

impl Default for EventQueueManager {
    fn default() -> Self {
        Self {
            writers: Arc::default(),
            capacity: DEFAULT_QUEUE_CAPACITY,
            max_event_size: DEFAULT_MAX_EVENT_SIZE,
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            max_concurrent_queues: None,
            metrics: None,
        }
    }
}

impl EventQueueManager {
    /// Creates a new, empty event queue manager with default capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use a2a_protocol_server::EventQueueManager;
    ///
    /// let manager = EventQueueManager::new();
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new event queue manager with the specified channel capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            writers: Arc::default(),
            capacity,
            max_event_size: DEFAULT_MAX_EVENT_SIZE,
            write_timeout: DEFAULT_WRITE_TIMEOUT,
            max_concurrent_queues: None,
            metrics: None,
        }
    }

    /// Sets the write timeout for event queue sends.
    ///
    /// Retained for API compatibility. Broadcast-based queues do not block
    /// on writes, so this value is not actively used for backpressure.
    #[must_use]
    pub const fn with_write_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.write_timeout = timeout;
        self
    }

    /// Creates a new event queue manager with the specified maximum event size.
    ///
    /// Events exceeding this size (in serialized bytes) will be rejected with
    /// an error to prevent OOM conditions.
    #[must_use]
    pub const fn with_max_event_size(mut self, max_event_size: usize) -> Self {
        self.max_event_size = max_event_size;
        self
    }

    /// Sets the metrics hook for reporting queue depth changes.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Sets the maximum number of concurrent event queues.
    ///
    /// When the limit is reached, new queue creation will return an error
    /// reader (`None`) to signal capacity exhaustion.
    #[must_use]
    pub const fn with_max_concurrent_queues(mut self, max: usize) -> Self {
        self.max_concurrent_queues = Some(max);
        self
    }

    /// Returns the writer for the given task, creating a new queue if none
    /// exists.
    ///
    /// If a queue already exists, the returned reader is `None` (callers
    /// should use [`subscribe()`](Self::subscribe) to get additional readers
    /// for existing queues). If a new queue is created, both the writer and
    /// the first reader are returned.
    ///
    /// If `max_concurrent_queues` is set and the limit is reached, returns
    /// the writer with `None` reader (same as existing queue case).
    pub async fn get_or_create(
        &self,
        task_id: &TaskId,
    ) -> (Arc<InMemoryQueueWriter>, Option<InMemoryQueueReader>) {
        let mut map = self.writers.write().await;
        #[allow(clippy::option_if_let_else)]
        let result = if let Some(existing) = map.get(task_id) {
            (Arc::clone(existing), None)
        } else if self
            .max_concurrent_queues
            .is_some_and(|max| map.len() >= max)
        {
            // Concurrent queue limit reached — create a disconnected writer
            // so the caller gets an error when trying to use it.
            let (writer, _reader) = new_in_memory_queue_with_options(
                self.capacity,
                self.max_event_size,
                self.write_timeout,
            );
            (Arc::new(writer), None)
        } else {
            let (writer, reader) = new_in_memory_queue_with_options(
                self.capacity,
                self.max_event_size,
                self.write_timeout,
            );
            let writer = Arc::new(writer);
            map.insert(task_id.clone(), Arc::clone(&writer));
            (writer, Some(reader))
        };
        let queue_count = map.len();
        drop(map);
        if let Some(ref metrics) = self.metrics {
            metrics.on_queue_depth_change(queue_count);
        }
        result
    }

    /// Creates a new reader for an existing task's event queue.
    ///
    /// Returns `None` if no queue exists for the given task. The returned
    /// reader will receive all future events written to the queue.
    ///
    /// This enables `SubscribeToTask` (resubscribe) to work even when
    /// another SSE stream is already consuming events from the same queue.
    pub async fn subscribe(&self, task_id: &TaskId) -> Option<InMemoryQueueReader> {
        let map = self.writers.read().await;
        map.get(task_id).map(|writer| writer.subscribe())
    }

    /// Removes and drops the event queue for the given task.
    pub async fn destroy(&self, task_id: &TaskId) {
        let mut map = self.writers.write().await;
        map.remove(task_id);
        let queue_count = map.len();
        drop(map);
        if let Some(ref metrics) = self.metrics {
            metrics.on_queue_depth_change(queue_count);
        }
    }

    /// Returns the number of active event queues.
    pub async fn active_count(&self) -> usize {
        let map = self.writers.read().await;
        map.len()
    }

    /// Removes all event queues, causing all readers to see EOF.
    pub async fn destroy_all(&self) {
        let mut map = self.writers.write().await;
        map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
    use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

    /// Helper: create a minimal `StreamResponse::StatusUpdate` for testing.
    fn make_status_event(task_id: &str, state: TaskState) -> StreamResponse {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new(task_id),
            context_id: ContextId::new("ctx-test"),
            status: TaskStatus {
                state,
                message: None,
                timestamp: None,
            },
            metadata: None,
        })
    }

    // ── new_in_memory_queue constructors ─────────────────────────────────

    #[test]
    fn new_in_memory_queue_returns_pair() {
        let (_writer, _reader) = new_in_memory_queue();
        // Should compile and not panic.
    }

    #[test]
    fn new_in_memory_queue_with_capacity_returns_pair() {
        let (_writer, _reader) = new_in_memory_queue_with_capacity(128);
    }

    #[test]
    fn new_in_memory_queue_with_options_returns_pair() {
        let (_writer, _reader) =
            new_in_memory_queue_with_options(32, 1024, std::time::Duration::from_secs(1));
    }

    // ── write / read lifecycle ───────────────────────────────────────────

    #[tokio::test]
    async fn write_then_read_single_event() {
        let (writer, mut reader) = new_in_memory_queue();
        let event = make_status_event("t1", TaskState::Working);

        writer.write(event).await.expect("write should succeed");
        drop(writer);

        let received = reader.read().await;
        assert!(received.is_some(), "reader should return the written event");
        let result = received.unwrap();
        assert!(result.is_ok(), "event should be Ok");

        // After writer is dropped, reader should see EOF.
        let eof = reader.read().await;
        assert!(
            eof.is_none(),
            "reader should return None after writer is dropped"
        );
    }

    #[tokio::test]
    async fn write_multiple_events_read_in_order() {
        let (writer, mut reader) = new_in_memory_queue();

        let e1 = make_status_event("t1", TaskState::Working);
        let e2 = make_status_event("t1", TaskState::Completed);

        writer.write(e1).await.expect("first write should succeed");
        writer.write(e2).await.expect("second write should succeed");
        drop(writer);

        // Read first event.
        let r1 = reader.read().await.expect("should read first event");
        let sr1 = r1.expect("first event should be Ok");
        match &sr1 {
            StreamResponse::StatusUpdate(evt) => {
                assert_eq!(
                    evt.status.state,
                    TaskState::Working,
                    "first event should be Working"
                );
            }
            other => panic!("expected StatusUpdate, got: {other:?}"),
        }

        // Read second event.
        let r2 = reader.read().await.expect("should read second event");
        let sr2 = r2.expect("second event should be Ok");
        match &sr2 {
            StreamResponse::StatusUpdate(evt) => {
                assert_eq!(
                    evt.status.state,
                    TaskState::Completed,
                    "second event should be Completed"
                );
            }
            other => panic!("expected StatusUpdate, got: {other:?}"),
        }

        // EOF.
        assert!(
            reader.read().await.is_none(),
            "should be EOF after all events"
        );
    }

    // ── closed queue behavior ────────────────────────────────────────────

    #[tokio::test]
    async fn read_returns_none_on_empty_closed_queue() {
        let (writer, mut reader) = new_in_memory_queue();
        drop(writer); // close immediately without writing

        let result = reader.read().await;
        assert!(
            result.is_none(),
            "reading from an empty closed queue should return None"
        );
    }

    #[tokio::test]
    async fn write_after_all_readers_dropped_returns_error() {
        let (writer, reader) = new_in_memory_queue();
        drop(reader);

        let result = writer
            .write(make_status_event("t1", TaskState::Working))
            .await;
        assert!(
            result.is_err(),
            "writing with no active receivers should return an error"
        );
    }

    #[tokio::test]
    async fn close_is_no_op_and_succeeds() {
        let (writer, _reader) = new_in_memory_queue();
        let result = writer.close().await;
        assert!(result.is_ok(), "close() should succeed");
    }

    // ── subscribe creates independent readers ────────────────────────────

    #[tokio::test]
    async fn subscribe_creates_independent_reader() {
        let (writer, mut reader1) = new_in_memory_queue();
        let mut reader2 = writer.subscribe();

        let event = make_status_event("t1", TaskState::Working);
        writer.write(event).await.expect("write should succeed");
        drop(writer);

        // Both readers should receive the event independently.
        let r1 = reader1.read().await;
        assert!(r1.is_some(), "reader1 should receive the event");

        let r2 = reader2.read().await;
        assert!(r2.is_some(), "reader2 should receive the event");

        // Both should see EOF.
        assert!(reader1.read().await.is_none(), "reader1 should see EOF");
        assert!(reader2.read().await.is_none(), "reader2 should see EOF");
    }

    #[tokio::test]
    async fn subscriber_only_sees_events_after_subscribe() {
        let (writer, mut reader1) = new_in_memory_queue();

        // Write first event before subscribing.
        writer
            .write(make_status_event("t1", TaskState::Submitted))
            .await
            .expect("write should succeed");

        // Subscribe after the first event.
        let mut reader2 = writer.subscribe();

        // Write second event.
        writer
            .write(make_status_event("t1", TaskState::Working))
            .await
            .expect("write should succeed");
        drop(writer);

        // reader1 sees both events.
        let r1a = reader1
            .read()
            .await
            .expect("reader1 should see first event");
        assert!(r1a.is_ok());
        let r1b = reader1
            .read()
            .await
            .expect("reader1 should see second event");
        assert!(r1b.is_ok());
        assert!(reader1.read().await.is_none());

        // reader2 only sees the second event (subscribed after first).
        let r2a = reader2
            .read()
            .await
            .expect("reader2 should see second event");
        assert!(r2a.is_ok());
        assert!(
            reader2.read().await.is_none(),
            "reader2 should see EOF after the one event it received"
        );
    }

    // ── max event size enforcement ───────────────────────────────────────

    #[tokio::test]
    async fn oversized_event_is_rejected() {
        // Use a very small max_event_size to trigger rejection.
        let (writer, _reader) = new_in_memory_queue_with_options(
            16,
            10, // 10 bytes max — any real StreamResponse will exceed this
            DEFAULT_WRITE_TIMEOUT,
        );

        let event = make_status_event("t1", TaskState::Working);
        let result = writer.write(event).await;
        assert!(
            result.is_err(),
            "event exceeding max_event_size should be rejected"
        );
        let err = result.unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("exceeds maximum"),
            "error message should mention size limit, got: {msg}"
        );
    }

    #[tokio::test]
    async fn event_within_size_limit_is_accepted() {
        // Use a generous max_event_size.
        let (writer, mut reader) =
            new_in_memory_queue_with_options(16, DEFAULT_MAX_EVENT_SIZE, DEFAULT_WRITE_TIMEOUT);

        let event = make_status_event("t1", TaskState::Working);
        writer
            .write(event)
            .await
            .expect("event within size limit should be accepted");
        drop(writer);

        let r = reader.read().await;
        assert!(r.is_some(), "reader should receive the event");
    }

    // ── EventQueueManager ────────────────────────────────────────────────

    #[tokio::test]
    async fn manager_get_or_create_new_task() {
        let manager = EventQueueManager::new();
        let task_id = TaskId::new("task-1");

        let (writer, reader) = manager.get_or_create(&task_id).await;
        assert!(
            reader.is_some(),
            "first get_or_create should return a reader"
        );

        // Writing through the returned writer should succeed.
        writer
            .write(make_status_event("task-1", TaskState::Working))
            .await
            .expect("write through manager writer should succeed");

        assert_eq!(
            manager.active_count().await,
            1,
            "should have 1 active queue"
        );
    }

    #[tokio::test]
    async fn manager_get_or_create_existing_task_returns_no_reader() {
        let manager = EventQueueManager::new();
        let task_id = TaskId::new("task-1");

        let (_w1, r1) = manager.get_or_create(&task_id).await;
        assert!(r1.is_some(), "first call should return a reader");

        let (_w2, r2) = manager.get_or_create(&task_id).await;
        assert!(
            r2.is_none(),
            "second call for same task should return None reader"
        );

        assert_eq!(
            manager.active_count().await,
            1,
            "should still have only 1 active queue"
        );
    }

    #[tokio::test]
    async fn manager_subscribe_existing_task() {
        let manager = EventQueueManager::new();
        let task_id = TaskId::new("task-1");

        let (writer, _reader) = manager.get_or_create(&task_id).await;

        let sub = manager.subscribe(&task_id).await;
        assert!(
            sub.is_some(),
            "subscribe should return a reader for existing task"
        );

        let mut sub_reader = sub.unwrap();
        writer
            .write(make_status_event("task-1", TaskState::Working))
            .await
            .expect("write should succeed");
        drop(writer);

        let r = sub_reader.read().await;
        assert!(r.is_some(), "subscriber should receive the event");
    }

    #[tokio::test]
    async fn manager_subscribe_nonexistent_task_returns_none() {
        let manager = EventQueueManager::new();
        let task_id = TaskId::new("no-such-task");

        let sub = manager.subscribe(&task_id).await;
        assert!(
            sub.is_none(),
            "subscribe should return None for nonexistent task"
        );
    }

    #[tokio::test]
    async fn manager_destroy_removes_queue() {
        let manager = EventQueueManager::new();
        let task_id = TaskId::new("task-1");

        let (_writer, _reader) = manager.get_or_create(&task_id).await;
        assert_eq!(manager.active_count().await, 1);

        manager.destroy(&task_id).await;
        assert_eq!(
            manager.active_count().await,
            0,
            "destroy should remove the queue"
        );
    }

    #[tokio::test]
    async fn manager_destroy_all_clears_queues() {
        let manager = EventQueueManager::new();

        let _q1 = manager.get_or_create(&TaskId::new("t1")).await;
        let _q2 = manager.get_or_create(&TaskId::new("t2")).await;
        assert_eq!(manager.active_count().await, 2);

        manager.destroy_all().await;
        assert_eq!(
            manager.active_count().await,
            0,
            "destroy_all should clear all queues"
        );
    }

    #[tokio::test]
    async fn manager_max_concurrent_queues_enforced() {
        let manager = EventQueueManager::new().with_max_concurrent_queues(1);

        let (_w1, r1) = manager.get_or_create(&TaskId::new("t1")).await;
        assert!(r1.is_some(), "first queue should be created successfully");

        // Second queue creation should hit the limit.
        let (_w2, r2) = manager.get_or_create(&TaskId::new("t2")).await;
        assert!(
            r2.is_none(),
            "second queue should return None reader when limit is reached"
        );
        assert_eq!(
            manager.active_count().await,
            1,
            "should still have only 1 queue (second was not stored)"
        );
    }

    #[tokio::test]
    async fn manager_with_capacity_and_max_event_size() {
        let manager = EventQueueManager::with_capacity(4).with_max_event_size(10); // tiny limit

        let task_id = TaskId::new("t1");
        let (writer, _reader) = manager.get_or_create(&task_id).await;

        let event = make_status_event("t1", TaskState::Working);
        let result = writer.write(event).await;
        assert!(
            result.is_err(),
            "event should be rejected by the size limit configured on the manager"
        );
    }
}
