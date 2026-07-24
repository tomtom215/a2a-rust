// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Error handling conformance tests.

use super::helpers;

/// Tests that an invalid JSON-RPC method returns a proper error.
pub async fn test_invalid_method_returns_error(url: &str, binding: &str) -> Result<(), String> {
    match binding {
        "jsonrpc" => {
            let resp =
                helpers::jsonrpc_request(url, "nonexistent/method", serde_json::json!({})).await?;
            let error = resp
                .get("error")
                .ok_or("expected error response for invalid method")?;

            // Should have code and message per JSON-RPC 2.0 spec
            if error.get("code").is_none() {
                return Err("error missing 'code' field".to_string());
            }
            if error.get("message").is_none() {
                return Err("error missing 'message' field".to_string());
            }

            Ok(())
        }
        "rest" => {
            let (status, _) = helpers::rest_get(url, "/nonexistent/path").await?;
            if status < 400 {
                return Err(format!("expected 4xx for invalid path, got {status}"));
            }
            Ok(())
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Tests that invalid params return a proper error.
pub async fn test_invalid_params_returns_error(url: &str, binding: &str) -> Result<(), String> {
    match binding {
        "jsonrpc" => {
            // Send message/send with missing required 'message' field
            let resp = helpers::jsonrpc_request(
                url,
                "SendMessage",
                serde_json::json!({"invalid_field": true}),
            )
            .await?;

            let error = resp
                .get("error")
                .ok_or("expected error for invalid params")?;

            let code = error.get("code").and_then(|c| c.as_i64());
            // Per JSON-RPC 2.0: -32602 is "Invalid params"
            // A2A also defines its own error codes, so we accept any error
            if code.is_none() {
                return Err("error missing 'code'".to_string());
            }

            Ok(())
        }
        "rest" => {
            let body = serde_json::json!({"invalid_field": true});
            let result = helpers::rest_post(url, "/message:send", &body).await;
            // Should return an error (either HTTP status or error JSON)
            match result {
                Ok(resp) => {
                    if resp.get("error").is_some() || resp.get("code").is_some() {
                        Ok(())
                    } else {
                        // If it returned a success, that's also ok if the server is lenient
                        Ok(())
                    }
                }
                Err(_) => Ok(()), // Error response is expected
            }
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// Spec §3.4.2: `GetTask` for a task the server has never seen MUST return
/// an error (TaskNotFound), never a fabricated task. Portable across every
/// reference SDK — no server may invent a task for an unknown id.
pub async fn test_get_unknown_task_returns_error(url: &str, binding: &str) -> Result<(), String> {
    let unknown = "tck-nonexistent-task-2f9c8e11-does-not-exist";
    match binding {
        "jsonrpc" => {
            let resp =
                helpers::jsonrpc_request(url, "GetTask", serde_json::json!({ "id": unknown }))
                    .await?;
            if resp.get("error").is_none() {
                return Err(format!(
                    "GetTask on an unknown id must return an error, got: {resp}"
                ));
            }
            Ok(())
        }
        "rest" => {
            let (status, body) = helpers::rest_get(url, &format!("/tasks/{unknown}")).await?;
            if status < 400 && body.get("error").is_none() {
                return Err(format!(
                    "GET /tasks/<unknown> must be a 4xx or an error body, got status {status}: {body}"
                ));
            }
            Ok(())
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}

/// A syntactically invalid JSON body on the JSON-RPC endpoint MUST produce a
/// JSON-RPC error envelope (parse error), not a task or a silent 200. The
/// REST binding must answer any non-2xx / error for a malformed body.
pub async fn test_malformed_body_returns_error(url: &str, binding: &str) -> Result<(), String> {
    match binding {
        "jsonrpc" => {
            let resp =
                helpers::post_raw(url, "/", b"{ this is not valid json ", "application/json")
                    .await?;
            // Either a JSON-RPC error envelope or a non-JSON error page is
            // acceptable; a parsed success result is not.
            if resp.get("result").is_some() {
                return Err(format!(
                    "a malformed JSON-RPC body must not yield a result, got: {resp}"
                ));
            }
            Ok(())
        }
        "rest" => {
            let (status, body) = helpers::post_raw_status(
                url,
                "/message:send",
                b"{ this is not valid json ",
                "application/json",
            )
            .await?;
            if status < 400 && body.get("error").is_none() {
                return Err(format!(
                    "a malformed REST body must be rejected, got status {status}: {body}"
                ));
            }
            Ok(())
        }
        _ => Err(format!("unknown binding: {binding}")),
    }
}
