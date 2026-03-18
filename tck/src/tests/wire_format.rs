// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Wire format conformance tests.

use super::helpers;

/// Tests that JSON-RPC responses have proper envelope format.
pub async fn test_jsonrpc_envelope_format(url: &str, binding: &str) -> Result<(), String> {
    if binding != "jsonrpc" {
        // This test only applies to JSON-RPC binding
        return Ok(());
    }

    let params = helpers::make_send_params("TCK: wire format test");
    let resp = helpers::jsonrpc_request(url, "message/send", params).await?;

    // Must have "jsonrpc": "2.0"
    let version = resp
        .get("jsonrpc")
        .and_then(|v| v.as_str())
        .ok_or("response missing 'jsonrpc' field")?;
    if version != "2.0" {
        return Err(format!("expected jsonrpc '2.0', got '{version}'"));
    }

    // Must have "id" matching the request
    if resp.get("id").is_none() {
        return Err("response missing 'id' field".to_string());
    }

    // Must have either "result" or "error" (not both)
    let has_result = resp.get("result").is_some();
    let has_error = resp.get("error").is_some();
    if !has_result && !has_error {
        return Err("response must have either 'result' or 'error'".to_string());
    }
    if has_result && has_error {
        return Err("response must not have both 'result' and 'error'".to_string());
    }

    Ok(())
}

/// Tests that task state values use the correct wire format strings.
pub async fn test_task_state_values(url: &str, binding: &str) -> Result<(), String> {
    let params = helpers::make_send_params("TCK: state values test");
    let result = helpers::send_message(url, binding, params).await?;

    let state = result
        .get("status")
        .and_then(|s| s.get("state"))
        .and_then(|s| s.as_str())
        .ok_or("task missing status.state")?;

    let valid_states = [
        "submitted",
        "working",
        "input-required",
        "auth-required",
        "completed",
        "failed",
        "canceled",
        "rejected",
    ];

    if !valid_states.contains(&state) {
        return Err(format!(
            "invalid task state wire format: '{state}'. Expected one of: {}",
            valid_states.join(", ")
        ));
    }

    Ok(())
}
