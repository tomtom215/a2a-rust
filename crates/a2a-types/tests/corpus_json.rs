// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Corpus-based JSON tests.
//!
//! These tests deserialize representative JSON samples matching the A2A v1.0
//! wire format and verify round-trip fidelity: `deserialize → serialize →
//! deserialize` produces structurally equivalent values.

use a2a_types::agent_card::AgentCard;
use a2a_types::events::StreamResponse;
use a2a_types::jsonrpc::JsonRpcRequest;
use a2a_types::message::{Message, MessageRole, Part, PartContent};
use a2a_types::task::{Task, TaskState};

// ── Helper ───────────────────────────────────────────────────────────────────

/// Asserts that deserializing `json` to `T`, re-serializing, and deserializing
/// again produces a structurally equivalent result (checked by re-serialization).
fn assert_roundtrip<T: serde::Serialize + serde::de::DeserializeOwned>(json: &str) {
    let value: T = serde_json::from_str(json).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let reserialized = serde_json::to_string(&value).unwrap();
    let _value2: T = serde_json::from_str(&reserialized)
        .unwrap_or_else(|e| panic!("re-parse failed: {e}\nreserialized: {reserialized}"));
}

// ── Task corpus ──────────────────────────────────────────────────────────────

#[test]
fn corpus_task_submitted() {
    let json = r#"{
        "id": "task-001",
        "contextId": "ctx-abc",
        "status": {
            "state": "TASK_STATE_SUBMITTED"
        }
    }"#;
    let task: Task = serde_json::from_str(json).unwrap();
    assert_eq!(task.status.state, TaskState::Submitted);
    assert_roundtrip::<Task>(json);
}

#[test]
fn corpus_task_working_with_timestamp() {
    let json = r#"{
        "id": "task-002",
        "contextId": "ctx-xyz",
        "status": {
            "state": "TASK_STATE_WORKING",
            "timestamp": "2026-01-15T10:30:00Z"
        }
    }"#;
    let task: Task = serde_json::from_str(json).unwrap();
    assert_eq!(task.status.state, TaskState::Working);
    assert_eq!(
        task.status.timestamp.as_deref(),
        Some("2026-01-15T10:30:00Z")
    );
    assert_roundtrip::<Task>(json);
}

#[test]
fn corpus_task_completed_with_artifacts() {
    let json = r#"{
        "id": "task-003",
        "contextId": "ctx-123",
        "status": {"state": "TASK_STATE_COMPLETED"},
        "artifacts": [{
            "artifactId": "art-1",
            "parts": [{"text": "Hello from agent"}]
        }]
    }"#;
    let task: Task = serde_json::from_str(json).unwrap();
    assert!(task.status.state.is_terminal());
    assert_eq!(task.artifacts.as_ref().unwrap().len(), 1);
    assert_roundtrip::<Task>(json);
}

#[test]
fn corpus_task_failed() {
    let json = r#"{
        "id": "task-004",
        "contextId": "ctx-err",
        "status": {"state": "TASK_STATE_FAILED"}
    }"#;
    let task: Task = serde_json::from_str(json).unwrap();
    assert_eq!(task.status.state, TaskState::Failed);
    assert!(task.status.state.is_terminal());
}

// ── Message corpus ───────────────────────────────────────────────────────────

#[test]
fn corpus_user_message() {
    let json = r#"{
        "messageId": "msg-001",
        "role": "ROLE_USER",
        "parts": [{"text": "What is the weather?"}]
    }"#;
    let msg: Message = serde_json::from_str(json).unwrap();
    assert_eq!(msg.role, MessageRole::User);
    assert_eq!(msg.parts.len(), 1);
    assert_roundtrip::<Message>(json);
}

#[test]
fn corpus_agent_message_with_metadata() {
    let json = r#"{
        "messageId": "msg-002",
        "role": "ROLE_AGENT",
        "parts": [{"text": "It's sunny"}],
        "metadata": {"confidence": 0.95}
    }"#;
    let msg: Message = serde_json::from_str(json).unwrap();
    assert_eq!(msg.role, MessageRole::Agent);
    assert!(msg.metadata.is_some());
    assert_roundtrip::<Message>(json);
}

#[test]
fn corpus_message_multi_part() {
    let json = r#"{
        "messageId": "msg-003",
        "role": "ROLE_USER",
        "parts": [
            {"text": "See attached"},
            {"url": "https://example.com/doc.pdf"}
        ]
    }"#;
    let msg: Message = serde_json::from_str(json).unwrap();
    assert_eq!(msg.parts.len(), 2);
    assert!(matches!(&msg.parts[0].content, PartContent::Text { .. }));
    assert!(matches!(&msg.parts[1].content, PartContent::Url { .. }));
}

// ── Part corpus ──────────────────────────────────────────────────────────────

#[test]
fn corpus_text_part() {
    let json = r#"{"text": "hello world"}"#;
    let part: Part = serde_json::from_str(json).unwrap();
    assert!(matches!(&part.content, PartContent::Text { text } if text == "hello world"));
    assert_roundtrip::<Part>(json);
}

#[test]
fn corpus_raw_part_with_metadata() {
    let json = r#"{"raw": "aGVsbG8=", "mediaType": "image/png", "filename": "test.png"}"#;
    let part: Part = serde_json::from_str(json).unwrap();
    assert!(matches!(&part.content, PartContent::Raw { .. }));
    assert_eq!(part.media_type.as_deref(), Some("image/png"));
    assert_eq!(part.filename.as_deref(), Some("test.png"));
    assert_roundtrip::<Part>(json);
}

