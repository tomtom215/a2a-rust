// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task operation conformance tests (GetTask, ListTasks, CancelTask).

use super::helpers;

/// Tests that GetTask returns a previously created task.
pub async fn test_get_task_existing(url: &str, binding: &str) -> Result<(), String> {
    // Create a task first
    let params = helpers::make_send_params("TCK: get_task_existing");
    let created = helpers::send_message(url, binding, params).await?;
    let task_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("created task missing 'id'")?;

    // Now retrieve it
    let task = helpers::get_task(url, binding, task_id).await?;
    let retrieved_id = task
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("retrieved task missing 'id'")?;

    if retrieved_id != task_id {
        return Err(format!(
            "task ID mismatch: created '{task_id}', retrieved '{retrieved_id}'"
        ));
    }

    // Verify structure
    if task.get("status").is_none() {
        return Err("retrieved task missing 'status'".to_string());
    }
    if task.get("contextId").is_none() {
        return Err("retrieved task missing 'contextId'".to_string());
    }

    Ok(())
}

/// Tests that GetTask returns an error for a non-existent task.
pub async fn test_get_task_not_found(url: &str, binding: &str) -> Result<(), String> {
    helpers::get_task_expect_not_found(url, binding, "non-existent-task-id-12345").await
}

/// Tests that ListTasks returns a list structure.
pub async fn test_list_tasks_basic(url: &str, binding: &str) -> Result<(), String> {
    // Create a task to ensure the list is non-empty
    let ctx_id = uuid::Uuid::new_v4().to_string();
    let params = helpers::make_send_params_with_context("TCK: list_tasks", &ctx_id);
    helpers::send_message(url, binding, params).await?;

    // List tasks
    let result = match binding {
        "jsonrpc" => {
            let params = serde_json::json!({});
            let resp = helpers::jsonrpc_request(url, "tasks/list", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result")
                .cloned()
                .ok_or("missing 'result'")?
        }
        "rest" => {
            let (status, body) = helpers::rest_get(url, "/tasks").await?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {body}"));
            }
            body
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    // Response should have a tasks array
    let tasks = result
        .get("tasks")
        .ok_or("list response missing 'tasks' field")?;
    if !tasks.is_array() {
        return Err("'tasks' must be an array".to_string());
    }

    Ok(())
}

/// Tests CancelTask behavior.
pub async fn test_cancel_task(url: &str, binding: &str) -> Result<(), String> {
    // Create a task first
    let params = helpers::make_send_params("TCK: cancel_task");
    let created = helpers::send_message(url, binding, params).await?;
    let task_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("created task missing 'id'")?;

    // Attempt to cancel it
    let result = match binding {
        "jsonrpc" => {
            let params = serde_json::json!({"id": task_id});
            let resp = helpers::jsonrpc_request(url, "tasks/cancel", params).await?;
            // Cancel may return an error if the task is already completed or not cancelable
            // Both are valid conformance behaviors
            resp
        }
        "rest" => {
            let body = serde_json::json!({"id": task_id});
            helpers::rest_post(url, &format!("/tasks/{task_id}/cancel"), &body).await?
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    // The response should be valid JSON (either a task or an error)
    if result.is_null() {
        return Err("cancel returned null".to_string());
    }

    Ok(())
}
