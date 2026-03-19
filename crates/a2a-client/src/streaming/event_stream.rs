// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Async SSE event stream with typed deserialization.
//!
//! [`EventStream`] provides an async `next()` iterator over
//! [`a2a_protocol_types::StreamResponse`] events received via Server-Sent Events.
//!
//! The stream terminates when:
//! - The underlying HTTP body closes (normal end-of-stream).
//! - A [`a2a_protocol_types::TaskStatusUpdateEvent`] with `final: true` is received.
//! - A protocol or transport error occurs (returned as `Some(Err(...))`).
//!
//! # Example
//!
//! ```rust,ignore
//! let mut stream = client.stream_message(params).await?;
//! while let Some(event) = stream.next().await {
//!     match event? {
//!         StreamResponse::StatusUpdate(ev) => {
//!             println!("State: {:?}", ev.state);
//!             if ev.r#final { break; }
//!         }
//!         StreamResponse::ArtifactUpdate(ev) => {
//!             println!("Artifact: {:?}", ev.artifact);
//!         }
//!         _ => {}
//!     }
//! }
//! ```

use a2a_protocol_types::{JsonRpcResponse, StreamResponse};
use hyper::body::Bytes;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use crate::error::{ClientError, ClientResult};
use crate::streaming::sse_parser::SseParser;

// ── Chunk ─────────────────────────────────────────────────────────────────────

/// A raw byte chunk from the HTTP body reader task.
pub(crate) type BodyChunk = ClientResult<Bytes>;

// ── EventStream ───────────────────────────────────────────────────────────────

/// An async stream of [`StreamResponse`] events from an SSE endpoint.
///
/// Created by [`crate::A2aClient::stream_message`] or
/// [`crate::A2aClient::subscribe_to_task`]. Call [`EventStream::next`] in a loop
/// to consume events.
///
/// When dropped, the background body-reader task is aborted to prevent
/// resource leaks.
pub struct EventStream {
    /// Channel receiver delivering raw byte chunks from the HTTP body.
    rx: mpsc::Receiver<BodyChunk>,
    /// SSE parser state machine.
    parser: SseParser,
    /// Whether the stream has been signalled as terminated.
    done: bool,
    /// Handle to abort the background body-reader task on drop.
    abort_handle: Option<AbortHandle>,
}

impl EventStream {
    /// Creates a new [`EventStream`] from a channel receiver (without abort handle).
    ///
    /// The channel must be fed raw HTTP body bytes from a background task.
    /// Prefer [`EventStream::with_abort_handle`] to ensure the background task
    /// is cancelled when the stream is dropped.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn new(rx: mpsc::Receiver<BodyChunk>) -> Self {
        Self {
            rx,
            parser: SseParser::new(),
            done: false,
            abort_handle: None,
        }
    }

    /// Creates a new [`EventStream`] with an abort handle for the body-reader task.
    ///
    /// When the `EventStream` is dropped, the abort handle is used to cancel
    /// the background task, preventing resource leaks.
    #[must_use]
    pub(crate) fn with_abort_handle(
        rx: mpsc::Receiver<BodyChunk>,
        abort_handle: AbortHandle,
    ) -> Self {
        Self {
            rx,
            parser: SseParser::new(),
            done: false,
            abort_handle: Some(abort_handle),
        }
    }

    /// Returns the next event from the stream.
    ///
    /// Returns `None` when the stream ends normally (either the HTTP body
    /// closed or a `final: true` event was received).
    ///
    /// Returns `Some(Err(...))` on transport or protocol errors.
    pub async fn next(&mut self) -> Option<ClientResult<StreamResponse>> {
        loop {
            // First, drain any frames the parser already has buffered.
            if let Some(result) = self.parser.next_frame() {
                match result {
                    Ok(frame) => return Some(self.decode_frame(&frame.data)),
                    Err(e) => {
                        return Some(Err(ClientError::Transport(e.to_string())));
                    }
                }
            }

            if self.done {
                return None;
            }

            // Need more bytes — wait for the next chunk from the body reader.
            match self.rx.recv().await {
                None => {
                    // Channel closed — body reader task exited.
                    self.done = true;
                    // Drain any remaining parser frames.
                    if let Some(result) = self.parser.next_frame() {
                        match result {
                            Ok(frame) => return Some(self.decode_frame(&frame.data)),
                            Err(e) => {
                                return Some(Err(ClientError::Transport(e.to_string())));
                            }
                        }
                    }
                    return None;
                }
                Some(Err(e)) => {
                    self.done = true;
                    return Some(Err(e));
                }
                Some(Ok(bytes)) => {
                    self.parser.feed(&bytes);
                }
            }
        }
    }

    // ── internals ─────────────────────────────────────────────────────────────

    fn decode_frame(&mut self, data: &str) -> ClientResult<StreamResponse> {
        // Each SSE frame's `data` is a JSON-RPC response carrying a StreamResponse.
        let envelope: JsonRpcResponse<StreamResponse> =
            serde_json::from_str(data).map_err(ClientError::Serialization)?;

        match envelope {
            JsonRpcResponse::Success(ok) => {
                // Check for terminal event so callers don't need to.
                if is_terminal(&ok.result) {
                    self.done = true;
                }
                Ok(ok.result)
            }
            JsonRpcResponse::Error(err) => {
                self.done = true;
                let a2a = a2a_protocol_types::A2aError::new(
                    a2a_protocol_types::ErrorCode::try_from(err.error.code)
                        .unwrap_or(a2a_protocol_types::ErrorCode::InternalError),
                    err.error.message,
                );
                Err(ClientError::Protocol(a2a))
            }
        }
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        if let Some(handle) = self.abort_handle.take() {
            handle.abort();
        }
    }
}

