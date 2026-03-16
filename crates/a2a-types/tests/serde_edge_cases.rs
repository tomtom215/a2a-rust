// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Serde edge-case tests for A2A types.
//!
//! Covers unknown enum variants, null vs missing optional fields,
//! `JsonRpcError::with_data()`, and `SendMessageConfiguration::default()`.

use a2a_protocol_types::jsonrpc::JsonRpcError;
use a2a_protocol_types::params::SendMessageConfiguration;
use a2a_protocol_types::task::{Task, TaskState};

// ── Unknown enum variants ────────────────────────────────────────────────────

#[test]
fn unknown_task_state_variant_fails_gracefully() {
    let result = serde_json::from_str::<TaskState>("\"TASK_STATE_FUTURE_VALUE\"");
    assert!(
        result.is_err(),
        "unknown TaskState variant should fail to deserialize"
    );
}

#[test]
fn unknown_message_role_variant_fails_gracefully() {
    use a2a_protocol_types::message::MessageRole;

    let result = serde_json::from_str::<MessageRole>("\"ROLE_FUTURE_VALUE\"");
    assert!(
        result.is_err(),
        "unknown MessageRole variant should fail to deserialize"
    );
}

#[test]
fn unknown_task_state_in_task_json_fails() {
    let json = r#"{
        "id": "task-x",
        "contextId": "ctx-x",
        "status": {"state": "TASK_STATE_FUTURE_VALUE"}
    }"#;
    let result = serde_json::from_str::<Task>(json);
    assert!(
        result.is_err(),
        "Task with unknown state should fail to deserialize"
    );
}

// ── Null vs missing optional fields ──────────────────────────────────────────

#[test]
fn task_with_explicit_null_optional_fields() {
    // Fields explicitly set to null should deserialize as None.
    let json = r#"{
        "id": "task-null",
        "contextId": "ctx-null",
        "status": {"state": "TASK_STATE_SUBMITTED"},
        "history": null,
        "artifacts": null,
        "metadata": null
    }"#;
    let task: Task = serde_json::from_str(json).expect("explicit nulls should parse");
    assert!(task.history.is_none());
    assert!(task.artifacts.is_none());
    assert!(task.metadata.is_none());
}

#[test]
fn task_with_omitted_optional_fields() {
    // Fields omitted entirely should also deserialize as None.
    let json = r#"{
        "id": "task-omit",
        "contextId": "ctx-omit",
        "status": {"state": "TASK_STATE_SUBMITTED"}
    }"#;
    let task: Task = serde_json::from_str(json).expect("omitted fields should parse");
    assert!(task.history.is_none());
    assert!(task.artifacts.is_none());
    assert!(task.metadata.is_none());
}

#[test]
fn null_and_omitted_tasks_are_equivalent() {
    let json_null = r#"{
        "id": "t1",
        "contextId": "c1",
        "status": {"state": "TASK_STATE_WORKING"},
        "history": null,
        "artifacts": null,
        "metadata": null
    }"#;
    let json_omit = r#"{
        "id": "t1",
        "contextId": "c1",
        "status": {"state": "TASK_STATE_WORKING"}
    }"#;
    let task_null: Task = serde_json::from_str(json_null).unwrap();
    let task_omit: Task = serde_json::from_str(json_omit).unwrap();
    // Both should serialize identically (skip_serializing_if = None).
    let ser_null = serde_json::to_string(&task_null).unwrap();
    let ser_omit = serde_json::to_string(&task_omit).unwrap();
    assert_eq!(ser_null, ser_omit);
}

#[test]
fn message_with_explicit_null_optional_fields() {
    use a2a_protocol_types::message::Message;

    let json = r#"{
        "messageId": "msg-null",
        "role": "ROLE_USER",
        "parts": [{"type": "text", "text": "hi"}],
        "taskId": null,
        "contextId": null,
        "referenceTaskIds": null,
        "extensions": null,
        "metadata": null
    }"#;
    let msg: Message = serde_json::from_str(json).expect("explicit nulls should parse");
    assert!(msg.task_id.is_none());
    assert!(msg.context_id.is_none());
    assert!(msg.reference_task_ids.is_none());
    assert!(msg.extensions.is_none());
    assert!(msg.metadata.is_none());
}

// ── JsonRpcError::with_data() ────────────────────────────────────────────────

#[test]
fn jsonrpc_error_with_data() {
    let err = JsonRpcError::with_data(
        -32600,
        "Invalid Request",
        serde_json::json!({"detail": "missing method field"}),
    );
    assert_eq!(err.code, -32600);
    assert_eq!(err.message, "Invalid Request");
    assert!(err.data.is_some());

    let json = serde_json::to_string(&err).expect("serialize");
    assert!(json.contains("\"data\""));
    assert!(json.contains("\"detail\""));

    let back: JsonRpcError = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.code, -32600);
    assert_eq!(back.message, "Invalid Request");
    let data = back.data.expect("data should be present");
    assert_eq!(data["detail"], "missing method field");
}

#[test]
fn jsonrpc_error_without_data_omits_field() {
    let err = JsonRpcError::new(-32601, "Method not found");
    assert!(err.data.is_none());

    let json = serde_json::to_string(&err).expect("serialize");
    assert!(
        !json.contains("\"data\""),
        "data should be omitted when None: {json}"
    );
}

#[test]
fn jsonrpc_error_display() {
    let err = JsonRpcError::new(-32601, "Method not found");
    assert_eq!(err.to_string(), "[-32601] Method not found");
}

// ── SendMessageConfiguration default ─────────────────────────────────────────

#[test]
fn send_message_configuration_default() {
    let config = SendMessageConfiguration::default();
    assert_eq!(config.accepted_output_modes, vec!["text/plain".to_owned()]);
    assert!(config.task_push_notification_config.is_none());
    assert!(config.history_length.is_none());
    assert!(config.return_immediately.is_none());
}

#[test]
fn send_message_configuration_default_roundtrip() {
    let config = SendMessageConfiguration::default();
    let json = serde_json::to_string(&config).expect("serialize default config");
    assert!(json.contains("\"acceptedOutputModes\""));
    assert!(
        !json.contains("\"historyLength\""),
        "None fields should be omitted: {json}"
    );

    let back: SendMessageConfiguration = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.accepted_output_modes, vec!["text/plain".to_owned()]);
    assert!(back.history_length.is_none());
}
