// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! WebSocket dispatcher for bidirectional A2A communication.
//!
//! Provides [`WebSocketDispatcher`] that upgrades HTTP connections to WebSocket
//! and handles JSON-RPC messages over the WebSocket channel. Streaming responses
//! are sent as individual WebSocket text frames rather than SSE.
//!
//! # Protocol
//!
//! - Client sends JSON-RPC 2.0 requests as text frames
//! - Server responds with JSON-RPC 2.0 responses as text frames
//! - For streaming methods (`SendStreamingMessage`, `SubscribeToTask`), the
//!   server sends multiple frames: one per SSE event, followed by a final
//!   JSON-RPC success response
//! - Connection closes cleanly on WebSocket close frame
//!
//! # Feature gate
//!
//! Requires the `websocket` feature flag:
//!
//! ```toml
//! a2a-protocol-server = { version = "0.2", features = ["websocket"] }
//! ```

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::WebSocketStream;

use a2a_protocol_types::jsonrpc::{
    JsonRpcError, JsonRpcErrorResponse, JsonRpcId, JsonRpcRequest, JsonRpcSuccessResponse,
    JsonRpcVersion,
};

use crate::error::ServerError;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::EventQueueReader;

/// WebSocket-based A2A dispatcher.
///
/// Accepts WebSocket connections and processes JSON-RPC 2.0 messages over the
/// WebSocket channel. Streaming responses are sent as individual text frames.
pub struct WebSocketDispatcher {
    handler: Arc<RequestHandler>,
}

impl WebSocketDispatcher {
    /// Creates a new WebSocket dispatcher.
    #[must_use]
    pub const fn new(handler: Arc<RequestHandler>) -> Self {
        Self { handler }
    }

    /// Starts a WebSocket server on the given address.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] if the TCP listener fails to bind.
    pub async fn serve(
        self: Arc<Self>,
        addr: impl tokio::net::ToSocketAddrs,
    ) -> std::io::Result<()> {
        let listener = TcpListener::bind(addr).await?;

        trace_info!(
            addr = %listener.local_addr().unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0))),
            "A2A WebSocket server listening"
        );

        loop {
            let (stream, _peer) = listener.accept().await?;
            let dispatcher = Arc::clone(&self);
            tokio::spawn(async move {
                trace_debug!("WebSocket connection accepted");
                if let Err(_e) = dispatcher.handle_connection(stream).await {
                    trace_warn!("WebSocket connection error");
                }
            });
        }
    }

    /// Starts a WebSocket server and returns the bound address.
    ///
    /// Like [`serve`](Self::serve), but useful for tests (bind to port 0).
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] if the TCP listener fails to bind.
    pub async fn serve_with_addr(
        self: Arc<Self>,
        addr: impl tokio::net::ToSocketAddrs,
    ) -> std::io::Result<SocketAddr> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;

        trace_info!(%local_addr, "A2A WebSocket server listening");

        tokio::spawn(async move {
            loop {
                let Ok((stream, _peer)) = listener.accept().await else {
                    break;
                };
                let dispatcher = Arc::clone(&self);
                tokio::spawn(async move {
                    let _ = dispatcher.handle_connection(stream).await;
                });
            }
        });

        Ok(local_addr)
    }

    /// Handles a single WebSocket connection.
    async fn handle_connection(&self, stream: TcpStream) -> Result<(), WsError> {
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .map_err(WsError::Handshake)?;

        let (writer, mut reader) = ws_stream.split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer));

        while let Some(msg) = reader.next().await {
            match msg {
                Ok(WsMessage::Text(text)) => {
                    let writer = Arc::clone(&writer);
                    let handler = Arc::clone(&self.handler);
                    tokio::spawn(async move {
                        process_ws_message(&handler, &text, writer).await;
                    });
                }
                Ok(WsMessage::Ping(data)) => {
                    let mut w = writer.lock().await;
                    let _ = w.send(WsMessage::Pong(data)).await;
                    drop(w);
                }
                Ok(WsMessage::Close(_)) | Err(_) => break,
                Ok(_) => {} // Binary frames, pongs — ignore
            }
        }

        Ok(())
    }
}

/// Internal WebSocket error type.
#[derive(Debug)]
enum WsError {
    Handshake(tokio_tungstenite::tungstenite::Error),
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Handshake(e) => write!(f, "WebSocket handshake failed: {e}"),
        }
    }
}

type WsSink = Arc<tokio::sync::Mutex<SplitSink<WebSocketStream<TcpStream>, WsMessage>>>;

