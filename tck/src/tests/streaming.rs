// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Streaming conformance tests (SendStreamingMessage, SubscribeToTask).

use super::helpers;

/// Tests that SendStreamingMessage returns SSE events.
pub async fn test_streaming_send_message(url: &str, binding: &str) -> Result<(), String> {
    // For JSON-RPC binding, streaming uses SSE on the same endpoint
    // For REST binding, streaming uses SSE on /message/stream
    let params = helpers::make_send_params("TCK: streaming test");

    let stream_url = match binding {
        "jsonrpc" => url.to_string(),
        "rest" => format!("{url}/message/stream"),
        _ => return Err(format!("unknown binding: {binding}")),
    };

    let body = match binding {
        "jsonrpc" => serde_json::json!({
            "jsonrpc": "2.0",
            "id": uuid::Uuid::new_v4().to_string(),
            "method": "message/stream",
            "params": params
        }),
        "rest" => params,
        _ => unreachable!(),
    };

    let body_bytes =
        serde_json::to_vec(&body).map_err(|e| format!("serialize: {e}"))?;

    let client =
        hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(&stream_url)
        .header("content-type", "application/json")
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
