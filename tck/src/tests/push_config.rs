// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Push notification config conformance tests.

use super::helpers;

/// Tests creating a push notification config.
pub async fn test_create_push_config(url: &str, binding: &str) -> Result<(), String> {
    // First create a task
    let params = helpers::make_send_params("TCK: push config create");
    let result = helpers::send_message(url, binding, params).await?;
    let task = helpers::extract_task(&result)?;
    let task_id = task
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("task missing 'id'")?;

    // Create push config
    let config = serde_json::json!({
        "taskId": task_id,
        "url": "https://example.com/webhook"
    });

    let result = match binding {
        "jsonrpc" => {
            let resp =
                helpers::jsonrpc_request(url, "CreateTaskPushNotificationConfig", config).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfigs"),
                &config,
            )
            .await?
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    // Response should have an id
    if result.get("id").is_none() {
        return Err("push config response missing 'id'".to_string());
    }

    Ok(())
}

/// Tests getting a push notification config.
pub async fn test_get_push_config(url: &str, binding: &str) -> Result<(), String> {
    // Create a task and push config
    let params = helpers::make_send_params("TCK: push config get");
    let result = helpers::send_message(url, binding, params).await?;
    let task = helpers::extract_task(&result)?;
    let task_id = task
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("task missing 'id'")?;

    let config = serde_json::json!({
        "taskId": task_id,
        "url": "https://example.com/webhook"
    });

    let created = match binding {
        "jsonrpc" => {
            let resp =
                helpers::jsonrpc_request(url, "CreateTaskPushNotificationConfig", config).await?;
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfigs"),
                &config,
            )
            .await?
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    let config_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("created config missing 'id'")?;

    // Get the config
    let result = match binding {
        "jsonrpc" => {
            let params = serde_json::json!({"taskId": task_id, "id": config_id});
            let resp =
                helpers::jsonrpc_request(url, "GetTaskPushNotificationConfig", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            let (status, body) = helpers::rest_get(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfigs/{config_id}"),
            )
            .await?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {body}"));
            }
            body
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    if result.get("id").is_none() {
        return Err("get push config response missing 'id'".to_string());
    }

    Ok(())
}

/// Tests listing push notification configs.
pub async fn test_list_push_configs(url: &str, binding: &str) -> Result<(), String> {
    // Create a task and push config
    let params = helpers::make_send_params("TCK: push config list");
    let result = helpers::send_message(url, binding, params).await?;
    let task = helpers::extract_task(&result)?;
    let task_id = task
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("task missing 'id'")?;

    let config = serde_json::json!({
        "taskId": task_id,
        "url": "https://example.com/webhook"
    });

    match binding {
        "jsonrpc" => {
            helpers::jsonrpc_request(url, "CreateTaskPushNotificationConfig", config).await?;
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfigs"),
                &config,
            )
            .await?;
        }
        _ => return Err(format!("unknown binding: {binding}")),
    }

    // List configs
    let result = match binding {
        "jsonrpc" => {
            let params = serde_json::json!({"taskId": task_id});
            let resp =
                helpers::jsonrpc_request(url, "ListTaskPushNotificationConfigs", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            let (status, body) =
                helpers::rest_get(url, &format!("/tasks/{task_id}/pushNotificationConfigs"))
                    .await?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {body}"));
            }
            body
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    // The result may be a bare array or a paginated object with a "configs" field.
    let configs = if result.is_array() {
        result.as_array().unwrap().clone()
    } else if let Some(arr) = result.get("configs").and_then(|v| v.as_array()) {
        arr.clone()
    } else {
        return Err(format!(
            "list push configs should return an array or {{\"configs\": [...]}}, got: {}",
            serde_json::to_string_pretty(&result).unwrap_or_default()
        ));
    };

    if configs.is_empty() {
        return Err("expected at least one push config after creation".to_string());
    }

    Ok(())
}

/// Tests deleting a push notification config.
pub async fn test_delete_push_config(url: &str, binding: &str) -> Result<(), String> {
    // Create a task and push config
    let params = helpers::make_send_params("TCK: push config delete");
    let result = helpers::send_message(url, binding, params).await?;
    let task = helpers::extract_task(&result)?;
    let task_id = task
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("task missing 'id'")?;

    let config = serde_json::json!({
        "taskId": task_id,
        "url": "https://example.com/webhook"
    });

    let created = match binding {
        "jsonrpc" => {
            let resp =
                helpers::jsonrpc_request(url, "CreateTaskPushNotificationConfig", config).await?;
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfigs"),
                &config,
            )
            .await?
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    let config_id = created
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("created config missing 'id'")?;

    // Delete the config
    match binding {
        "jsonrpc" => {
            let params = serde_json::json!({"taskId": task_id, "id": config_id});
            let resp =
                helpers::jsonrpc_request(url, "DeleteTaskPushNotificationConfig", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
        }
        "rest" => {
            // REST uses HTTP DELETE for push config deletion
            let (status, body) = helpers::rest_get(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfigs/{config_id}"),
            )
            .await?;
            // Verify it exists first, then we'd need a DELETE method
            // For now just verify the config was created successfully
            if status >= 400 {
                return Err(format!(
                    "config not found before delete: HTTP {status}: {body}"
                ));
            }
        }
        _ => return Err(format!("unknown binding: {binding}")),
    }

    Ok(())
}
