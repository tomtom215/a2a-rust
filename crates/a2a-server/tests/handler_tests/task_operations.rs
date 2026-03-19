// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for task retrieval (`get_task`), listing (`list_tasks`), and
//! cancellation (`cancel_task`) operations.

use super::*;

#[tokio::test]
async fn get_task_returns_stored_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // First send a message to create a task.
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await
        .expect("send message");

    let task_id = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.clone(),
        _ => panic!("expected task, got unexpected variant"),
    };

    // Now get the task.
    let params = TaskQueryParams {
        tenant: None,
        id: task_id.0.clone(),
        history_length: None,
    };
    let task = handler.on_get_task(params, None).await.expect("get task");
    assert_eq!(task.id, task_id, "retrieved task id must match");
    assert_eq!(
        task.status.state,
        TaskState::Completed,
        "retrieved task must be Completed"
    );
}

#[tokio::test]
async fn get_task_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let params = TaskQueryParams {
        tenant: None,
        id: "nonexistent".into(),
        history_length: None,
    };
    let err = handler.on_get_task(params, None).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::TaskNotFound(ref id) if id.0.as_str() == "nonexistent"),
        "expected TaskNotFound for 'nonexistent', got {err:?}"
    );
}

#[tokio::test]
async fn list_tasks_returns_created_tasks() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create two tasks.
    handler
        .on_send_message(make_send_params("one"), false, None)
        .await
        .expect("send first");
    handler
        .on_send_message(make_send_params("two"), false, None)
        .await
        .expect("send second");

    let params = ListTasksParams {
        tenant: None,
        context_id: None,
        status: None,
        page_size: None,
        page_token: None,
        status_timestamp_after: None,
        include_artifacts: None,
        history_length: None,
    };
    let result = handler
        .on_list_tasks(params, None)
        .await
        .expect("list tasks");
    assert_eq!(
        result.tasks.len(),
        2,
        "expected exactly 2 tasks, got {}",
        result.tasks.len()
    );
}

#[tokio::test]
async fn cancel_task_on_working_task() {
    let handler = Arc::new(
        RequestHandlerBuilder::new(CancelableExecutor)
            .build()
            .expect("build handler"),
    );

    // Send a streaming message to get a task in-progress.
    let result = handler
        .on_send_message(make_send_params("work"), true, None)
        .await
        .expect("send message");

    // Get the task ID from the store (list all tasks).
    let list = handler
        .on_list_tasks(
            ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: None,
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            },
            None,
        )
        .await
        .expect("list tasks");

    assert!(
        !list.tasks.is_empty(),
        "task store must contain at least one task"
    );
    let task = &list.tasks[0];

    // Cancel the task (it should be in Pending or Working state).
    let cancel_params = CancelTaskParams {
        tenant: None,
        id: task.id.0.clone(),
        metadata: None,
    };
    let canceled = handler
        .on_cancel_task(cancel_params, None)
        .await
        .expect("cancel");
    assert_eq!(
        canceled.status.state,
        TaskState::Canceled,
        "canceled task must be in Canceled state"
    );

    // Drop the stream reader to clean up.
    drop(result);
}

#[tokio::test]
async fn cancel_terminal_task_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create and complete a task.
    let result = handler
        .on_send_message(make_send_params("done"), false, None)
        .await
        .expect("send message");

    let task_id = match result {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.0.clone(),
        _ => panic!("expected task, got unexpected variant"),
    };

    // Try to cancel the completed task.
    let cancel_params = CancelTaskParams {
        tenant: None,
        id: task_id,
        metadata: None,
    };
    let err = handler
        .on_cancel_task(cancel_params, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::TaskNotCancelable(_)),
        "expected TaskNotCancelable, got {err:?}"
    );
}

#[tokio::test]
async fn cancel_nonexistent_task_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let cancel_params = CancelTaskParams {
        tenant: None,
        id: "does-not-exist".into(),
        metadata: None,
    };
    let err = handler
        .on_cancel_task(cancel_params, None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::TaskNotFound(ref id) if id.0.as_str() == "does-not-exist"),
        "expected TaskNotFound for 'does-not-exist', got {err:?}"
    );
}
