// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A streaming call's events, one frame each, and the end of the call with
//! them: the call's span and `rpc.server.call.duration` cover the stream
//! (`rpc_span::stream`). Moved out of `websocket.rs`, which is over the
//! 500-line limit.

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use a2a_protocol_types::jsonrpc::{
    JsonRpcErrorResponse, JsonRpcId, JsonRpcSuccessResponse, JsonRpcVersion,
};

use super::{WsSink, send_json};
use crate::rpc_span::{StreamEnd, StreamOutcome};
use crate::streaming::EventQueueReader;

/// Streams events from an event queue reader over WebSocket as individual
/// frames, then records the call `end` claimed with how the stream ended.
pub(super) async fn stream_events(
    writer: &WsSink,
    reader: crate::streaming::InMemoryQueueReader,
    id: JsonRpcId,
    end: Option<StreamEnd>,
) {
    let outcome = forward(writer, reader, id).await;
    if let Some(end) = end {
        end.end(outcome);
    }
}

async fn forward(
    writer: &WsSink,
    mut reader: crate::streaming::InMemoryQueueReader,
    id: JsonRpcId,
) -> StreamOutcome {
    while let Some(event) = reader.read().await {
        match event.map(|e| e.event) {
            Ok(stream_resp) => {
                // Wrap each event in a JSON-RPC success envelope so the client
                // can route it by `id` and deserialize as `JsonRpcResponse<StreamResponse>`.
                //
                // No log position on the wire here: resumption is the SSE
                // binding's `id:`/`Last-Event-ID` pair, and inventing a
                // WebSocket spelling for it would be a protocol extension
                // this server made up.
                let envelope = JsonRpcSuccessResponse {
                    jsonrpc: JsonRpcVersion,
                    id: id.clone(),
                    result: stream_resp,
                };
                let json = serde_json::to_string(&envelope).unwrap_or_default();
                let mut w = writer.sink.lock().await;
                if w.send(WsMessage::Text(json.into())).await.is_err() {
                    return StreamOutcome::Abandoned; // Client disconnected
                }
                drop(w);
                // This is the write that keeps a long subscription alive under
                // an idle timeout: the consumer may say nothing for hours.
                writer.activity.touch();
            }
            Err(e) => {
                // The error's own code and details, as the SSE binding sends
                // them. It was rewritten to -32000 with `e.to_string()`,
                // which lost the A2A code and the `streamLagged` marker
                // (§12.4: preserve error information).
                let err_resp = JsonRpcErrorResponse::new(
                    id.clone(),
                    crate::streaming::sse::jsonrpc_error_from(&e),
                );
                send_json(writer, &err_resp).await;
                return StreamOutcome::Failed(e);
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
    StreamOutcome::Completed
}
