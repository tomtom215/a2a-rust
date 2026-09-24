// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The WebSocket binding of the scripted peer: JSON-RPC 2.0 frames on one
//! socket, answered by the id each request carries.

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;

use super::{Script, hold_open, working_event};

pub(super) async fn serve(script: Script, stream: TcpStream) {
    #[allow(clippy::result_large_err)] // tungstenite's callback signature
    let refuse = |_req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        if script == Script::Unauthorized {
            let mut refusal = ErrorResponse::new(Some("unauthorized".to_owned()));
            *refusal.status_mut() = StatusCode::UNAUTHORIZED;
            return Err(refusal);
        }
        Ok(resp)
    };
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, refuse).await else {
        return;
    };
    while let Some(Ok(frame)) = socket.next().await {
        let Message::Text(text) = frame else { continue };
        let rpc: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let id = rpc.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let streaming = matches!(
            rpc.get("method").and_then(serde_json::Value::as_str),
            Some("SendStreamingMessage" | "SubscribeToTask")
        );
        let after = match script {
            Script::Stall { after } | Script::CutOff { after } | Script::MisFrame { after } => {
                after
            }
            Script::Unauthorized => 0,
        };
        if streaming {
            for _ in 0..after {
                let event =
                    serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": working_event() });
                if socket
                    .send(Message::Text(event.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        match script {
            // Dropped without a close frame: the TCP connection just ends.
            Script::CutOff { .. } => return,
            Script::MisFrame { .. } => {
                // The request's own id, so the frame is routed to the caller
                // waiting for it, and a result that is not an A2A response.
                let bad = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": { "statusUpdate": 42 } });
                let _ = socket.send(Message::Text(bad.to_string().into())).await;
            }
            _ => {}
        }
        hold_open().await;
    }
}
