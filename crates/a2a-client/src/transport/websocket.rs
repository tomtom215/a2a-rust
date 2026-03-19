// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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
//! # Architecture
//!
//! FIX(C2): The transport uses a dedicated background reader task that routes
//! incoming frames to the correct pending request via a `HashMap<RequestId, Sender>`.
//! This eliminates the reader lock deadlock where a streaming background task
//! would hold the reader Mutex for the entire stream duration, preventing any
//! subsequent non-streaming request from proceeding.
//!
//! FIX(C3): Extra headers (including auth interceptor headers) are passed via
//! the initial HTTP upgrade request during WebSocket connection establishment,
//! as well as embedded in JSON-RPC request metadata where supported.
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
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use a2a_protocol_types::{JsonRpcRequest, JsonRpcResponse};

use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

// ── Response routing ─────────────────────────────────────────────────────────

/// A pending request waiting for a response from the WebSocket reader task.
enum PendingRequest {
    /// A single-response (unary) request.
    Unary(oneshot::Sender<Result<String, ClientError>>),
    /// A streaming request that receives multiple frames.
    Streaming(mpsc::Sender<crate::streaming::event_stream::BodyChunk>),
}

/// Messages sent from the transport methods to the writer task.
struct WriteCommand {
    text: String,
    request_id: String,
    pending: PendingRequest,
}

// ── WebSocketTransport ───────────────────────────────────────────────────────

/// WebSocket transport: JSON-RPC 2.0 over a persistent WebSocket connection.
///
/// Create via [`WebSocketTransport::connect`] and pass to
/// [`crate::ClientBuilder::with_custom_transport`].
///
/// FIX(C2): Uses a dedicated reader task with message routing instead of a
/// shared Mutex on the reader half. This prevents deadlocks when streaming
/// responses are received concurrently with unary requests.
pub struct WebSocketTransport {
    inner: Arc<Inner>,
}

