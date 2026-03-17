// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! WebSocket transport implementation for A2A clients.
//!
//! [`WebSocketTransport`] opens a persistent WebSocket connection to the agent
//! and multiplexes JSON-RPC 2.0 requests over text frames.
//!
//! # Streaming
//!
//! For streaming methods (`SendStreamingMessage`, `SubscribeToTask`), the server
//! sends multiple text frames — one per event — followed by a final JSON-RPC
//! success response. The transport delivers these as an [`EventStream`].
//!
//! # Feature gate
//!
//! Requires the `websocket` feature flag:
//!
//! ```toml
//! a2a-protocol-client = { version = "0.2", features = ["websocket"] }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use a2a_protocol_types::{JsonRpcRequest, JsonRpcResponse};

use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

// ── WebSocketTransport ───────────────────────────────────────────────────────

/// WebSocket transport: JSON-RPC 2.0 over a persistent WebSocket connection.
///
/// Create via [`WebSocketTransport::connect`] and pass to
/// [`crate::ClientBuilder::with_custom_transport`].
///
/// The transport opens a single WebSocket connection that is reused across
/// all requests. Requests are serialized through a mutex to ensure only one
/// request/response pair is in-flight at a time.
pub struct WebSocketTransport {
    inner: Arc<Inner>,
}

type WsWriter = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    WsMessage,
>;

type WsReader = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

struct Inner {
    writer: Mutex<WsWriter>,
    reader: Mutex<WsReader>,
    endpoint: String,
    request_timeout: Duration,
}

impl WebSocketTransport {
    /// Connects to the agent's WebSocket endpoint.
    ///
    /// The `endpoint` should use the `ws://` or `wss://` scheme.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] if the WebSocket handshake fails.
    pub async fn connect(endpoint: impl Into<String>) -> ClientResult<Self> {
        Self::connect_with_timeout(endpoint, Duration::from_secs(30)).await
    }

    /// Connects with a custom request timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] if the WebSocket handshake fails.
    pub async fn connect_with_timeout(
        endpoint: impl Into<String>,
        request_timeout: Duration,
    ) -> ClientResult<Self> {
        let endpoint = endpoint.into();
        validate_ws_url(&endpoint)?;

        let (ws_stream, _resp) = tokio_tungstenite::connect_async(&endpoint)
            .await
            .map_err(|e| ClientError::Transport(format!("WebSocket connect failed: {e}")))?;

        let (writer, reader) = ws_stream.split();

        Ok(Self {
            inner: Arc::new(Inner {
                writer: Mutex::new(writer),
                reader: Mutex::new(reader),
                endpoint,
                request_timeout,
            }),
        })
    }

    /// Returns the endpoint URL this transport is connected to.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.inner.endpoint
    }

    /// Sends a JSON-RPC request and reads a single response.
    async fn execute_request(
        &self,
        method: &str,
        params: serde_json::Value,
        _extra_headers: &HashMap<String, String>,
    ) -> ClientResult<serde_json::Value> {
        trace_info!(method, endpoint = %self.inner.endpoint, "sending WebSocket JSON-RPC request");

        let rpc_req = build_rpc_request(method, params);
        let body = serde_json::to_string(&rpc_req).map_err(ClientError::Serialization)?;

        // Lock writer, send, then release before locking reader.
        let mut writer = self.inner.writer.lock().await;
        writer
            .send(WsMessage::Text(body))
            .await
            .map_err(|e| ClientError::Transport(format!("WebSocket send failed: {e}")))?;
        drop(writer);

        let mut reader = self.inner.reader.lock().await;
        let response_text =
            tokio::time::timeout(self.inner.request_timeout, read_text(&mut reader))
                .await
                .map_err(|_| ClientError::Timeout("WebSocket response timed out".into()))?
                .map_err(|e| ClientError::Transport(format!("WebSocket read failed: {e}")))?;
        drop(reader);

        let envelope: JsonRpcResponse<serde_json::Value> =
            serde_json::from_str(&response_text).map_err(ClientError::Serialization)?;

        match envelope {
            JsonRpcResponse::Success(ok) => {
                trace_info!(method, "WebSocket request succeeded");
                Ok(ok.result)
            }
            JsonRpcResponse::Error(err) => {
                trace_warn!(
                    method,
                    code = err.error.code,
                    "JSON-RPC error over WebSocket"
                );
                let a2a = a2a_protocol_types::A2aError::new(
                    a2a_protocol_types::ErrorCode::try_from(err.error.code)
                        .unwrap_or(a2a_protocol_types::ErrorCode::InternalError),
                    err.error.message,
                );
                Err(ClientError::Protocol(a2a))
            }
        }
    }

    /// Sends a JSON-RPC request and returns a stream of responses.
    async fn execute_streaming_request(
        &self,
        method: &str,
        params: serde_json::Value,
        _extra_headers: &HashMap<String, String>,
    ) -> ClientResult<EventStream> {
        trace_info!(method, endpoint = %self.inner.endpoint, "opening WebSocket stream");

        let rpc_req = build_rpc_request(method, params);
        let body = serde_json::to_string(&rpc_req).map_err(ClientError::Serialization)?;

        let mut writer = self.inner.writer.lock().await;
        writer
            .send(WsMessage::Text(body))
            .await
            .map_err(|e| ClientError::Transport(format!("WebSocket send failed: {e}")))?;
        drop(writer);

        // Create a channel-based EventStream. A background task reads frames
        // from the WebSocket and converts them to SSE-like data lines that the
        // existing EventStream parser can consume.
        let (tx, rx) = mpsc::channel::<crate::streaming::event_stream::BodyChunk>(64);

        // We need to take the reader out temporarily for the spawned task.
        // Since the transport serializes requests, we pass the reader into the
        // background task and restore it when done.
        let reader = self.inner.reader.lock().await;
        // We can't move out of a MutexGuard, so we use a different approach:
        // spawn a task that holds the reader lock for the duration of streaming.
        drop(reader);

        let inner = Arc::clone(&self.inner);
        let task_handle = tokio::spawn(async move {
            ws_stream_reader_task(inner, tx).await;
        });

        Ok(EventStream::with_abort_handle(
            rx,
            task_handle.abort_handle(),
        ))
    }
}

