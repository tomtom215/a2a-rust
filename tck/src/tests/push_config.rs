// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Push notification config conformance tests.

use super::helpers;

/// Tests creating a push notification config.
pub async fn test_create_push_config(url: &str, binding: &str) -> Result<(), String> {
    // First create a task
    let params = helpers::make_send_params("TCK: push config create");
    let task = helpers::send_message(url, binding, params).await?;
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
                helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/set", config).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfig"),
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
    let task = helpers::send_message(url, binding, params).await?;
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
                helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/set", config).await?;
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfig"),
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
                helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/get", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            let (status, body) = helpers::rest_get(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfig/{config_id}"),
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
    let task = helpers::send_message(url, binding, params).await?;
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
            helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/set", config).await?;
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfig"),
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
                helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/list", params).await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            let (status, body) =
                helpers::rest_get(url, &format!("/tasks/{task_id}/pushNotificationConfig")).await?;
            if status >= 400 {
                return Err(format!("HTTP {status}: {body}"));
            }
            body
        }
        _ => return Err(format!("unknown binding: {binding}")),
    };

    // Should be an array
    if !result.is_array() {
        return Err("list push configs should return an array".to_string());
    }

    let configs = result.as_array().unwrap();
    if configs.is_empty() {
        return Err("expected at least one push config after creation".to_string());
    }

    Ok(())
}

/// Tests deleting a push notification config.
pub async fn test_delete_push_config(url: &str, binding: &str) -> Result<(), String> {
    // Create a task and push config
    let params = helpers::make_send_params("TCK: push config delete");
    let task = helpers::send_message(url, binding, params).await?;
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
                helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/set", config).await?;
            resp.get("result").cloned().ok_or("missing 'result'")?
        }
        "rest" => {
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfig"),
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
            let resp = helpers::jsonrpc_request(url, "tasks/pushNotificationConfig/delete", params)
                .await?;
            if let Some(error) = resp.get("error") {
                return Err(format!("JSON-RPC error: {error}"));
            }
        }
        "rest" => {
            // DELETE is typically used for deletion but we use POST with a body
            let body = serde_json::json!({"taskId": task_id, "id": config_id});
            helpers::rest_post(
                url,
                &format!("/tasks/{task_id}/pushNotificationConfig/{config_id}/delete"),
                &body,
            )
            .await?;
        }
        _ => return Err(format!("unknown binding: {binding}")),
    }

    Ok(())
}
