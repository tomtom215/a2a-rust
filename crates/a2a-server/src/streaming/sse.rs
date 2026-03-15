// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

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

use a2a_types::jsonrpc::{JsonRpcId, JsonRpcSuccessResponse, JsonRpcVersion};

use crate::streaming::event_queue::{EventQueueReader, InMemoryQueueReader};

/// Default keep-alive interval for SSE streams.
const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(30);

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
pub fn build_sse_response(
    mut reader: InMemoryQueueReader,
    keep_alive_interval: Option<Duration>,
) -> hyper::Response<http_body_util::combinators::BoxBody<Bytes, Infallible>> {
    trace_info!("building SSE response stream");
    let interval = keep_alive_interval.unwrap_or(DEFAULT_KEEP_ALIVE);
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Frame<Bytes>, Infallible>>(64);

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
                            // Wrap in a JSON-RPC success envelope so the client
                            // can parse it uniformly.
                            let envelope = JsonRpcSuccessResponse {
                                jsonrpc: JsonRpcVersion,
                                id: JsonRpcId::default(),
                                result: stream_response,
                            };
                            let Ok(data) = serde_json::to_string(&envelope) else {
                                break;
                            };
                            if body_writer.send_event("message", &data).await.is_err() {
                                break;
                            }
                        }
                        Some(Err(e)) => {
                            // Best-effort: send the error as an SSE event before closing.
                            // If the client has already disconnected, this is harmless.
                            let Ok(data) = serde_json::to_string(&e) else {
                                break;
                            };
                            // Intentionally ignoring send failure — we're closing regardless.
                            let _ = body_writer.send_event("error", &data).await;
                            break;
                        }
                        None => {
                            // Stream closed.
                            break;
                        }
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
            // Fallback: empty 500 response if builder fails (should never happen
            // with valid static header names).
            hyper::Response::new(
                http_body_util::Full::new(Bytes::from_static(b"SSE response build error"))
                    .boxed(),
            )
        })
}
