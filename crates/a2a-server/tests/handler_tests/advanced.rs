// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Advanced handler tests covering `return_immediately`, task continuation with
//! context reuse, context/task-id mismatch rejection, interceptor rejection,
//! resubscription errors, and `ServerError`-to-`A2aError` mapping.

use super::*;

// ── Resubscribe tests ───────────────────────────────────────────────────────

#[tokio::test]
async fn resubscribe_nonexistent_task_fails() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let params = TaskIdParams {
        tenant: None,
        id: "nonexistent".into(),
    };
    let err = handler.on_resubscribe(params, None).await.unwrap_err();
    assert!(
        matches!(err, a2a_protocol_server::ServerError::TaskNotFound(ref id) if id.0.as_str() == "nonexistent"),
        "expected TaskNotFound for 'nonexistent', got {err:?}"
    );
}

// ── Phase 7 tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn return_immediately_returns_pending_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    let params = MessageSendParams {
        tenant: None,
        message: make_message("hello"),
        configuration: Some(a2a_protocol_types::params::SendMessageConfiguration {
            accepted_output_modes: vec!["text/plain".into()],
            task_push_notification_config: None,
            history_length: None,
            return_immediately: Some(true),
        }),
        metadata: None,
    };

    let result = handler
        .on_send_message(params, false, None)
        .await
        .expect("send message");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(
                task.status.state,
                TaskState::Submitted,
                "return_immediately should return Submitted task, got {:?}",
                task.status.state
            );
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn task_continuation_same_context_finds_stored_task() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // First message creates a task with a specific context_id.
    let mut msg1 = make_message("first");
    msg1.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-continuation"));

    let result1 = handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg1,
                configuration: None,
                metadata: None,
            },
            false,
            None,
        )
        .await
        .expect("first send");

    let task_id_1 = match result1 {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.clone(),
        _ => panic!("expected task"),
    };

    // Second message with same context_id should create a NEW task but have
    // stored_task set. We verify indirectly by checking two tasks exist.
    let mut msg2 = make_message("second");
    msg2.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-continuation"));

    let result2 = handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg2,
                configuration: None,
                metadata: None,
            },
            false,
            None,
        )
        .await
        .expect("second send");

    let task_id_2 = match result2 {
        SendMessageResult::Response(SendMessageResponse::Task(t)) => t.id.clone(),
        _ => panic!("expected task"),
    };

    // Two different tasks should be created.
    assert_ne!(task_id_1, task_id_2, "second send should create a new task");

    // Both tasks should be in the store.
    let list = handler
        .on_list_tasks(
            ListTasksParams {
                tenant: None,
                context_id: Some("ctx-continuation".into()),
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
    assert_eq!(
        list.tasks.len(),
        2,
        "should have exactly 2 tasks for the context, got {}",
        list.tasks.len()
    );
}

#[tokio::test]
async fn context_task_mismatch_rejected() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .build()
        .expect("build handler");

    // Create a task with a specific context.
    let mut msg1 = make_message("first");
    msg1.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-mismatch"));

    handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg1,
                configuration: None,
                metadata: None,
            },
            false,
            None,
        )
        .await
        .expect("first send");

    // Second message with same context but WRONG task_id should be rejected.
    let mut msg2 = make_message("second");
    msg2.context_id = Some(a2a_protocol_types::task::ContextId::new("ctx-mismatch"));
    msg2.task_id = Some(TaskId::new("wrong-task-id"));

    let result = handler
        .on_send_message(
            MessageSendParams {
                tenant: None,
                message: msg2,
                configuration: None,
                metadata: None,
            },
            false,
            None,
        )
        .await;
    let err = result.err().expect("expected error for task_id mismatch");

    assert!(
        matches!(err, a2a_protocol_server::ServerError::InvalidParams(ref msg) if msg.contains("task") || msg.contains("mismatch")),
        "expected InvalidParams for task_id mismatch, got {err:?}"
    );
}

#[tokio::test]
async fn interceptor_rejection_stops_processing() {
    let handler = RequestHandlerBuilder::new(EchoExecutor)
        .with_interceptor(RejectInterceptor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("hello"), false, None)
        .await;
    let err = result.err().expect("expected error from interceptor");

    // The error should be a Protocol error from the interceptor.
    assert!(
        matches!(err, a2a_protocol_server::ServerError::Protocol(ref a2a_err) if a2a_err.message.contains("rejected by interceptor")),
        "expected Protocol error with 'rejected by interceptor' message, got {err:?}"
    );
}

// ── Error conversion tests ──────────────────────────────────────────────────

#[test]
fn server_error_to_a2a_error_mapping() {
    use a2a_protocol_server::ServerError;
    use a2a_protocol_types::error::ErrorCode;

    let cases = vec![
        (
            ServerError::TaskNotFound(TaskId::new("t1")),
            ErrorCode::TaskNotFound,
            "t1",
        ),
        (
            ServerError::TaskNotCancelable(TaskId::new("t1")),
            ErrorCode::TaskNotCancelable,
            "t1",
        ),
        (
            ServerError::InvalidParams("bad".into()),
            ErrorCode::InvalidParams,
            "bad",
        ),
        (
            ServerError::PushNotSupported,
            ErrorCode::PushNotificationNotSupported,
            "",
        ),
        (
            ServerError::MethodNotFound("Foo".into()),
            ErrorCode::MethodNotFound,
            "Foo",
        ),
        (
            ServerError::Internal("oops".into()),
            ErrorCode::InternalError,
            "oops",
        ),
    ];

    for (server_err, expected_code, expected_substr) in cases {
        let display_msg = format!("{server_err}");
        let a2a_err = server_err.to_a2a_error();
        assert_eq!(
            a2a_err.code, expected_code,
            "mapping mismatch: display was '{display_msg}'"
        );
        if !expected_substr.is_empty() {
            assert!(
                a2a_err.message.contains(expected_substr),
                "expected error message to contain '{expected_substr}', got '{}'",
                a2a_err.message
            );
        }
    }
}
