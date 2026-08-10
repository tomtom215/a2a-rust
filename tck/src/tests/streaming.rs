// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Streaming conformance tests (SendStreamingMessage, SubscribeToTask).

use super::helpers;

/// Tests that SendStreamingMessage returns SSE events.
pub async fn test_streaming_send_message(url: &str, binding: &str) -> Result<(), String> {
    // For JSON-RPC binding, streaming uses SSE on the same endpoint
    // For REST binding, streaming uses SSE on /message/stream
    let params = helpers::make_send_params("TCK: streaming test");

    // The WebSocket binding streams by pushing further frames down the same
    // connection rather than by holding an SSE body open, so it cannot share
    // the HTTP path below.
    if binding == "websocket" {
        return ws_streaming(url, params).await;
    }

    let stream_url = match binding {
        "jsonrpc" => url.to_string(),
        "rest" => format!("{url}/message:stream"),
        _ => return Err(format!("unknown binding: {binding}")),
    };

    let body = match binding {
        "jsonrpc" => serde_json::json!({
            "jsonrpc": "2.0",
            "id": uuid::Uuid::new_v4().to_string(),
            "method": "SendStreamingMessage",
            "params": params
        }),
        "rest" => params,
        _ => unreachable!(),
    };

    let body_bytes = serde_json::to_vec(&body).map_err(|e| format!("serialize: {e}"))?;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(&stream_url)
        .header("content-type", "application/json")
        .header("a2a-version", "1.0")
        .header("accept", "text/event-stream")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body_bytes,
        )))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status().as_u16();

    // The server should return 200 with text/event-stream content type,
    // OR it might return a regular JSON response if it doesn't support streaming.
    // Both are valid — the spec says streaming is optional per agent card capabilities.
    if status != 200 {
        return Err(format!("expected 200 for streaming, got {status}"));
    }

    // Read the body to verify it's not empty
    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();

    if body.is_empty() {
        return Err("streaming response body is empty".to_string());
    }

    Ok(())
}

/// `SendStreamingMessage` over the WebSocket binding.
///
/// §12 defines a custom *binding*, not a custom protocol: the request is the
/// same JSON-RPC envelope the `jsonrpc` binding sends. What differs is the
/// response shape — instead of one SSE body held open, the server pushes a
/// sequence of JSON-RPC frames down the same socket, each carrying the
/// request's `id` so the client can route it.
///
/// Conformance here is that the stream *arrives and terminates*: at least one
/// frame correlated to the request, and a terminal task state before the
/// deadline. A binding that answers once and stops, or that never reaches a
/// terminal state, fails — both are real ways a streaming transport breaks
/// while still returning something.
///
/// What it deliberately does **not** assert is the frame sequence. §12
/// registers WebSocket as a *custom* binding without prescribing one, so an
/// implementation is free to end the stream by closing the socket, by sending
/// a sentinel frame, or by sending nothing further. Demanding this SDK's own
/// `{"result":{"status":"stream_complete"}}` terminator would test our
/// invention as though it were the specification, and would fail a
/// conforming peer for no reason.
async fn ws_streaming(url: &str, params: serde_json::Value) -> Result<(), String> {
    use futures_util::{SinkExt as _, StreamExt as _};
    use tokio_tungstenite::tungstenite::Message;

    let id = uuid::Uuid::new_v4().to_string();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "SendStreamingMessage",
        "params": params
    });

    let ws_url = helpers::to_ws_url_public(url);
    let mut req = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
        ws_url.as_str(),
    )
    .map_err(|e| format!("websocket: bad url {ws_url}: {e}"))?;
    req.headers_mut().insert(
        "a2a-version",
        "1.0".parse().map_err(|e| format!("header: {e}"))?,
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| format!("websocket connect to {ws_url} failed: {e}"))?;
    ws.send(Message::Text(body.to_string().into()))
        .await
        .map_err(|e| format!("websocket send failed: {e}"))?;

    let mut frames = 0usize;
    let mut saw_terminal = false;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(20);

    while tokio::time::Instant::now() < deadline {
        let next = tokio::time::timeout_at(deadline, ws.next()).await;
        let Ok(Some(Ok(msg))) = next else { break };
        let Message::Text(text) = msg else { continue };

        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("streaming frame is not JSON: {e}: {text}"))?;
        if let Some(error) = v.get("error") {
            return Err(format!("streaming returned an error frame: {error}"));
        }
        if v.get("id").and_then(serde_json::Value::as_str) != Some(id.as_str()) {
            return Err(format!(
                "streaming frame does not echo the request id {id}: {text}"
            ));
        }
        frames += 1;

        if frame_state(&v).is_some_and(is_terminal_state) {
            saw_terminal = true;
            break;
        }
    }

    if frames == 0 {
        return Err("websocket streaming produced no frames".to_string());
    }
    if !saw_terminal {
        return Err(format!(
            "websocket streaming produced {frames} frame(s) but never reached a terminal state"
        ));
    }
    Ok(())
}

