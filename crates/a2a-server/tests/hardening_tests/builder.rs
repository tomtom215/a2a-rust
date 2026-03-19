// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for `RequestHandlerBuilder`: default configuration, custom stores,
//! push sender enablement, and push rejection without a sender.

use super::*;

#[tokio::test]
async fn builder_build_with_all_defaults_succeeds() {
    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .build()
        .expect("build with defaults should succeed");

    // Verify it's functional by sending a message.
    let result = handler
        .on_send_message(make_send_params("test"), false, None)
        .await
        .expect("send message");
    assert!(matches!(result, SendMessageResult::Response(_)));
}

#[tokio::test]
async fn builder_with_task_store_uses_custom_store() {
    let custom_store = InMemoryTaskStore::with_config(TaskStoreConfig {
        max_capacity: Some(5),
        task_ttl: None,
        ..Default::default()
    });

    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .with_task_store(custom_store)
        .build()
        .expect("build with custom store");

    // The handler should work normally.
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await
        .expect("send message");
    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn builder_with_push_sender_enables_push() {
    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .with_push_sender(MockPushSender)
        .build()
        .expect("build with push sender");

    // Push config operations should succeed (not return PushNotSupported).
    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let saved = handler
        .on_set_push_config(config, None)
        .await
        .expect("set push config should succeed when push is enabled");
    assert!(saved.id.is_some());
}

#[tokio::test]
async fn builder_without_push_sender_rejects_push_config() {
    let handler = RequestHandlerBuilder::new(QuickExecutor)
        .build()
        .expect("build without push sender");

    let config = TaskPushNotificationConfig::new("task-1", "https://example.com/hook");
    let err = handler.on_set_push_config(config, None).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::PushNotSupported),
        "should reject push config when no push sender is configured"
    );
}
