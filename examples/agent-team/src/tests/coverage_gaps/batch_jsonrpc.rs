// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests 61-66: Batch JSON-RPC requests.
//!
//! Covers single-element batch, multi-request batch, empty batch, mixed
//! valid/invalid batch, streaming-in-batch rejection, and subscribe-in-batch
//! rejection.

use super::*;

// ── Batch JSON-RPC tests (61-66) ─────────────────────────────────────────────

/// Test 61: Single-element batch — a batch `[{...}]` with one SendMessage.
pub async fn test_batch_single_element(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let body = format!(
        "[{}]",
        jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params())
    );
    match post_raw(&ctx.analyzer_url, &body).await {
        Ok((status, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if arr.len() == 1 && status == 200 => {
                    let has_result = arr[0].get("result").is_some();
                    let has_id = arr[0].get("id") == Some(&serde_json::json!(1));
                    if has_result && has_id {
                        TestResult::pass(
                            "batch-single-element",
                            start.elapsed().as_millis(),
                            "1-element batch returned array[1]",
                        )
                    } else {
                        TestResult::fail(
                            "batch-single-element",
                            start.elapsed().as_millis(),
                            &format!(
                                "unexpected response: has_result={has_result}, has_id={has_id}, body={}",
                                &resp_body[..resp_body.len().min(100)]
                            ),
                        )
                    }
                }
                Ok(arr) => TestResult::fail(
                    "batch-single-element",
                    start.elapsed().as_millis(),
                    &format!("status={status}, array len={}", arr.len()),
                ),
                Err(e) => TestResult::fail(
                    "batch-single-element",
                    start.elapsed().as_millis(),
                    &format!("not a JSON array: {e}"),
                ),
            }
        }
        Err(e) => TestResult::fail("batch-single-element", start.elapsed().as_millis(), &e),
    }
}

/// Test 62: Multi-request batch — SendMessage + GetTask (chained via task ID).
pub async fn test_batch_multi_request(ctx: &TestContext) -> TestResult {
    let start = Instant::now();

    // First, send a single message to get a task ID.
    let single = jsonrpc_request(serde_json::json!(100), "SendMessage", send_message_params());
    let pre = post_raw(&ctx.analyzer_url, &single).await;
    let task_id = match pre {
        Ok((200, body)) => {
            let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            // SendMessageResponse::Task serializes as {"task": {"id": "...", ...}}
            v["result"]["task"]["id"]
                .as_str()
                .or_else(|| v["result"]["id"].as_str())
                .map(|s| s.to_owned())
        }
        _ => None,
    };
    let Some(task_id) = task_id else {
        return TestResult::fail(
            "batch-multi-request",
            start.elapsed().as_millis(),
            "could not create initial task",
        );
    };

    // Now batch: SendMessage + GetTask
    let batch = format!(
        "[{},{}]",
        jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params()),
        jsonrpc_request(
            serde_json::json!(2),
            "GetTask",
            serde_json::json!({ "id": task_id }),
        ),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if arr.len() == 2 => {
                    let r1_ok =
                        arr[0].get("result").is_some() && arr[0]["id"] == serde_json::json!(1);
                    let r2_ok =
                        arr[1].get("result").is_some() && arr[1]["id"] == serde_json::json!(2);
                    if r1_ok && r2_ok {
                        TestResult::pass(
                            "batch-multi-request",
                            start.elapsed().as_millis(),
                            "SendMessage + GetTask in batch",
                        )
                    } else {
                        TestResult::fail(
                            "batch-multi-request",
                            start.elapsed().as_millis(),
                            &format!("r1={r1_ok}, r2={r2_ok}"),
                        )
                    }
                }
                Ok(arr) => TestResult::fail(
                    "batch-multi-request",
                    start.elapsed().as_millis(),
                    &format!("expected 2 responses, got {}", arr.len()),
                ),
                Err(e) => TestResult::fail(
                    "batch-multi-request",
                    start.elapsed().as_millis(),
                    &format!("parse error: {e}"),
                ),
            }
        }
        Ok((status, body)) => TestResult::fail(
            "batch-multi-request",
            start.elapsed().as_millis(),
            &format!("status={status}, body={}", &body[..body.len().min(80)]),
        ),
        Err(e) => TestResult::fail("batch-multi-request", start.elapsed().as_millis(), &e),
    }
}

/// Test 63: Empty batch `[]` returns a JSON-RPC parse error.
pub async fn test_batch_empty(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    match post_raw(&ctx.analyzer_url, "[]").await {
        Ok((status, resp_body)) => {
            let v: serde_json::Value = serde_json::from_str(&resp_body).unwrap_or_default();
            let is_error = v.get("error").is_some();
            let error_code = v["error"]["code"].as_i64().unwrap_or(0);
            // JSON-RPC parse error = -32700, or invalid request = -32600
            if is_error && (error_code == -32700 || error_code == -32600) {
                TestResult::pass(
                    "batch-empty",
                    start.elapsed().as_millis(),
                    &format!("status={status}, error code={error_code}"),
                )
            } else {
                TestResult::fail(
                    "batch-empty",
                    start.elapsed().as_millis(),
                    &format!(
                        "expected JSON-RPC error code -32700 or -32600, got: code={error_code}, body={}",
                        &resp_body[..resp_body.len().min(100)]
                    ),
                )
            }
        }
        Err(e) => TestResult::fail("batch-empty", start.elapsed().as_millis(), &e),
    }
}

