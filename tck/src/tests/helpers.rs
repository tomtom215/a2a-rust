// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Shared helpers for TCK tests.

use serde_json::Value;

/// Sends a JSON-RPC request and returns the parsed response.
pub async fn jsonrpc_request(url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params
    });

    http_post(url, &body).await
}

/// Sends a REST request (POST for actions, GET for queries).
pub async fn rest_post(url: &str, path: &str, body: &Value) -> Result<Value, String> {
    let full_url = format!("{url}{path}");
    http_post(&full_url, body).await
}

/// Sends a REST GET request.
pub async fn rest_get(url: &str, path: &str) -> Result<(u16, Value), String> {
    let full_url = format!("{url}{path}");
    http_get(&full_url).await
}

/// Low-level HTTP POST that returns parsed JSON.
async fn http_post(url: &str, body: &Value) -> Result<Value, String> {
    let body_bytes = serde_json::to_vec(body).map_err(|e| format!("serialize: {e}"))?;

    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(url)
        .header("content-type", "application/json")
        .body(http_body_util::Full::new(hyper::body::Bytes::from(
            body_bytes,
        )))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();

    serde_json::from_slice(&body).map_err(|e| {
        format!(
            "parse response: {e} (body: {})",
            String::from_utf8_lossy(&body)
        )
    })
}

/// Low-level HTTP GET that returns status code and parsed JSON.
async fn http_get(url: &str) -> Result<(u16, Value), String> {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build_http();

    let req = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(url)
        .body(http_body_util::Full::new(hyper::body::Bytes::new()))
        .map_err(|e| format!("build request: {e}"))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status().as_u16();

    let body = http_body_util::BodyExt::collect(resp.into_body())
        .await
        .map_err(|e| format!("read body: {e}"))?
        .to_bytes();

    let json: Value = serde_json::from_slice(&body).map_err(|e| {
        format!(
            "parse response: {e} (body: {})",
            String::from_utf8_lossy(&body)
        )
    })?;

    Ok((status, json))
}

/// Creates a minimal SendMessage params with a text message.
pub fn make_send_params(text: &str) -> Value {
    serde_json::json!({
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": text}],
            "messageId": uuid::Uuid::new_v4().to_string()
        }
    })
}

/// Creates SendMessage params with a specific context ID.
pub fn make_send_params_with_context(text: &str, context_id: &str) -> Value {
    serde_json::json!({
        "message": {
            "role": "user",
            "parts": [{"type": "text", "text": text}],
            "messageId": uuid::Uuid::new_v4().to_string()
        },
        "configuration": {
            "acceptedOutputModes": ["text"]
        },
        "contextId": context_id
    })
}

/// Sends a message via the appropriate binding and returns the response.
pub async fn send_message(url: &str, binding: &str, params: Value) -> Result<Value, String> {
    match binding {
        "jsonrpc" => {
            let resp = jsonrpc_request(url, "message/send", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result")
                .cloned()
                .ok_or_else(|| "missing 'result' in JSON-RPC response".to_string())
        }
        "rest" => rest_post(url, "/message/send", &params).await,
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Gets a task by ID via the appropriate binding.
pub async fn get_task(url: &str, binding: &str, task_id: &str) -> Result<Value, String> {
    match binding {
        "jsonrpc" => {
            let params = serde_json::json!({"id": task_id});
            let resp = jsonrpc_request(url, "tasks/get", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result")
                .cloned()
                .ok_or_else(|| "missing 'result' in JSON-RPC response".to_string())
        }
        "rest" => {
            let (status, body) = rest_get(url, &format!("/tasks/{task_id}")).await?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {body}"));
            }
            Ok(body)
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Gets a task that is expected to not exist — returns Ok if error, Err if found.
pub async fn get_task_expect_not_found(
    url: &str,
    binding: &str,
    task_id: &str,
) -> Result<(), String> {
    match binding {
        "jsonrpc" => {
            let params = serde_json::json!({"id": task_id});
            let resp = jsonrpc_request(url, "tasks/get", params).await?;
            if resp.get("error").is_some() {
                Ok(())
            } else {
                Err("expected error for non-existent task, got result".to_string())
            }
        }
        "rest" => {
            let (status, _) = rest_get(url, &format!("/tasks/{task_id}")).await?;
            if status == 404 || status >= 400 {
                Ok(())
            } else {
                Err(format!("expected 404 for non-existent task, got {status}"))
            }
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}
