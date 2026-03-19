// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for push notification configuration CRUD operations, including the
//! not-supported and not-found error paths.

use super::*;

#[tokio::test]
async fn push_config_crud_lifecycle() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_push_sender(MockPushSender)
        .build()
        .expect("build handler");

    // Create push config.
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/webhook");
    let created = handler
        .on_set_push_config(config, None)
        .await
        .expect("set push config");
    assert!(
        created.id.is_some(),
        "created push config must have an id assigned"
    );
    let config_id = created.id.clone().unwrap();

    // Get push config.
    let get_params = GetPushConfigParams {
        tenant: None,
        task_id: "task-1".into(),
        id: config_id.clone(),
    };
    let fetched = handler
        .on_get_push_config(get_params, None)
        .await
        .expect("get push config");
    assert_eq!(
        fetched.url, "https://example.com/webhook",
        "fetched push config URL must match"
    );

    // List push configs.
    let configs = handler
        .on_list_push_configs("task-1", None, None)
        .await
        .expect("list push configs");
    assert_eq!(
        configs.len(),
        1,
        "expected exactly 1 push config, got {}",
        configs.len()
    );

    // Delete push config.
    let delete_params = DeletePushConfigParams {
        tenant: None,
        task_id: "task-1".into(),
        id: config_id,
    };
    handler
        .on_delete_push_config(delete_params, None)
        .await
        .expect("delete push config");

    // Verify deleted.
    let configs = handler
        .on_list_push_configs("task-1", None, None)
        .await
        .expect("list push configs after delete");
    assert!(
        configs.is_empty(),
        "push config list must be empty after deletion, got {} entries",
        configs.len()
    );
}

#[tokio::test]
async fn push_config_not_supported_without_sender() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/webhook");
    let err = handler.on_set_push_config(config, None).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::PushNotSupported),
        "expected PushNotSupported, got {err:?}"
    );
}

#[tokio::test]
async fn get_push_config_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_push_sender(MockPushSender)
        .build()
        .expect("build handler");

    let params = GetPushConfigParams {
        tenant: None,
        task_id: "task-1".into(),
        id: "nonexistent".into(),
    };
    let err = handler.on_get_push_config(params, None).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(ref msg) if msg.contains("nonexistent") || !msg.is_empty()),
        "expected InvalidParams error, got {err:?}"
    );
}