/// Test 64: Batch with mixed valid and invalid requests.
pub async fn test_batch_mixed_valid_invalid(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let batch = format!(
        r#"[{},{{"jsonrpc":"2.0","invalid":true}}]"#,
        jsonrpc_request(serde_json::json!(1), "SendMessage", send_message_params()),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if arr.len() == 2 => {
                    let first_ok = arr[0].get("result").is_some();
                    let second_err = arr[1].get("error").is_some();
                    if first_ok && second_err {
                        TestResult::pass(
                            "batch-mixed",
                            start.elapsed().as_millis(),
                            "valid->result, invalid->error",
                        )
                    } else {
                        TestResult::fail(
                            "batch-mixed",
                            start.elapsed().as_millis(),
                            &format!("first_ok={first_ok}, second_err={second_err}"),
                        )
                    }
                }
                Ok(arr) => TestResult::fail(
                    "batch-mixed",
                    start.elapsed().as_millis(),
                    &format!("expected 2, got {}", arr.len()),
                ),
                Err(e) => TestResult::fail(
                    "batch-mixed",
                    start.elapsed().as_millis(),
                    &format!("parse: {e}"),
                ),
            }
        }
        Ok((status, body)) => TestResult::fail(
            "batch-mixed",
            start.elapsed().as_millis(),
            &format!("status={status}: {}", &body[..body.len().min(80)]),
        ),
        Err(e) => TestResult::fail("batch-mixed", start.elapsed().as_millis(), &e),
    }
}

/// Test 65: Streaming method (SendStreamingMessage) in batch returns error.
pub async fn test_batch_streaming_rejected(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let batch = format!(
        "[{}]",
        jsonrpc_request(
            serde_json::json!(1),
            "SendStreamingMessage",
            send_message_params(),
        ),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if !arr.is_empty() => {
                    let is_error = arr[0].get("error").is_some();
                    if is_error {
                        TestResult::pass(
                            "batch-streaming-rejected",
                            start.elapsed().as_millis(),
                            "streaming in batch -> error",
                        )
                    } else {
                        TestResult::fail(
                            "batch-streaming-rejected",
                            start.elapsed().as_millis(),
                            "expected error for streaming in batch, but got a result",
                        )
                    }
                }
                _ => TestResult::fail(
                    "batch-streaming-rejected",
                    start.elapsed().as_millis(),
                    &format!(
                        "unexpected response: {}",
                        &resp_body[..resp_body.len().min(80)]
                    ),
                ),
            }
        }
        Ok((status, _)) => TestResult::fail(
            "batch-streaming-rejected",
            start.elapsed().as_millis(),
            &format!("status={status}"),
        ),
        Err(e) => TestResult::fail("batch-streaming-rejected", start.elapsed().as_millis(), &e),
    }
}

/// Test 66: SubscribeToTask in batch returns error.
pub async fn test_batch_subscribe_rejected(ctx: &TestContext) -> TestResult {
    let start = Instant::now();
    let batch = format!(
        "[{}]",
        jsonrpc_request(
            serde_json::json!(1),
            "SubscribeToTask",
            serde_json::json!({ "id": "nonexistent-task" }),
        ),
    );
    match post_raw(&ctx.analyzer_url, &batch).await {
        Ok((200, resp_body)) => {
            let parsed: Result<Vec<serde_json::Value>, _> = serde_json::from_str(&resp_body);
            match parsed {
                Ok(arr) if !arr.is_empty() => {
                    let is_error = arr[0].get("error").is_some();
                    if is_error {
                        TestResult::pass(
                            "batch-subscribe-rejected",
                            start.elapsed().as_millis(),
                            "subscribe in batch -> error",
                        )
                    } else {
                        TestResult::fail(
                            "batch-subscribe-rejected",
                            start.elapsed().as_millis(),
                            "expected error for subscribe in batch, but got a result",
                        )
                    }
                }
                _ => TestResult::fail(
                    "batch-subscribe-rejected",
                    start.elapsed().as_millis(),
                    &format!("unexpected: {}", &resp_body[..resp_body.len().min(80)]),
                ),
            }
        }
        Ok((status, _)) => TestResult::fail(
            "batch-subscribe-rejected",
            start.elapsed().as_millis(),
            &format!("status={status}"),
        ),
        Err(e) => TestResult::fail("batch-subscribe-rejected", start.elapsed().as_millis(), &e),
    }
}
