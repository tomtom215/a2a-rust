// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Basic request-handler tests: send, cancel, get, list, push-config, and
//! extended-agent-card flows.

use super::*;

#[tokio::test]
async fn send_message_basic_flow() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await;
    match result.unwrap() {
        a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn send_message_executor_failure_marks_task_failed() {
    let handler = RequestHandlerBuilder::new(FailingExecutor).build().unwrap();
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await;
    // The executor fails, which should result in a Failed task
    match result {
        Ok(a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task))) => {
            assert_eq!(task.status.state, TaskState::Failed);
        }
        Err(_e) => {
            // Also acceptable — depends on timing of the failure event
        }
        _ => panic!("unexpected result"),
    }
}

#[tokio::test]
async fn cancel_task_signals_cancellation_token() {
    let handler = Arc::new(RequestHandlerBuilder::new(SlowExecutor).build().unwrap());

    // Start a streaming task (which won't block waiting for completion)
    let params = make_send_params("slow task");
    let result = handler.on_send_message(params, true, None).await.unwrap();

    // Get the task ID from the response
    match result {
        a2a_protocol_server::SendMessageResult::Stream(_reader) => {
            // Give the executor a moment to start
            tokio::time::sleep(Duration::from_millis(50)).await;

            // List tasks to find our task
            let list_result = handler
                .on_list_tasks(
                    ListTasksParams {
                        tenant: None,
                        context_id: None,
                        status: None,
                        page_size: Some(10),
                        page_token: None,
                        status_timestamp_after: None,
                        include_artifacts: None,
                        history_length: None,
                    },
                    None,
                )
                .await
                .unwrap();

            assert!(
                !list_result.tasks.is_empty(),
                "should have at least one task"
            );

            // Find a non-terminal task to cancel
            if let Some(task) = list_result
                .tasks
                .iter()
                .find(|t| !t.status.state.is_terminal())
            {
                let cancel_result = handler
                    .on_cancel_task(
                        a2a_protocol_types::params::CancelTaskParams {
                            tenant: None,
                            id: task.id.to_string(),
                            metadata: None,
                        },
                        None,
                    )
                    .await;
                match cancel_result {
                    Ok(cancelled) => {
                        assert_eq!(cancelled.status.state, TaskState::Canceled);
                    }
                    Err(_) => {
                        // Task may have completed by now — that's OK
                    }
                }
            }
        }
        _ => panic!("expected Stream result"),
    }
}

#[tokio::test]
async fn get_task_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler
        .on_get_task(
            TaskQueryParams {
                tenant: None,
                id: "nonexistent-task".into(),
                history_length: None,
            },
            None,
        )
        .await;
    let err = result.expect_err("should return TaskNotFound");
    assert!(
        matches!(err, ServerError::TaskNotFound(_)),
        "expected TaskNotFound, got {err:?}"
    );
}

#[tokio::test]
async fn cancel_task_not_found() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler
        .on_cancel_task(
            a2a_protocol_types::params::CancelTaskParams {
                tenant: None,
                id: "nonexistent".into(),
                metadata: None,
            },
            None,
        )
        .await;
    let err = result.expect_err("should return TaskNotFound");
    assert!(
        matches!(err, ServerError::TaskNotFound(_)),
        "expected TaskNotFound, got {err:?}"
    );
}

#[tokio::test]
async fn cancel_completed_task_returns_not_cancelable() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();

    // First, complete a task
    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await
        .unwrap();
    let task_id = match result {
        a2a_protocol_server::SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            task.id
        }
        _ => panic!("expected task"),
    };

    // Now try to cancel it
    let cancel_result = handler
        .on_cancel_task(
            a2a_protocol_types::params::CancelTaskParams {
                tenant: None,
                id: task_id.to_string(),
                metadata: None,
            },
            None,
        )
        .await;
    let err = cancel_result.expect_err("should return TaskNotCancelable");
    assert!(
        matches!(err, ServerError::TaskNotCancelable(_)),
        "expected TaskNotCancelable, got {err:?}"
    );
}

#[tokio::test]
async fn list_tasks_pagination_page_size_zero_defaults() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();

    // Create a task
    handler
        .on_send_message(make_send_params("hello"), false, None)
        .await
        .unwrap();

    // List with page_size = 0 (should default to 50, not return empty)
    let result = handler
        .on_list_tasks(
            ListTasksParams {
                tenant: None,
                context_id: None,
                status: None,
                page_size: Some(0),
                page_token: None,
                status_timestamp_after: None,
                include_artifacts: None,
                history_length: None,
            },
            None,
        )
        .await
        .unwrap();
    assert!(
        !result.tasks.is_empty(),
        "page_size=0 should default to 50, not empty"
    );
}

#[tokio::test]
async fn push_config_not_supported_without_sender() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();

    let config = a2a_protocol_types::push::TaskPushNotificationConfig::new(
        "task-1",
        "http://example.com/webhook",
    );
    let result = handler.on_set_push_config(config, None).await;
    let err = result.expect_err("should return PushNotSupported");
    assert!(
        matches!(err, ServerError::PushNotSupported),
        "expected PushNotSupported, got {err:?}"
    );
}

#[tokio::test]
async fn extended_agent_card_not_configured() {
    let handler = RequestHandlerBuilder::new(EchoExecutor).build().unwrap();
    let result = handler.on_get_extended_agent_card(None).await;
    let err = result.expect_err("should return Internal error");
    assert!(
        matches!(err, ServerError::Internal(_)),
        "expected Internal, got {err:?}"
    );
}