#[allow(clippy::missing_fields_in_debug)]
impl std::fmt::Debug for EventStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `rx` and `parser` don't implement Debug in a useful way; show key state only.
        f.debug_struct("EventStream")
            .field("done", &self.done)
            .field("pending_frames", &self.parser.pending_count())
            .finish()
    }
}

/// Returns `true` if `event` is the terminal event for its stream.
const fn is_terminal(event: &StreamResponse) -> bool {
    matches!(
        event,
        StreamResponse::StatusUpdate(ev) if ev.status.state.is_terminal()
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::{
        JsonRpcSuccessResponse, JsonRpcVersion, TaskId, TaskState, TaskStatus,
        TaskStatusUpdateEvent,
    };
    use std::time::Duration;

    /// Generous per-test timeout to prevent async tests from hanging
    /// when mutations break the SSE parser or event stream logic.
    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    fn make_status_event(state: TaskState, _is_final: bool) -> StreamResponse {
        StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t1"),
            context_id: a2a_protocol_types::ContextId::new("c1"),
            status: TaskStatus {
                state,
                message: None,
                timestamp: None,
            },
            metadata: None,
        })
    }

    fn sse_frame(event: &StreamResponse) -> String {
        let resp = JsonRpcSuccessResponse {
            jsonrpc: JsonRpcVersion,
            id: Some(serde_json::json!(1)),
            result: event.clone(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        format!("data: {json}\n\n")
    }

    #[tokio::test]
    async fn stream_delivers_events() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        let event = make_status_event(TaskState::Working, false);
        let sse_bytes = sse_frame(&event);
        tx.send(Ok(Bytes::from(sse_bytes))).await.unwrap();
        drop(tx);

        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out")
            .unwrap();
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), StreamResponse::StatusUpdate(_)));
    }

    #[tokio::test]
    async fn stream_ends_on_final_event() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        let event = make_status_event(TaskState::Completed, true);
        let sse_bytes = sse_frame(&event);
        tx.send(Ok(Bytes::from(sse_bytes))).await.unwrap();

        // First next() returns the final event.
        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out waiting for final event")
            .unwrap();
        assert!(result.is_ok());

        // Second next() returns None — stream is done.
        let end = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out waiting for stream end");
        assert!(end.is_none());
    }

    #[tokio::test]
    async fn stream_propagates_body_error() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        tx.send(Err(ClientError::Transport("network error".into())))
            .await
            .unwrap();

        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out")
            .unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stream_ends_when_channel_closed() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);
        drop(tx);

        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn drop_aborts_background_task() {
        let (tx, rx) = mpsc::channel::<BodyChunk>(8);
        // Spawn a task that will block forever unless aborted.
        let handle = tokio::spawn(async move {
            // Keep the sender alive so the channel doesn't close.
            let _tx = tx;
            // Sleep forever — this will be aborted by EventStream::drop.
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        });
        let abort_handle = handle.abort_handle();
        let stream = EventStream::with_abort_handle(rx, abort_handle);
        // Drop the stream, which should abort the task.
        drop(stream);
        // The spawned task should finish with a cancelled error.
        let result = tokio::time::timeout(TEST_TIMEOUT, handle)
            .await
            .expect("timed out waiting for task abort");
        assert!(result.is_err(), "task should have been aborted");
        assert!(
            result.unwrap_err().is_cancelled(),
            "task should be cancelled"
        );
    }

    #[test]
    fn debug_output_contains_fields() {
        let (_tx, rx) = mpsc::channel::<BodyChunk>(8);
        let stream = EventStream::new(rx);
        let debug = format!("{stream:?}");
        assert!(debug.contains("EventStream"), "should contain struct name");
        assert!(debug.contains("done"), "should contain 'done' field");
        assert!(
            debug.contains("pending_frames"),
            "should contain 'pending_frames' field"
        );
    }

    #[test]
    fn is_terminal_returns_false_for_working() {
        let event = make_status_event(TaskState::Working, false);
        assert!(!is_terminal(&event), "Working state should not be terminal");
    }

    #[test]
    fn is_terminal_returns_true_for_completed() {
        let event = make_status_event(TaskState::Completed, true);
        assert!(is_terminal(&event), "Completed state should be terminal");
    }

    /// Tests that an SSE frame containing a JSON-RPC error response
    /// is decoded as a `ClientError::Protocol`. Covers lines 164-171.
    #[tokio::test]
    async fn stream_decodes_jsonrpc_error_as_protocol_error() {
        use a2a_protocol_types::{JsonRpcErrorResponse, JsonRpcVersion};

        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        // Build a JSON-RPC error response frame.
        let error_resp = JsonRpcErrorResponse {
            jsonrpc: JsonRpcVersion,
            id: Some(serde_json::json!(1)),
            error: a2a_protocol_types::JsonRpcError {
                code: -32601,
                message: "method not found".into(),
                data: None,
            },
        };
        let json = serde_json::to_string(&error_resp).unwrap();
        let sse_data = format!("data: {json}\n\n");
        tx.send(Ok(Bytes::from(sse_data))).await.unwrap();
        drop(tx);

        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out")
            .unwrap();
        assert!(result.is_err(), "JSON-RPC error should produce Err");
        match result.unwrap_err() {
            ClientError::Protocol(err) => {
                assert!(
                    format!("{err}").contains("method not found"),
                    "error message should be preserved"
                );
            }
            other => panic!("expected Protocol error, got {other:?}"),
        }

        // Stream should be done after an error response.
        let end = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out");
        assert!(end.is_none(), "stream should end after JSON-RPC error");
    }

    /// Tests that invalid JSON in an SSE frame produces a serialization error.
    /// Covers the `decode_frame` path for malformed data.
    #[tokio::test]
    async fn stream_invalid_json_returns_serialization_error() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        let sse_data = "data: {not valid json}\n\n";
        tx.send(Ok(Bytes::from(sse_data))).await.unwrap();
        drop(tx);

        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out")
            .unwrap();
        assert!(result.is_err(), "invalid JSON should produce Err");
        assert!(
            matches!(result.unwrap_err(), ClientError::Serialization(_)),
            "should be a Serialization error"
        );
    }

    /// Tests that channel close with remaining parser data produces a frame.
    /// Covers lines 129-132 (drain after channel close).
    #[tokio::test]
    async fn stream_drains_parser_after_channel_close() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        // Send an event split across two chunks, then close the channel
        // before the event is complete (but the second chunk completes it).
        let event = make_status_event(TaskState::Working, false);
        let sse_bytes = sse_frame(&event);
        let (first_half, second_half) = sse_bytes.split_at(sse_bytes.len() / 2);

        tx.send(Ok(Bytes::from(first_half.to_owned())))
            .await
            .unwrap();
        tx.send(Ok(Bytes::from(second_half.to_owned())))
            .await
            .unwrap();
        drop(tx);

        let result = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out")
            .unwrap();
        assert!(result.is_ok(), "should deliver event from drained parser");
    }

    #[tokio::test]
    async fn non_terminal_event_does_not_end_stream() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        // Send a Working (non-terminal) event followed by another event.
        let working = make_status_event(TaskState::Working, false);
        let completed = make_status_event(TaskState::Completed, true);
        tx.send(Ok(Bytes::from(sse_frame(&working)))).await.unwrap();
        tx.send(Ok(Bytes::from(sse_frame(&completed))))
            .await
            .unwrap();

        // First call should return the Working event.
        let first = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out on first event")
            .unwrap()
            .unwrap();
        assert!(
            matches!(first, StreamResponse::StatusUpdate(ref ev) if ev.status.state == TaskState::Working)
        );

        // Second call should return the Completed event (stream didn't end early).
        let second = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out on second event")
            .unwrap()
            .unwrap();
        assert!(
            matches!(second, StreamResponse::StatusUpdate(ref ev) if ev.status.state == TaskState::Completed)
        );

        // Now the stream should be done because Completed is terminal.
        let end = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("timed out waiting for stream end");
        assert!(end.is_none());
    }
}
