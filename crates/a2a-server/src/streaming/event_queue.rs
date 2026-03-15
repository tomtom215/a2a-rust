// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Event queue for server-side streaming.
//!
//! The executor writes [`StreamResponse`] events to an [`EventQueueWriter`];
//! the HTTP layer reads them from an [`EventQueueReader`] and serializes them
//! as SSE frames.
//!
//! [`InMemoryQueueWriter`] and [`InMemoryQueueReader`] are backed by a
//! bounded `tokio::sync::mpsc` channel.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_types::error::{A2aError, A2aResult};
use a2a_types::events::StreamResponse;
use a2a_types::task::TaskId;
use tokio::sync::{mpsc, RwLock};

/// Default channel capacity for event queues.
pub const DEFAULT_QUEUE_CAPACITY: usize = 64;

// ── EventQueueWriter ─────────────────────────────────────────────────────────

/// Trait for writing streaming events.
///
/// Object-safe; used as `&dyn EventQueueWriter` in the executor.
pub trait EventQueueWriter: Send + Sync + 'static {
    /// Writes a streaming event to the queue.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`] if the receiver has been dropped.
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

/// In-memory [`EventQueueWriter`] backed by an `mpsc` channel sender.
#[derive(Debug, Clone)]
pub struct InMemoryQueueWriter {
    tx: mpsc::Sender<A2aResult<StreamResponse>>,
}

#[allow(clippy::manual_async_fn)]
impl EventQueueWriter for InMemoryQueueWriter {
    fn write<'a>(
        &'a self,
        event: StreamResponse,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.tx
                .send(Ok(event))
                .await
                .map_err(|_| A2aError::internal("event queue receiver dropped"))
        })
    }

    fn close<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Dropping all senders closes the channel. Since `InMemoryQueueWriter`
            // is `Clone`, we cannot truly close here — but we can drop our permit
            // by sending nothing and letting the reader see `None` when all
            // senders are dropped. As a convention, send a synthetic close signal
            // isn't needed; the spawned task will drop its writer clone.
            Ok(())
        })
    }
}

// ── InMemoryQueueReader ──────────────────────────────────────────────────────

/// In-memory [`EventQueueReader`] backed by an `mpsc` channel receiver.
#[derive(Debug)]
pub struct InMemoryQueueReader {
    rx: mpsc::Receiver<A2aResult<StreamResponse>>,
}

impl EventQueueReader for InMemoryQueueReader {
    fn read(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Option<A2aResult<StreamResponse>>> + Send + '_>> {
        Box::pin(async move { self.rx.recv().await })
    }
}

// ── Constructor ──────────────────────────────────────────────────────────────

/// Creates a new in-memory event queue pair with the default capacity.
#[must_use]
pub fn new_in_memory_queue() -> (InMemoryQueueWriter, InMemoryQueueReader) {
    new_in_memory_queue_with_capacity(DEFAULT_QUEUE_CAPACITY)
}

/// Creates a new in-memory event queue pair with the specified capacity.
#[must_use]
pub fn new_in_memory_queue_with_capacity(
    capacity: usize,
) -> (InMemoryQueueWriter, InMemoryQueueReader) {
    let (tx, rx) = mpsc::channel(capacity);
    (InMemoryQueueWriter { tx }, InMemoryQueueReader { rx })
}

// ── EventQueueManager ────────────────────────────────────────────────────────

/// Manages event queues for active tasks.
///
/// Each task can have at most one active writer. When a client subscribes
/// (or resubscribes), the manager returns the existing writer and a fresh
/// reader, or creates both if none exists.
#[derive(Debug, Clone)]
pub struct EventQueueManager {
    writers: Arc<RwLock<HashMap<TaskId, Arc<InMemoryQueueWriter>>>>,
    /// Channel capacity for new event queues.
    capacity: usize,
}

impl Default for EventQueueManager {
    fn default() -> Self {
        Self {
            writers: Arc::default(),
            capacity: DEFAULT_QUEUE_CAPACITY,
        }
    }
}

impl EventQueueManager {
    /// Creates a new, empty event queue manager with default capacity.
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
        }
    }

    /// Returns the writer for the given task, creating a new queue if none
    /// exists.
    ///
    /// If a queue already exists, the returned reader is `None` (the original
    /// reader was given out at creation time). If a new queue is created, both
    /// the writer and reader are returned.
    pub async fn get_or_create(
        &self,
        task_id: &TaskId,
    ) -> (Arc<InMemoryQueueWriter>, Option<InMemoryQueueReader>) {
        let mut map = self.writers.write().await;
        #[allow(clippy::option_if_let_else)]
        let result = if let Some(existing) = map.get(task_id) {
            (Arc::clone(existing), None)
        } else {
            let (writer, reader) = new_in_memory_queue_with_capacity(self.capacity);
            let writer = Arc::new(writer);
            map.insert(task_id.clone(), Arc::clone(&writer));
            (writer, Some(reader))
        };
        drop(map);
        result
    }

    /// Removes and drops the event queue for the given task.
    pub async fn destroy(&self, task_id: &TaskId) {
        let mut map = self.writers.write().await;
        map.remove(task_id);
        drop(map);
    }
}
