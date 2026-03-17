// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! In-memory event queue backed by a `tokio::sync::broadcast` channel.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use tokio::sync::broadcast;

use super::{EventQueueReader, EventQueueWriter};

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
    /// Creates a new `InMemoryQueueWriter`.
    pub(super) fn new(
        tx: broadcast::Sender<A2aResult<StreamResponse>>,
        max_event_size: usize,
        write_timeout: std::time::Duration,
    ) -> Self {
        Self {
            tx,
            max_event_size,
            write_timeout,
        }
    }

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

impl InMemoryQueueReader {
    /// Creates a new `InMemoryQueueReader`.
    pub(super) fn new(rx: broadcast::Receiver<A2aResult<StreamResponse>>) -> Self {
        Self { rx }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::event_queue::{
        new_in_memory_queue, new_in_memory_queue_with_options, DEFAULT_MAX_EVENT_SIZE,
        DEFAULT_WRITE_TIMEOUT,
    };
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
}