impl Transport for WebSocketTransport {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(self.execute_request(method, params, extra_headers))
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(self.execute_streaming_request(method, params, extra_headers))
    }
}

impl std::fmt::Debug for WebSocketTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebSocketTransport")
            .field("endpoint", &self.inner.endpoint)
            .finish()
    }
}

// ── Background stream reader ─────────────────────────────────────────────────

/// Reads WebSocket text frames and feeds them to the `EventStream` channel as
/// SSE-formatted data lines (reusing the existing SSE parser in `EventStream`).
async fn ws_stream_reader_task(
    inner: Arc<Inner>,
    tx: mpsc::Sender<crate::streaming::event_stream::BodyChunk>,
) {
    let mut reader = inner.reader.lock().await;

    loop {
        match reader.next().await {
            Some(Ok(WsMessage::Text(text))) => {
                // Wrap each JSON-RPC frame as an SSE data line so the existing
                // EventStream SSE parser can decode it.
                let sse_line = format!("data: {text}\n\n");
                if tx
                    .send(Ok(hyper::body::Bytes::from(sse_line)))
                    .await
                    .is_err()
                {
                    break; // Consumer dropped
                }

                // Check if this is the final response by parsing the JSON-RPC
                // envelope and looking for a terminal task state in the result.
                // This replaces fragile string matching with proper deserialization.
                if is_stream_terminal(&text) {
                    break;
                }
            }
            Some(Ok(WsMessage::Close(_))) | None => break,
            Some(Ok(_)) => {}
            Some(Err(e)) => {
                let _ = tx
                    .send(Err(ClientError::Transport(format!(
                        "WebSocket read error: {e}"
                    ))))
                    .await;
                break;
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Checks whether a JSON-RPC frame represents a terminal streaming event.
///
/// A stream is terminal when the result contains a status update with a
/// terminal task state (`completed`, `failed`, `canceled`, `rejected`),
/// or when the frame is a `stream_complete` sentinel.
///
/// Uses structural JSON inspection rather than fragile string matching
/// to avoid false positives from payload content containing those words.
fn is_stream_terminal(text: &str) -> bool {
    let Ok(frame) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };

    // Helper: check whether a JSON object contains a terminal task state
    // at one of the known locations (statusUpdate.status.state or status.state).
    let has_terminal_state = |obj: &serde_json::Value| -> bool {
        // Check for terminal status in statusUpdate
        if let Some(status_update) = obj.get("statusUpdate") {
            if let Some(status) = status_update.get("status") {
                if let Some(state) = status.get("state").and_then(|s| s.as_str()) {
                    return matches!(state, "completed" | "failed" | "canceled" | "rejected");
                }
            }
        }
        // Check for terminal status in a full task response
        if let Some(status) = obj.get("status") {
            if let Some(state) = status.get("state").and_then(|s| s.as_str()) {
                return matches!(state, "completed" | "failed" | "canceled" | "rejected");
            }
        }
        false
    };

    // If the frame is a JSON-RPC envelope, inspect the result field.
    if let Some(r) = frame.get("result") {
        // Check for explicit stream_complete sentinel.
        // The server may send either {"stream_complete": true} or
        // {"status": "stream_complete"}.
        if r.get("stream_complete").is_some() {
            return true;
        }
        if r.get("status").and_then(|s| s.as_str()) == Some("stream_complete") {
            return true;
        }
        return has_terminal_state(r);
    }

    // The frame may be a raw StreamResponse (not wrapped in a JSON-RPC envelope).
    // This happens when the server sends streaming events as bare JSON objects.
    has_terminal_state(&frame)
}

fn build_rpc_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
    let id = serde_json::Value::String(Uuid::new_v4().to_string());
    JsonRpcRequest::with_params(id, method, params)
}

/// Reads the next text frame from the WebSocket.
async fn read_text(reader: &mut WsReader) -> Result<String, tokio_tungstenite::tungstenite::Error> {
    loop {
        match reader.next().await {
            Some(Ok(WsMessage::Text(text))) => return Ok(text),
            Some(Ok(WsMessage::Close(_))) | None => {
                return Err(tokio_tungstenite::tungstenite::Error::ConnectionClosed);
            }
            Some(Ok(_)) => {} // Ping, Pong, Binary — skip
            Some(Err(e)) => return Err(e),
        }
    }
}

fn validate_ws_url(url: &str) -> ClientResult<()> {
    if url.is_empty() {
        return Err(ClientError::InvalidEndpoint("URL must not be empty".into()));
    }
    if !url.starts_with("ws://") && !url.starts_with("wss://") {
        return Err(ClientError::InvalidEndpoint(format!(
            "WebSocket URL must start with ws:// or wss://: {url}"
        )));
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_ws_url_rejects_empty() {
        assert!(validate_ws_url("").is_err());
    }

    #[test]
    fn validate_ws_url_rejects_http() {
        assert!(validate_ws_url("http://localhost:8080").is_err());
    }

    #[test]
    fn validate_ws_url_accepts_ws() {
        assert!(validate_ws_url("ws://localhost:8080").is_ok());
    }

    #[test]
    fn validate_ws_url_accepts_wss() {
        assert!(validate_ws_url("wss://agent.example.com/a2a").is_ok());
    }

    #[test]
    fn is_stream_terminal_completed_status() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"statusUpdate":{"status":{"state":"completed"}}}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_failed_status() {
        let frame =
            r#"{"jsonrpc":"2.0","id":"1","result":{"statusUpdate":{"status":{"state":"failed"}}}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_working_is_not_terminal() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"statusUpdate":{"status":{"state":"working"}}}}"#;
        assert!(!is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_stream_complete_sentinel() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"stream_complete":true}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_artifact_not_terminal() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"artifactUpdate":{"artifact":{"id":"a1","parts":[]}}}}"#;
        assert!(!is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_payload_containing_word_not_terminal() {
        // Payload text containing "completed" should NOT trigger termination
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"artifactUpdate":{"artifact":{"id":"a1","parts":[{"text":"task completed successfully"}]}}}}"#;
        assert!(!is_stream_terminal(frame));
    }

    #[test]
    fn build_rpc_request_has_method() {
        let req = build_rpc_request("TestMethod", serde_json::json!({"key": "val"}));
        assert_eq!(req.method, "TestMethod");
        assert!(req.params.is_some());
        // ID should be a UUID string
        assert!(req.id.is_some());
    }

    #[test]
    fn is_stream_terminal_invalid_json() {
        assert!(!is_stream_terminal("not json"));
    }

    #[test]
    fn is_stream_terminal_no_result() {
        assert!(!is_stream_terminal(r#"{"jsonrpc":"2.0","id":"1"}"#));
    }

    #[test]
    fn is_stream_terminal_task_level_completed() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"status":{"state":"completed"}}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_canceled() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"statusUpdate":{"status":{"state":"canceled"}}}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_rejected() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"statusUpdate":{"status":{"state":"rejected"}}}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_task_level_failed() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"status":{"state":"failed"}}}"#;
        assert!(is_stream_terminal(frame));
    }

    #[test]
    fn is_stream_terminal_non_string_state() {
        let frame = r#"{"jsonrpc":"2.0","id":"1","result":{"status":{"state":42}}}"#;
        assert!(!is_stream_terminal(frame));
    }

    #[test]
    fn validate_ws_url_rejects_https() {
        assert!(validate_ws_url("https://example.com").is_err());
    }

    #[test]
    fn validate_ws_url_error_message_contains_url() {
        let err = validate_ws_url("http://bad").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("http://bad") || msg.contains("ws://"));
    }
}
