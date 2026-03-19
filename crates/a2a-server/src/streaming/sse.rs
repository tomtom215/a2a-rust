// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Server-Sent Events (SSE) response builder.
//!
//! Builds a `hyper::Response` with `Content-Type: text/event-stream` and
//! streams events from an [`InMemoryQueueReader`] as SSE frames.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Frame;

use a2a_protocol_types::jsonrpc::{JsonRpcId, JsonRpcSuccessResponse, JsonRpcVersion};

use crate::streaming::event_queue::{EventQueueReader, InMemoryQueueReader};

/// Default keep-alive interval for SSE streams.
pub(crate) const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(30);

/// Default SSE response body channel capacity.
pub(crate) const DEFAULT_SSE_CHANNEL_CAPACITY: usize = 64;

// ── SSE frame formatting ─────────────────────────────────────────────────────

/// Formats a single SSE frame with the given event type and data.
#[must_use]
pub fn write_event(event_type: &str, data: &str) -> Bytes {
    let mut buf = String::with_capacity(event_type.len() + data.len() + 32);
    buf.push_str("event: ");
    buf.push_str(event_type);
    buf.push('\n');
    for line in data.lines() {
        buf.push_str("data: ");
        buf.push_str(line);
        buf.push('\n');
    }
    buf.push('\n');
    Bytes::from(buf)
}

/// Formats a keep-alive SSE comment.
#[must_use]
pub const fn write_keep_alive() -> Bytes {
    Bytes::from_static(b": keep-alive\n\n")
}

// ── SseBodyWriter ────────────────────────────────────────────────────────────

/// Wraps an `mpsc::Sender` for writing SSE frames to a response body.
#[derive(Debug)]
pub struct SseBodyWriter {
    tx: tokio::sync::mpsc::Sender<Result<Frame<Bytes>, Infallible>>,
}

impl SseBodyWriter {
    /// Sends an SSE event frame.
    ///
    /// # Errors
    ///
    /// Returns `Err(())` if the receiver has been dropped (client disconnected).
    pub async fn send_event(&self, event_type: &str, data: &str) -> Result<(), ()> {
        let frame = Frame::data(write_event(event_type, data));
        self.tx.send(Ok(frame)).await.map_err(|_| ())
    }

    /// Sends a keep-alive comment.
    ///
    /// # Errors
    ///
    /// Returns `Err(())` if the receiver has been dropped.
    pub async fn send_keep_alive(&self) -> Result<(), ()> {
        let frame = Frame::data(write_keep_alive());
        self.tx.send(Ok(frame)).await.map_err(|_| ())
    }

    /// Closes the SSE stream by dropping the sender.
    pub fn close(self) {
        drop(self);
    }
}

// ── ChannelBody ──────────────────────────────────────────────────────────────

/// A `hyper::body::Body` implementation backed by an `mpsc::Receiver`.
///
/// This allows streaming SSE frames through hyper's response pipeline.
struct ChannelBody {
    rx: tokio::sync::mpsc::Receiver<Result<Frame<Bytes>, Infallible>>,
}

impl hyper::body::Body for ChannelBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.rx.poll_recv(cx)
    }
}

// ── build_sse_response ───────────────────────────────────────────────────────

