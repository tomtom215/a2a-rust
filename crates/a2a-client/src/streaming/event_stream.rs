// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

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

        let result = stream.next().await.unwrap();
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
        let result = stream.next().await.unwrap();
        assert!(result.is_ok());

        // Second next() returns None — stream is done.
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn stream_propagates_body_error() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);

        tx.send(Err(ClientError::Transport("network error".into())))
            .await
            .unwrap();

        let result = stream.next().await.unwrap();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn stream_ends_when_channel_closed() {
        let (tx, rx) = mpsc::channel(8);
        let mut stream = EventStream::new(rx);
        drop(tx);

        assert!(stream.next().await.is_none());
    }
}