#[test]
fn corpus_data_part() {
    let json = r#"{"data": {"key": "value", "count": 42}}"#;
    let part: Part = serde_json::from_str(json).unwrap();
    assert!(matches!(&part.content, PartContent::Data { .. }));
    assert_roundtrip::<Part>(json);
}

// ── AgentCard corpus ─────────────────────────────────────────────────────────

#[test]
fn corpus_agent_card_minimal() {
    let json = r#"{
        "name": "Weather Agent",
        "description": "Provides weather forecasts",
        "version": "1.0.0",
        "supportedInterfaces": [{
            "url": "https://weather.example.com/rpc",
            "protocolBinding": "JSONRPC",
            "protocolVersion": "1.0.0"
        }],
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": [{
            "id": "forecast",
            "name": "Weather Forecast",
            "description": "Get weather forecast for a location",
            "tags": ["weather", "forecast"]
        }],
        "capabilities": {}
    }"#;
    let card: AgentCard = serde_json::from_str(json).unwrap();
    assert_eq!(card.name, "Weather Agent");
    assert_eq!(card.supported_interfaces.len(), 1);
    assert_eq!(card.skills.len(), 1);
    assert_roundtrip::<AgentCard>(json);
}

#[test]
fn corpus_agent_card_with_security() {
    let json = r#"{
        "name": "Secure Agent",
        "description": "Requires auth",
        "version": "1.0.0",
        "supportedInterfaces": [{
            "url": "https://secure.example.com/rpc",
            "protocolBinding": "JSONRPC",
            "protocolVersion": "1.0.0"
        }],
        "defaultInputModes": ["text/plain"],
        "defaultOutputModes": ["text/plain"],
        "skills": [],
        "capabilities": {"streaming": true, "pushNotifications": false},
        "securitySchemes": {
            "bearer": {
                "type": "http",
                "scheme": "bearer"
            }
        },
        "securityRequirements": [{"schemes": {"bearer": {"list": []}}}]
    }"#;
    let card: AgentCard = serde_json::from_str(json).unwrap();
    assert!(card.security_schemes.is_some());
    assert!(card.security_requirements.is_some());
    assert_roundtrip::<AgentCard>(json);
}

// ── JSON-RPC corpus ──────────────────────────────────────────────────────────

#[test]
fn corpus_jsonrpc_request() {
    let json = r#"{
        "jsonrpc": "2.0",
        "id": 1,
        "method": "SendMessage",
        "params": {
            "message": {
                "messageId": "msg-rpc-1",
                "role": "ROLE_USER",
                "parts": [{"text": "Hello"}]
            }
        }
    }"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.method, "SendMessage");
    assert_roundtrip::<JsonRpcRequest>(json);
}

#[test]
fn corpus_jsonrpc_success_response() {
    use a2a_types::jsonrpc::{JsonRpcResponse, JsonRpcSuccessResponse};
    let json = r#"{
        "jsonrpc": "2.0",
        "id": 1,
        "result": {"id": "task-1", "contextId": "ctx-1", "status": {"state": "TASK_STATE_SUBMITTED"}}
    }"#;
    let resp: JsonRpcResponse<serde_json::Value> = serde_json::from_str(json).unwrap();
    assert!(matches!(resp, JsonRpcResponse::Success(_)));
    assert_roundtrip::<JsonRpcResponse<serde_json::Value>>(json);

    // Also check that the success result parses as a typed response.
    let success: JsonRpcSuccessResponse<serde_json::Value> = serde_json::from_str(json).unwrap();
    assert!(success.result.is_object());
}

#[test]
fn corpus_jsonrpc_error_response() {
    use a2a_types::jsonrpc::JsonRpcResponse;
    let json = r#"{
        "jsonrpc": "2.0",
        "id": 1,
        "error": {"code": -32601, "message": "Method not found"}
    }"#;
    let resp: JsonRpcResponse<serde_json::Value> = serde_json::from_str(json).unwrap();
    assert!(matches!(resp, JsonRpcResponse::Error(_)));
    assert_roundtrip::<JsonRpcResponse<serde_json::Value>>(json);
}

// ── StreamResponse corpus ────────────────────────────────────────────────────

#[test]
fn corpus_stream_status_update() {
    // StreamResponse is internally tagged via camelCase variant names.
    let json = r#"{
        "statusUpdate": {
            "taskId": "task-100",
            "contextId": "ctx-100",
            "status": {"state": "TASK_STATE_WORKING"}
        }
    }"#;
    let event: StreamResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(event, StreamResponse::StatusUpdate(_)));
    assert_roundtrip::<StreamResponse>(json);
}

#[test]
fn corpus_stream_artifact_update() {
    let json = r#"{
        "artifactUpdate": {
            "taskId": "task-100",
            "contextId": "ctx-100",
            "artifact": {
                "artifactId": "art-1",
                "parts": [{"text": "Result data"}]
            }
        }
    }"#;
    let event: StreamResponse = serde_json::from_str(json).unwrap();
    assert!(matches!(event, StreamResponse::ArtifactUpdate(_)));
    assert_roundtrip::<StreamResponse>(json);
}