/// Reads the task state out of one streaming frame's `result`, whichever
/// `StreamResponse` variant it carries.
///
/// `StreamResponse` is externally tagged, so the state sits at a different
/// depth per variant: `task` for a full snapshot, `statusUpdate` for a
/// `TaskStatusUpdateEvent`. The bare `status` form is the untagged v0.3 shape,
/// accepted because a peer may still emit it. `artifactUpdate` frames carry no
/// state at all and yield `None` — the caller keeps reading rather than
/// treating a stateless frame as non-terminal evidence.
///
/// Getting this wrong does not fail loudly: every frame simply reads as
/// stateless, the loop runs to the deadline, and the check reports "never
/// reached a terminal state" against a server that reached one on time.
fn frame_state(frame: &serde_json::Value) -> Option<&str> {
    frame
        .pointer("/result/statusUpdate/status/state")
        .or_else(|| frame.pointer("/result/task/status/state"))
        .or_else(|| frame.pointer("/result/status/state"))
        .and_then(serde_json::Value::as_str)
}

/// The four `TaskState` values §3.2 calls terminal, in v1.0 `ProtoJSON`
/// spelling. Note the single `L` in `CANCELED`: `TASK_STATE_CANCELLED` is not
/// a wire value, and matching on it would wait out the deadline on every
/// cancelled stream.
fn is_terminal_state(state: &str) -> bool {
    matches!(
        state,
        "TASK_STATE_COMPLETED"
            | "TASK_STATE_FAILED"
            | "TASK_STATE_CANCELED"
            | "TASK_STATE_REJECTED"
    )
}

#[cfg(test)]
mod tests {
    use super::{frame_state, is_terminal_state};

    /// The shapes the reference implementation actually puts on the wire,
    /// captured from a live `SendStreamingMessage` over §12. An earlier
    /// version of `frame_state` looked only at `/result/status/state` and
    /// `/result/task/status/state`, so it read every `statusUpdate` frame as
    /// stateless and reported a healthy stream as never terminating.
    #[test]
    fn reads_state_from_every_stream_response_variant() {
        let task_snapshot = serde_json::json!({
            "result": {"task": {"status": {"state": "TASK_STATE_SUBMITTED"}}}
        });
        assert_eq!(frame_state(&task_snapshot), Some("TASK_STATE_SUBMITTED"));

        let status_update = serde_json::json!({
            "result": {"statusUpdate": {"status": {"state": "TASK_STATE_COMPLETED"}}}
        });
        assert_eq!(frame_state(&status_update), Some("TASK_STATE_COMPLETED"));

        let untagged = serde_json::json!({
            "result": {"status": {"state": "TASK_STATE_WORKING"}}
        });
        assert_eq!(frame_state(&untagged), Some("TASK_STATE_WORKING"));
    }

    #[test]
    fn stateless_frames_yield_none() {
        let artifact = serde_json::json!({
            "result": {"artifactUpdate": {"artifact": {"artifactId": "a"}}}
        });
        assert_eq!(frame_state(&artifact), None);

        // This SDK's end-of-stream sentinel: `status` is a string here, not
        // a TaskStatus object, so it must not be mistaken for a state.
        let sentinel = serde_json::json!({"result": {"status": "stream_complete"}});
        assert_eq!(frame_state(&sentinel), None);
    }

    #[test]
    fn terminal_states_are_the_four_spec_names_in_v1_spelling() {
        for terminal in [
            "TASK_STATE_COMPLETED",
            "TASK_STATE_FAILED",
            "TASK_STATE_CANCELED",
            "TASK_STATE_REJECTED",
        ] {
            assert!(is_terminal_state(terminal), "{terminal} is terminal");
        }
        for non_terminal in [
            "TASK_STATE_SUBMITTED",
            "TASK_STATE_WORKING",
            "TASK_STATE_INPUT_REQUIRED",
            "TASK_STATE_AUTH_REQUIRED",
            "TASK_STATE_UNSPECIFIED",
        ] {
            assert!(!is_terminal_state(non_terminal), "{non_terminal} is not");
        }
        // Double-L is not a v1.0 wire value; accepting it would let a
        // non-conforming server pass on a misspelling.
        assert!(!is_terminal_state("TASK_STATE_CANCELLED"));
    }
}
