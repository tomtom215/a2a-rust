// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for `ServerError` to `A2aError` mapping and `Display` coverage.

use super::*;

#[tokio::test]
async fn server_error_to_a2a_error_mapping() {
    let mappings: Vec<(ServerError, ErrorCode)> = vec![
        (
            ServerError::TaskNotFound(TaskId::new("x")),
            ErrorCode::TaskNotFound,
        ),
        (
            ServerError::TaskNotCancelable(TaskId::new("x")),
            ErrorCode::TaskNotCancelable,
        ),
        (
            ServerError::InvalidParams("x".into()),
            ErrorCode::InvalidParams,
        ),
        (
            ServerError::MethodNotFound("x".into()),
            ErrorCode::MethodNotFound,
        ),
        (
            ServerError::PushNotSupported,
            ErrorCode::PushNotificationNotSupported,
        ),
        (ServerError::Internal("x".into()), ErrorCode::InternalError),
        (ServerError::Transport("x".into()), ErrorCode::InternalError),
        (
            ServerError::HttpClient("x".into()),
            ErrorCode::InternalError,
        ),
        (
            ServerError::PayloadTooLarge("x".into()),
            ErrorCode::InternalError,
        ),
    ];
    for (server_err, expected_code) in mappings {
        let a2a_err = server_err.to_a2a_error();
        assert_eq!(
            a2a_err.code, expected_code,
            "mapping failed for {server_err}"
        );
        assert!(
            !a2a_err.message.is_empty(),
            "mapped error message must not be empty for {expected_code:?}"
        );
    }
}

#[tokio::test]
async fn server_error_display_all_variants() {
    let errors: Vec<ServerError> = vec![
        ServerError::TaskNotFound(TaskId::new("t1")),
        ServerError::TaskNotCancelable(TaskId::new("t2")),
        ServerError::InvalidParams("bad param".into()),
        ServerError::HttpClient("conn refused".into()),
        ServerError::Transport("timeout".into()),
        ServerError::PushNotSupported,
        ServerError::Internal("something broke".into()),
        ServerError::MethodNotFound("unknown".into()),
        ServerError::PayloadTooLarge("too big".into()),
        ServerError::InvalidStateTransition {
            task_id: TaskId::new("t3"),
            from: TaskState::Completed,
            to: TaskState::Working,
        },
    ];
    for err in &errors {
        let display = err.to_string();
        assert!(
            !display.is_empty(),
            "display should not be empty for {err:?}"
        );
        // Ensure Display output contains some discriminating content
        assert!(
            display.len() > 3,
            "display too short for {err:?}: \"{display}\""
        );
    }
}