/// Processes a single JSON-RPC message received over WebSocket.
#[allow(clippy::too_many_lines)]
async fn process_ws_message(handler: &RequestHandler, text: &str, writer: WsSink) {
    let rpc_req: JsonRpcRequest = match serde_json::from_str(text) {
        Ok(req) => req,
        Err(e) => {
            let err_resp = JsonRpcErrorResponse::new(
                None,
                JsonRpcError::new(-32700, format!("parse error: {e}")),
            );
            send_json(&writer, &err_resp).await;
            return;
        }
    };

    let id = rpc_req.id.clone();
    let headers = HashMap::new();

    match rpc_req.method.as_str() {
        "SendMessage" => {
            dispatch_send_message(handler, &rpc_req, false, &headers, id, &writer).await;
        }
        "SendStreamingMessage" => {
            dispatch_send_message(handler, &rpc_req, true, &headers, id, &writer).await;
        }
        "GetTask" => {
            dispatch_simple(handler, &rpc_req, id, &headers, &writer, |h, p, hdr| {
                Box::pin(async move {
                    let params: a2a_protocol_types::params::TaskQueryParams =
                        serde_json::from_value(p).map_err(|e| {
                            a2a_protocol_types::error::A2aError::invalid_params(e.to_string())
                        })?;
                    h.on_get_task(params, Some(hdr))
                        .await
                        .map(|r| serde_json::to_value(&r).unwrap_or_default())
                        .map_err(|e| e.to_a2a_error())
                })
            })
            .await;
        }
        "ListTasks" => {
            dispatch_simple(handler, &rpc_req, id, &headers, &writer, |h, p, hdr| {
                Box::pin(async move {
                    let params: a2a_protocol_types::params::ListTasksParams =
                        serde_json::from_value(p).map_err(|e| {
                            a2a_protocol_types::error::A2aError::invalid_params(e.to_string())
                        })?;
                    h.on_list_tasks(params, Some(hdr))
                        .await
                        .map(|r| serde_json::to_value(&r).unwrap_or_default())
                        .map_err(|e| e.to_a2a_error())
                })
            })
            .await;
        }
        "CancelTask" => {
            dispatch_simple(handler, &rpc_req, id, &headers, &writer, |h, p, hdr| {
                Box::pin(async move {
                    let params: a2a_protocol_types::params::CancelTaskParams =
                        serde_json::from_value(p).map_err(|e| {
                            a2a_protocol_types::error::A2aError::invalid_params(e.to_string())
                        })?;
                    h.on_cancel_task(params, Some(hdr))
                        .await
                        .map(|r| serde_json::to_value(&r).unwrap_or_default())
                        .map_err(|e| e.to_a2a_error())
                })
            })
            .await;
        }
        "SubscribeToTask" => {
            let params = match parse_params::<a2a_protocol_types::params::TaskIdParams>(
                rpc_req.params.as_ref(),
            ) {
                Ok(p) => p,
                Err(e) => {
                    send_error(&writer, id, &e).await;
                    return;
                }
            };
            match handler.on_resubscribe(params, Some(&headers)).await {
                Ok(reader) => {
                    stream_events(&writer, reader, id).await;
                }
                Err(e) => {
                    send_error(&writer, id, &e).await;
                }
            }
        }
        other => {
            let err = ServerError::MethodNotFound(other.to_owned());
            send_error(&writer, id, &err).await;
        }
    }
}

/// Dispatches a `SendMessage` or `SendStreamingMessage`.
async fn dispatch_send_message(
    handler: &RequestHandler,
    rpc_req: &JsonRpcRequest,
    streaming: bool,
    headers: &HashMap<String, String>,
    id: JsonRpcId,
    writer: &WsSink,
) {
    let params = match parse_params::<a2a_protocol_types::params::MessageSendParams>(
        rpc_req.params.as_ref(),
    ) {
        Ok(p) => p,
        Err(e) => {
            send_error(writer, id, &e).await;
            return;
        }
    };

    match handler
        .on_send_message(params, streaming, Some(headers))
        .await
    {
        Ok(SendMessageResult::Response(resp)) => {
            let result = serde_json::to_value(&resp).unwrap_or(serde_json::Value::Null);
            let success = JsonRpcSuccessResponse {
                jsonrpc: JsonRpcVersion,
                id,
                result,
            };
            send_json(writer, &success).await;
        }
        Ok(SendMessageResult::Stream(reader)) => {
            stream_events(writer, reader, id).await;
        }
        Err(e) => {
            send_error(writer, id, &e).await;
        }
    }
}

