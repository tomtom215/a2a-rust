// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Wire format conformance tests.

use super::helpers;

/// Tests that JSON-RPC responses have proper envelope format.
///
/// Applies to every binding that carries the envelope — §9 over HTTP and §12
/// over a socket alike. The runner's `ENVELOPE_ONLY` scope excludes §11 and
/// reports it as `N/A`; this function no longer self-excludes, because the
/// `return Ok(())` that used to do it made the check report a pass for two
/// bindings it never ran against.
pub async fn test_jsonrpc_envelope_format(url: &str, binding: &str) -> Result<(), String> {
    let params = helpers::make_send_params("TCK: wire format test");
    let resp = helpers::rpc(url, binding, "SendMessage", params).await?;

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

/// Tests that task state values use the v1.0 SCREAMING_SNAKE_CASE wire format.
pub async fn test_task_state_values(url: &str, binding: &str) -> Result<(), String> {
    let params = helpers::make_send_params("TCK: state values test");
    let result = helpers::send_message(url, binding, params).await?;
    let task = helpers::extract_task(&result)?;

    let state = task
        .get("status")
        .and_then(|s| s.get("state"))
        .and_then(|s| s.as_str())
        .ok_or("task missing status.state")?;

    // v1.0: ProtoJSON SCREAMING_SNAKE_CASE with TASK_STATE_ prefix
    let valid_states = [
        "TASK_STATE_UNSPECIFIED",
        "TASK_STATE_SUBMITTED",
        "TASK_STATE_WORKING",
        "TASK_STATE_INPUT_REQUIRED",
        "TASK_STATE_AUTH_REQUIRED",
        "TASK_STATE_COMPLETED",
        "TASK_STATE_FAILED",
        "TASK_STATE_CANCELED",
        "TASK_STATE_REJECTED",
    ];

    if !valid_states.contains(&state) {
        return Err(format!(
            "invalid task state wire format: '{state}'. Expected TASK_STATE_* format: {}",
            valid_states.join(", ")
        ));
    }

    Ok(())
}

/// Tests that a §11 REST server accepts the media type `application/a2a+json`.
///
/// §11 says it **SHOULD** be used for requests and responses, and §14.1.1
/// registers it with the interoperability note *"This media type is intended
/// for the HTTP+JSON/REST binding."* A REST server that rejects it refuses a
/// media type the specification tells clients to send.
///
/// **Only §11.** The registration is §14.1.1, not §14.2, and the claim this
/// comment used to make — that "production clients, including
/// `a2a-protocol-client`, send it as the request Content-Type" — was not
/// true of either binding: `transport/jsonrpc.rs` and `transport/rest/`
/// both send `JSON_CONTENT_TYPE`. The check ran against JSON-RPC on the
/// strength of that claim and failed two conformant reference SDKs, which
/// the ITK then carried as upstream divergences. See `REST_MEDIA_TYPE_ONLY`
/// in `runner.rs` for the scope and the citations.
pub async fn test_a2a_media_type_accepted(url: &str, binding: &str) -> Result<(), String> {
    let params = helpers::make_send_params("TCK: a2a media type");
    let result = helpers::send_message_a2a_media_type(url, binding, params).await?;

    if result.is_null() {
        return Err("SendMessage with application/a2a+json returned null".to_string());
    }
    helpers::extract_task(&result)?;
    Ok(())
}