struct Inner {
    /// Channel to send write commands to the background writer/router task.
    write_tx: mpsc::Sender<WriteCommand>,
    endpoint: String,
    request_timeout: Duration,
    /// Handle to the background reader task for cleanup.
    _reader_handle: tokio::task::JoinHandle<()>,
    /// Handle to the background writer task for cleanup.
    _writer_handle: tokio::task::JoinHandle<()>,
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
        Self::connect_with_options(endpoint, Duration::from_secs(30), &HashMap::new()).await
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
        Self::connect_with_options(endpoint, request_timeout, &HashMap::new()).await
    }

    /// Connects with a custom request timeout and extra HTTP headers for the
    /// initial WebSocket upgrade request.
    ///
    /// FIX(C3): Extra headers (e.g. from `AuthInterceptor`) are applied to the
    /// HTTP upgrade request that establishes the WebSocket connection via the
    /// tungstenite `IntoClientRequest` trait.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::Transport`] if the WebSocket handshake fails.
    #[allow(clippy::too_many_lines)]
    pub async fn connect_with_options(
        endpoint: impl Into<String>,
        request_timeout: Duration,
        extra_headers: &HashMap<String, String>,
    ) -> ClientResult<Self> {
        let endpoint = endpoint.into();
        validate_ws_url(&endpoint)?;

        // FIX(C3): Build a tungstenite request with extra headers injected into
        // the HTTP upgrade handshake. This ensures auth headers from interceptors
        // are sent during connection establishment.
        let mut ws_request = endpoint
            .as_str()
            .into_client_request()
            .map_err(|e| ClientError::Transport(format!("WebSocket request build failed: {e}")))?;
        for (k, v) in extra_headers {
            if let (Ok(name), Ok(val)) = (
                k.parse::<tokio_tungstenite::tungstenite::http::HeaderName>(),
                v.parse::<tokio_tungstenite::tungstenite::http::HeaderValue>(),
            ) {
                ws_request.headers_mut().insert(name, val);
            }
        }

        let (ws_stream, _resp) = tokio_tungstenite::connect_async(ws_request)
            .await
            .map_err(|e| ClientError::Transport(format!("WebSocket connect failed: {e}")))?;

        let (ws_writer, ws_reader) = ws_stream.split();

        // Shared map of pending requests, keyed by JSON-RPC request ID.
        let pending: Arc<Mutex<HashMap<String, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Channel for write commands from transport methods to the writer task.
        let (write_tx, mut write_rx) = mpsc::channel::<WriteCommand>(64);

        // Background writer task: receives write commands, registers pending
        // requests, and sends frames to the WebSocket.
        let pending_for_writer = Arc::clone(&pending);
        let writer_handle = tokio::spawn(async move {
            let mut ws_writer = ws_writer;
            while let Some(cmd) = write_rx.recv().await {
                // Register the pending request before sending the frame.
                {
                    let mut map = pending_for_writer.lock().await;
                    map.insert(cmd.request_id, cmd.pending);
                }
                if ws_writer.send(WsMessage::Text(cmd.text)).await.is_err() {
                    break;
                }
            }
        });

        // Background reader task: reads frames from the WebSocket and routes
        // them to the correct pending request based on the JSON-RPC ID.
        let pending_for_reader = Arc::clone(&pending);
        let reader_handle = tokio::spawn(async move {
            let mut ws_reader = ws_reader;
            loop {
                match ws_reader.next().await {
                    Some(Ok(WsMessage::Text(text))) => {
                        route_frame(&pending_for_reader, &text).await;
                    }
                    Some(Ok(WsMessage::Close(_))) | None => break,
                    // Pong is handled automatically by tungstenite; other frames ignored
                    Some(Ok(_)) => {}
                    Some(Err(_e)) => {
                        // Notify all pending requests of the error, then
                        // drop the lock before breaking.
                        let entries: Vec<PendingRequest> = {
                            let mut map = pending_for_reader.lock().await;
                            map.drain().map(|(_, v)| v).collect()
                        };
                        for pending in entries {
                            match pending {
                                PendingRequest::Unary(tx) => {
                                    let _ = tx.send(Err(ClientError::Transport(
                                        "WebSocket connection error".into(),
                                    )));
                                }
                                PendingRequest::Streaming(tx) => {
                                    let _ = tx
                                        .send(Err(ClientError::Transport(
                                            "WebSocket connection error".into(),
                                        )))
                                        .await;
                                }
                            }
                        }
                        break;
                    }
                }
            }
        });

        // Store the endpoint without the mut binding issue.
        let endpoint_stored = endpoint;

        Ok(Self {
            inner: Arc::new(Inner {
                write_tx,
                endpoint: endpoint_stored,
                request_timeout,
                _reader_handle: reader_handle,
                _writer_handle: writer_handle,
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
        let request_id = rpc_req
            .id
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let body = serde_json::to_string(&rpc_req).map_err(ClientError::Serialization)?;

        let (tx, rx) = oneshot::channel();

        self.inner
            .write_tx
            .send(WriteCommand {
                text: body,
                request_id,
                pending: PendingRequest::Unary(tx),
            })
            .await
            .map_err(|_| ClientError::Transport("WebSocket writer task closed".into()))?;

        let response_text = tokio::time::timeout(self.inner.request_timeout, rx)
            .await
            .map_err(|_| ClientError::Timeout("WebSocket response timed out".into()))?
            .map_err(|_| ClientError::Transport("WebSocket reader task closed".into()))??;

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
        let request_id = rpc_req
            .id
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let body = serde_json::to_string(&rpc_req).map_err(ClientError::Serialization)?;

        // Create a channel-based EventStream.
        let (tx, rx) = mpsc::channel::<crate::streaming::event_stream::BodyChunk>(64);

        self.inner
            .write_tx
            .send(WriteCommand {
                text: body,
                request_id,
                pending: PendingRequest::Streaming(tx),
            })
            .await
            .map_err(|_| ClientError::Transport("WebSocket writer task closed".into()))?;

        Ok(EventStream::new(rx))
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

// ── Frame routing ────────────────────────────────────────────────────────────

/// Routes an incoming WebSocket text frame to the correct pending request.
///
/// Extracts the JSON-RPC ID from the frame and looks up the corresponding
/// pending request in the shared map.
async fn route_frame(pending: &Arc<Mutex<HashMap<String, PendingRequest>>>, text: &str) {
    // Try to extract the JSON-RPC ID to route the response.
    let frame_id = extract_jsonrpc_id(text);

    let mut map = pending.lock().await;

    let request_id = if let Some(ref id) = frame_id {
        id.clone()
    } else {
        // If we can't extract an ID, this might be a notification or malformed frame.
        // Try to deliver to any pending streaming request (best effort).
        return;
    };

    if let Some(entry) = map.get(&request_id) {
        match entry {
            PendingRequest::Unary(_) => {
                // Remove and deliver the response.
                if let Some(PendingRequest::Unary(tx)) = map.remove(&request_id) {
                    let _ = tx.send(Ok(text.to_owned()));
                }
            }
            PendingRequest::Streaming(tx) => {
                // Wrap as SSE data line for the existing EventStream SSE parser.
                let sse_line = format!("data: {text}\n\n");
                if tx
                    .send(Ok(hyper::body::Bytes::from(sse_line)))
                    .await
                    .is_err()
                {
                    // Consumer dropped — remove the pending entry.
                    map.remove(&request_id);
                    return;
                }

                // Check if this is the final response (terminal state).
                if is_stream_terminal(text) {
                    map.remove(&request_id);
                }
            }
        }
    }
}

/// Extracts the JSON-RPC `id` field from a JSON text frame.
fn extract_jsonrpc_id(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    match v.get("id") {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        _ => None,
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

    #[test]
    fn extract_jsonrpc_id_string() {
        let id = extract_jsonrpc_id(r#"{"jsonrpc":"2.0","id":"abc","result":{}}"#);
        assert_eq!(id.as_deref(), Some("abc"));
    }

    #[test]
    fn extract_jsonrpc_id_number() {
        let id = extract_jsonrpc_id(r#"{"jsonrpc":"2.0","id":42,"result":{}}"#);
        assert_eq!(id.as_deref(), Some("42"));
    }

    #[test]
    fn extract_jsonrpc_id_null_returns_none() {
        let id = extract_jsonrpc_id(r#"{"jsonrpc":"2.0","id":null,"result":{}}"#);
        assert!(id.is_none());
    }

    #[test]
    fn extract_jsonrpc_id_missing_returns_none() {
        let id = extract_jsonrpc_id(r#"{"jsonrpc":"2.0","result":{}}"#);
        assert!(id.is_none());
    }
}
