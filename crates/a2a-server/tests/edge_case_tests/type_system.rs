// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Type-level tests: newtype wrappers, state transitions, error codes, and
//! `Part` constructors.

use super::*;

#[tokio::test]
async fn task_status_with_timestamp_has_value() {
    let status = TaskStatus::with_timestamp(TaskState::Working);
    let ts = status
        .timestamp
        .as_deref()
        .expect("timestamp must be present");
    assert!(ts.ends_with('Z'), "timestamp should be UTC: {ts}");
    assert!(ts.contains('T'), "timestamp should be ISO 8601: {ts}");
}

#[tokio::test]
async fn id_newtypes_from_impls() {
    let task_id: TaskId = "my-task".into();
    assert_eq!(task_id.as_ref(), "my-task");

    let task_id: TaskId = String::from("my-task").into();
    assert_eq!(task_id.0, "my-task");

    let ctx_id: ContextId = "ctx-1".into();
    assert_eq!(ctx_id.as_ref(), "ctx-1");

    let msg_id: a2a_protocol_types::MessageId = "msg-1".into();
    assert_eq!(msg_id.as_ref(), "msg-1");

    let art_id: a2a_protocol_types::ArtifactId = "art-1".into();
    assert_eq!(art_id.as_ref(), "art-1");
}

#[tokio::test]
async fn task_state_transitions_comprehensive() {
    // Valid transitions
    assert!(TaskState::Submitted.can_transition_to(TaskState::Working));
    assert!(TaskState::Submitted.can_transition_to(TaskState::Failed));
    assert!(TaskState::Submitted.can_transition_to(TaskState::Canceled));
    assert!(TaskState::Submitted.can_transition_to(TaskState::Rejected));
    assert!(TaskState::Working.can_transition_to(TaskState::Completed));
    assert!(TaskState::Working.can_transition_to(TaskState::Failed));
    assert!(TaskState::Working.can_transition_to(TaskState::InputRequired));
    assert!(TaskState::Working.can_transition_to(TaskState::AuthRequired));
    assert!(TaskState::InputRequired.can_transition_to(TaskState::Working));
    assert!(TaskState::AuthRequired.can_transition_to(TaskState::Working));

    // Invalid transitions — terminal states can't transition
    assert!(!TaskState::Completed.can_transition_to(TaskState::Working));
    assert!(!TaskState::Failed.can_transition_to(TaskState::Working));
    assert!(!TaskState::Canceled.can_transition_to(TaskState::Working));
    assert!(!TaskState::Rejected.can_transition_to(TaskState::Working));

    // Invalid transitions — can't go backwards
    assert!(!TaskState::Submitted.can_transition_to(TaskState::Completed));
    assert!(!TaskState::Submitted.can_transition_to(TaskState::InputRequired));

    // Unspecified can transition to anything
    assert!(TaskState::Unspecified.can_transition_to(TaskState::Working));
    assert!(TaskState::Unspecified.can_transition_to(TaskState::Completed));
}

#[tokio::test]
async fn a2a_error_non_exhaustive() {
    // Verify A2aError can be created via constructors (not struct literal)
    let err = A2aError::new(ErrorCode::InternalError, "test");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert_eq!(err.message, "test");
    assert!(
        err.data.is_none(),
        "data must be None for basic constructor"
    );

    let err = A2aError::with_data(
        ErrorCode::InvalidParams,
        "bad",
        serde_json::json!({"detail": "extra"}),
    );
    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert_eq!(err.message, "bad");
    let data = err
        .data
        .expect("data must be present when constructed with_data");
    assert_eq!(data, serde_json::json!({"detail": "extra"}));
}

#[tokio::test]
async fn error_code_all_values_roundtrip() {
    let codes = [
        ErrorCode::ParseError,
        ErrorCode::InvalidRequest,
        ErrorCode::MethodNotFound,
        ErrorCode::InvalidParams,
        ErrorCode::InternalError,
        ErrorCode::TaskNotFound,
        ErrorCode::TaskNotCancelable,
        ErrorCode::PushNotificationNotSupported,
        ErrorCode::UnsupportedOperation,
        ErrorCode::ContentTypeNotSupported,
        ErrorCode::InvalidAgentResponse,
        ErrorCode::ExtendedAgentCardNotConfigured,
        ErrorCode::ExtensionSupportRequired,
        ErrorCode::VersionNotSupported,
    ];
    for code in &codes {
        let n: i32 = (*code).into();
        let back = ErrorCode::try_from(n).unwrap();
        assert_eq!(back, *code, "roundtrip failed for code {n}");
        // Verify Display produces a non-empty string
        let display = format!("{code}");
        assert!(
            !display.is_empty(),
            "Display for {code:?} must not be empty"
        );
        // Verify default_message produces a non-empty string
        let msg = code.default_message();
        assert!(
            !msg.is_empty(),
            "default_message for {code:?} must not be empty"
        );
    }
}

#[tokio::test]
async fn task_version_from_u64() {
    let v: TaskVersion = 42u64.into();
    assert_eq!(v.get(), 42);
    assert!(TaskVersion::new(2) > TaskVersion::new(1));
    assert!(
        !(TaskVersion::new(1) > TaskVersion::new(1)),
        "equal versions must not be greater"
    );
}

#[tokio::test]
async fn part_constructors() {
    let text = Part::text("hello");
    assert!(
        matches!(text.content, PartContent::Text { .. }),
        "Part::text should produce PartContent::Text"
    );

    let raw = Part::raw("base64data");
    assert!(
        matches!(raw.content, PartContent::File { .. }),
        "Part::raw should produce PartContent::File"
    );

    let url = Part::url("https://example.com");
    assert!(
        matches!(url.content, PartContent::File { .. }),
        "Part::url should produce PartContent::File"
    );

    let data = Part::data(serde_json::json!({"key": "value"}));
    assert!(
        matches!(data.content, PartContent::Data { .. }),
        "Part::data should produce PartContent::Data"
    );
}
