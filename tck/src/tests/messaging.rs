// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! SendMessage conformance tests.

use super::helpers;

/// Tests that a basic SendMessage request succeeds and returns a response.
pub async fn test_send_message_basic(url: &str, binding: &str) -> Result<(), String> {
    let params = helpers::make_send_params("Hello, TCK test");
    let result = helpers::send_message(url, binding, params).await?;

    // The response should be a task or a message-like structure
    if result.is_null() {
        return Err("SendMessage returned null".to_string());
    }

    Ok(())
}

/// Tests that SendMessage returns a task with valid structure.
pub async fn test_send_message_returns_task(url: &str, binding: &str) -> Result<(), String> {
    let params = helpers::make_send_params("TCK: send_message_returns_task");
    let result = helpers::send_message(url, binding, params).await?;

    // v1.0: result is {"task": {...}} — extract the task
    let task = helpers::extract_task(&result)?;

    let id = task.get("id").ok_or("task missing 'id' field")?;
    if !id.is_string() {
        return Err("'id' must be a string".to_string());
    }

    let status = task.get("status").ok_or("task missing 'status' field")?;
    let state = status.get("state").ok_or("status missing 'state' field")?;
    if !state.is_string() {
        return Err("'state' must be a string".to_string());
    }

    // v1.0: State uses TASK_STATE_* ProtoJSON SCREAMING_SNAKE_CASE
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
    let state_str = state.as_str().unwrap();
    if !valid_states.contains(&state_str) {
        return Err(format!("invalid task state: '{state_str}'"));
    }

    Ok(())
}

/// Tests that SendMessage respects contextId for multi-turn conversations.
pub async fn test_send_message_context_id(url: &str, binding: &str) -> Result<(), String> {
    let ctx_id = uuid::Uuid::new_v4().to_string();

    // First message with context
    let params = helpers::make_send_params_with_context("TCK: first turn", &ctx_id);
    let result1 = helpers::send_message(url, binding, params).await?;
    let task1 = helpers::extract_task(&result1)?;

    let context1 = task1
        .get("contextId")
        .ok_or("first task missing 'contextId'")?
        .as_str()
        .ok_or("'contextId' must be a string")?;

    if context1.is_empty() {
        return Err("contextId should not be empty".to_string());
    }

    // Second message with same context
    let params2 = helpers::make_send_params_with_context("TCK: second turn", context1);
    let result2 = helpers::send_message(url, binding, params2).await?;
    let task2 = helpers::extract_task(&result2)?;

    let context2 = task2
        .get("contextId")
        .ok_or("second task missing 'contextId'")?
        .as_str()
        .ok_or("'contextId' must be a string")?;

    // Context IDs should be consistent across turns
    if context2 != context1 {
        return Err(format!(
            "contextId changed between turns: '{context1}' vs '{context2}'"
        ));
    }

    Ok(())
}
