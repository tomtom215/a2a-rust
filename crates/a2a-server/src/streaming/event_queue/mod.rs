// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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

mod in_memory;
mod manager;

pub use in_memory::{InMemoryQueueReader, InMemoryQueueWriter};
pub use manager::EventQueueManager;

use std::future::Future;
use std::pin::Pin;

#[allow(unused_imports)] // Used in doc comments.
use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use tokio::sync::broadcast;

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
        InMemoryQueueWriter::new(tx, max_event_size, write_timeout),
        InMemoryQueueReader::new(rx),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