/// Streams events from an event queue reader over WebSocket as individual frames.
async fn stream_events(
    writer: &WsSink,
    mut reader: crate::streaming::InMemoryQueueReader,
    id: JsonRpcId,
) {
    while let Some(event) = reader.read().await {
        match event {
            Ok(stream_resp) => {
                let json = serde_json::to_string(&stream_resp).unwrap_or_default();
                let mut w = writer.lock().await;
                if w.send(WsMessage::Text(json)).await.is_err() {
                    return; // Client disconnected
                }
                drop(w);
            }
            Err(e) => {
                let err_resp =
                    JsonRpcErrorResponse::new(id.clone(), JsonRpcError::new(-32000, e.to_string()));
                send_json(writer, &err_resp).await;
                return;
            }
        }
    }

    // Stream complete — send final success response.
    let success = JsonRpcSuccessResponse {
        jsonrpc: JsonRpcVersion,
        id,
        result: serde_json::json!({"status": "stream_complete"}),
    };
    send_json(writer, &success).await;
}

/// Generic dispatcher for simple (non-streaming) methods.
async fn dispatch_simple<'a, F>(
    handler: &'a RequestHandler,
    rpc_req: &JsonRpcRequest,
    id: JsonRpcId,
    headers: &'a HashMap<String, String>,
    writer: &WsSink,
    f: F,
) where
    F: FnOnce(
        &'a RequestHandler,
        serde_json::Value,
        &'a HashMap<String, String>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<serde_json::Value, a2a_protocol_types::error::A2aError>,
                > + Send
                + 'a,
        >,
    >,
{
    let params = rpc_req.params.clone().unwrap_or(serde_json::Value::Null);
    match f(handler, params, headers).await {
        Ok(result) => {
            let success = JsonRpcSuccessResponse {
                jsonrpc: JsonRpcVersion,
                id,
                result,
            };
            send_json(writer, &success).await;
        }
        Err(e) => {
            let err_resp =
                JsonRpcErrorResponse::new(id, JsonRpcError::new(e.code.as_i32(), e.message));
            send_json(writer, &err_resp).await;
        }
    }
}

/// Sends a JSON-serializable value as a WebSocket text frame.
async fn send_json<T: serde::Serialize + Sync>(writer: &WsSink, value: &T) {
    let json = serde_json::to_string(value).unwrap_or_default();
    let mut w = writer.lock().await;
    let _ = w.send(WsMessage::Text(json)).await;
    drop(w);
}

/// Sends a server error as a JSON-RPC error response.
async fn send_error(writer: &WsSink, id: JsonRpcId, err: &ServerError) {
    let a2a_err = err.to_a2a_error();
    let resp = JsonRpcErrorResponse::new(
        id,
        JsonRpcError::new(a2a_err.code.as_i32(), a2a_err.message),
    );
    send_json(writer, &resp).await;
}

/// Parses params from an optional JSON value.
fn parse_params<T: serde::de::DeserializeOwned>(
    params: Option<&serde_json::Value>,
) -> Result<T, ServerError> {
    let value = params.cloned().unwrap_or(serde_json::Value::Null);
    serde_json::from_value(value)
        .map_err(|e| ServerError::InvalidParams(format!("invalid params: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_params_with_valid_json() {
        let value = Some(serde_json::json!({"id": "task-1"}));
        let result: Result<a2a_protocol_types::params::TaskQueryParams, _> =
            parse_params(value.as_ref());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "task-1");
    }

    #[test]
    fn parse_params_with_none_returns_error() {
        let result: Result<a2a_protocol_types::params::TaskQueryParams, _> = parse_params(None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_params_with_wrong_type_returns_error() {
        let value = Some(serde_json::json!("not an object"));
        let result: Result<a2a_protocol_types::params::TaskQueryParams, _> =
            parse_params(value.as_ref());
        assert!(result.is_err());
    }

    // WsError Display
    #[test]
    fn ws_error_display_contains_message() {
        let err = WsError::Handshake(tokio_tungstenite::tungstenite::Error::ConnectionClosed);
        let s = err.to_string();
        assert!(s.contains("WebSocket handshake failed"));
    }

    // WebSocketDispatcher construction
    #[test]
    fn websocket_dispatcher_new() {
        use crate::executor_helpers::agent_executor;
        use crate::handler::RequestHandlerBuilder;
        use std::sync::Arc;
        struct DummyExec;
        agent_executor!(DummyExec, |_ctx, _queue| async { Ok(()) });
        let handler = Arc::new(RequestHandlerBuilder::new(DummyExec).build().unwrap());
        let _dispatcher = WebSocketDispatcher::new(handler);
    }
}