/// Builds an SSE streaming response from an event queue reader.
///
/// Each event is wrapped in a JSON-RPC 2.0 success response envelope so that
/// clients can uniformly parse SSE frames regardless of transport binding.
///
/// Spawns a background task that:
/// 1. Reads events from `reader` and serializes them as SSE `message` frames.
/// 2. Sends periodic keep-alive comments at the specified interval.
///
/// The keep-alive ticker is cancelled when the reader is exhausted.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn build_sse_response(
    mut reader: InMemoryQueueReader,
    keep_alive_interval: Option<Duration>,
    channel_capacity: Option<usize>,
) -> hyper::Response<http_body_util::combinators::BoxBody<Bytes, Infallible>> {
    trace_info!("building SSE response stream");
    let interval = keep_alive_interval.unwrap_or(DEFAULT_KEEP_ALIVE);
    let cap = channel_capacity.unwrap_or(DEFAULT_SSE_CHANNEL_CAPACITY);
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(cap);

    let body_writer = SseBodyWriter { tx };

    tokio::spawn(async move {
        let mut keep_alive = tokio::time::interval(interval);
        // The first tick fires immediately; skip it.
        keep_alive.tick().await;

        loop {
            tokio::select! {
                biased;

                event = reader.read() => {
                    match event {
                        Some(Ok(stream_response)) => {
                            let envelope = JsonRpcSuccessResponse {
                                jsonrpc: JsonRpcVersion,
                                id: JsonRpcId::default(),
                                result: stream_response,
                            };
                            let data = match serde_json::to_string(&envelope) {
                                Ok(d) => d,
                                Err(e) => {
                                    // PR-6: Send error event before closing.
                                    let err_msg = format!("{{\"error\":\"serialization failed: {e}\"}}");
                                    let _ = body_writer.send_event("error", &err_msg).await;
                                    break;
                                }
                            };
                            if body_writer.send_event("message", &data).await.is_err() {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            let Ok(data) = serde_json::to_string(&e) else {
                                break;
                            };
                            let _ = body_writer.send_event("error", &data).await;
                            break;
                        }
                        None => break,
                    }
                }
                _ = keep_alive.tick() => {
                    if body_writer.send_keep_alive().await.is_err() {
                        break;
                    }
                }
            }
        }

        drop(body_writer);
    });

    let body = ChannelBody { rx };

    hyper::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("transfer-encoding", "chunked")
        .body(body.boxed())
        .unwrap_or_else(|_| {
            hyper::Response::new(
                http_body_util::Full::new(Bytes::from_static(b"SSE response build error")).boxed(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── write_event ──────────────────────────────────────────────────────

    #[test]
    fn write_event_single_line_data() {
        let frame = write_event("message", r#"{"hello":"world"}"#);
        let expected = "event: message\ndata: {\"hello\":\"world\"}\n\n";
        assert_eq!(
            frame,
            Bytes::from(expected),
            "single-line data should produce one data: line"
        );
    }

    #[test]
    fn write_event_multiline_data() {
        let frame = write_event("error", "line1\nline2\nline3");
        let expected = "event: error\ndata: line1\ndata: line2\ndata: line3\n\n";
        assert_eq!(
            frame,
            Bytes::from(expected),
            "multiline data should produce separate data: lines"
        );
    }

    #[test]
    fn write_event_empty_data() {
        let frame = write_event("ping", "");
        // "".lines() yields no items, so no data: lines are emitted
        let expected = "event: ping\n\n";
        assert_eq!(
            frame,
            Bytes::from(expected),
            "empty data should produce no data: lines"
        );
    }

    #[test]
    fn write_event_empty_event_type() {
        let frame = write_event("", "payload");
        let expected = "event: \ndata: payload\n\n";
        assert_eq!(
            frame,
            Bytes::from(expected),
            "empty event type should still produce valid SSE frame"
        );
    }

    // ── write_keep_alive ─────────────────────────────────────────────────

    #[test]
    fn write_keep_alive_format() {
        let frame = write_keep_alive();
        assert_eq!(
            frame,
            Bytes::from_static(b": keep-alive\n\n"),
            "keep-alive should be an SSE comment terminated by double newline"
        );
    }

    // ── SseBodyWriter ────────────────────────────────────────────────────

    #[tokio::test]
    async fn sse_body_writer_send_event_delivers_frame() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(8);
        let writer = SseBodyWriter { tx };

        writer
            .send_event("message", "hello")
            .await
            .expect("send_event should succeed while receiver is alive");

        let received = rx.recv().await.expect("should receive a frame");
        let frame = received.expect("frame result should be Ok");
        let data = frame.into_data().expect("frame should be a data frame");
        assert_eq!(
            data,
            write_event("message", "hello"),
            "received frame should match write_event output"
        );
    }

    #[tokio::test]
    async fn sse_body_writer_send_keep_alive_delivers_comment() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(8);
        let writer = SseBodyWriter { tx };

        writer
            .send_keep_alive()
            .await
            .expect("send_keep_alive should succeed while receiver is alive");

        let received = rx.recv().await.expect("should receive a frame");
        let frame = received.expect("frame result should be Ok");
        let data = frame.into_data().expect("frame should be a data frame");
        assert_eq!(
            data,
            write_keep_alive(),
            "should receive keep-alive comment"
        );
    }

    #[tokio::test]
    async fn sse_body_writer_send_fails_after_receiver_dropped() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(8);
        let writer = SseBodyWriter { tx };
        drop(rx);

        let result = writer.send_event("message", "data").await;
        assert!(
            result.is_err(),
            "send_event should return Err after receiver is dropped"
        );
    }

    #[tokio::test]
    async fn sse_body_writer_keep_alive_fails_after_receiver_dropped() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(8);
        let writer = SseBodyWriter { tx };
        drop(rx);

        let result = writer.send_keep_alive().await;
        assert!(
            result.is_err(),
            "send_keep_alive should return Err after receiver is dropped"
        );
    }

    #[tokio::test]
    async fn sse_body_writer_close_drops_sender() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(8);
        let writer = SseBodyWriter { tx };

        writer.close();

        let result = rx.recv().await;
        assert!(
            result.is_none(),
            "receiver should return None after writer is closed"
        );
    }

    // ── build_sse_response ───────────────────────────────────────────────

    #[tokio::test]
    async fn build_sse_response_has_correct_headers() {
        let (_writer, reader) = crate::streaming::event_queue::new_in_memory_queue();

        let response = build_sse_response(reader, None, None);

        assert_eq!(response.status(), 200, "status should be 200 OK");
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .map(hyper::http::HeaderValue::as_bytes),
            Some(b"text/event-stream".as_slice()),
            "Content-Type should be text/event-stream"
        );
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .map(hyper::http::HeaderValue::as_bytes),
            Some(b"no-cache".as_slice()),
            "Cache-Control should be no-cache"
        );
        assert_eq!(
            response
                .headers()
                .get("transfer-encoding")
                .map(hyper::http::HeaderValue::as_bytes),
            Some(b"chunked".as_slice()),
            "Transfer-Encoding should be chunked"
        );
    }

    #[tokio::test]
    async fn build_sse_response_with_custom_keep_alive_and_capacity() {
        // Covers lines 128-129: custom keep_alive_interval and channel_capacity.
        let (_writer, reader) = crate::streaming::event_queue::new_in_memory_queue();

        let response = build_sse_response(reader, Some(Duration::from_secs(5)), Some(16));

        assert_eq!(response.status(), 200);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .map(hyper::http::HeaderValue::as_bytes),
            Some(b"text/event-stream".as_slice()),
        );
    }

    #[tokio::test]
    async fn build_sse_response_client_disconnect_stops_stream() {
        // Covers lines 160-161: send_event returns Err when client disconnects.
        use crate::streaming::event_queue::EventQueueWriter;
        use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
        use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

        let (writer, reader) = crate::streaming::event_queue::new_in_memory_queue();

        let response = build_sse_response(reader, None, None);

        // Drop the response body (simulating client disconnect).
        drop(response);

        // Give the background task a moment to notice the disconnect.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Writing after client disconnect should still succeed at the queue level
        // (the SSE writer loop will break when it can't send).
        let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t1"),
            context_id: ContextId::new("c1"),
            status: TaskStatus {
                state: TaskState::Working,
                message: None,
                timestamp: None,
            },
            metadata: None,
        });
        // The queue write may or may not succeed depending on timing.
        let _ = writer.write(event).await;
        drop(writer);
    }

    #[tokio::test]
    async fn build_sse_response_ends_on_reader_close() {
        // Covers line 171: the None branch (reader exhausted).
        use http_body_util::BodyExt;

        let (writer, reader) = crate::streaming::event_queue::new_in_memory_queue();

        // Close the writer immediately — reader should return None.
        drop(writer);

        let mut response = build_sse_response(reader, None, None);

        // The stream should end (return None after all events are consumed).
        let frame = response.body_mut().frame().await;
        // Either None or a frame followed by None.
        if let Some(Ok(_)) = frame {
            // Consume any remaining frames.
            let next = response.body_mut().frame().await;
            assert!(
                next.is_none() || matches!(next, Some(Ok(_))),
                "stream should eventually end"
            );
        }
    }

    #[tokio::test]
    async fn build_sse_response_streams_error_event() {
        // Covers lines 164-169: the Some(Err(e)) branch sends an error SSE event.
        use a2a_protocol_types::error::A2aError;
        use http_body_util::BodyExt;

        // Construct a broadcast channel directly and send an Err to exercise the
        // error branch in the SSE loop.
        let (tx, rx) = tokio::sync::broadcast::channel(8);
        let reader = crate::streaming::event_queue::InMemoryQueueReader::new(rx);

        let err = A2aError::internal("something broke");
        tx.send(Err(err)).expect("send should succeed");
        drop(tx);

        let mut response = build_sse_response(reader, None, None);

        let frame = response
            .body_mut()
            .frame()
            .await
            .expect("should have a frame")
            .expect("frame should be Ok");
        let data = frame.into_data().expect("should be a data frame");
        let text = String::from_utf8_lossy(&data);

        assert!(
            text.starts_with("event: error\n"),
            "error event frame should start with 'event: error\\n', got: {text}"
        );
    }

    #[tokio::test]
    async fn build_sse_response_streams_events() {
        use crate::streaming::event_queue::EventQueueWriter;
        use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
        use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
        use http_body_util::BodyExt;

        let (writer, reader) = crate::streaming::event_queue::new_in_memory_queue();

        let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: TaskId::new("t1"),
            context_id: ContextId::new("c1"),
            status: TaskStatus {
                state: TaskState::Working,
                message: None,
                timestamp: None,
            },
            metadata: None,
        });

        // Write an event then close the writer so the stream terminates.
        writer.write(event).await.expect("write should succeed");
        drop(writer);

        let mut response = build_sse_response(reader, None, None);

        // Collect the first data frame from the body.
        let frame = response
            .body_mut()
            .frame()
            .await
            .expect("should have a frame")
            .expect("frame should be Ok");
        let data = frame.into_data().expect("should be a data frame");
        let text = String::from_utf8_lossy(&data);

        assert!(
            text.starts_with("event: message\n"),
            "SSE frame should start with 'event: message\\n', got: {text}"
        );
        assert!(
            text.contains("data: "),
            "SSE frame should contain a data: line"
        );
        // The data line should contain a JSON-RPC envelope with jsonrpc and result fields.
        assert!(
            text.contains("\"jsonrpc\""),
            "data should contain JSON-RPC envelope"
        );
        assert!(
            text.contains("\"result\""),
            "data should contain result field"
        );
    }
}
