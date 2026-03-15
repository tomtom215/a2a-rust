// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server-sent event types for A2A streaming.
//!
//! When a client calls `message/stream` or `tasks/resubscribe`, the server
//! responds with a stream of Server-Sent Events. Each event carries a
//! [`StreamResponse`] JSON payload discriminated on the `"kind"` field.
//!
//! # Stream event variants
//!
//! | `kind` value | Rust type |
//! |---|---|
//! | `"task"` | [`crate::task::Task`] |
//! | `"message"` | [`crate::message::Message`] |
//! | `"status-update"` | [`TaskStatusUpdateEvent`] |
//! | `"artifact-update"` | [`TaskArtifactUpdateEvent`] |

use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;
use crate::message::Message;
use crate::task::{ContextId, Task, TaskId, TaskState};

// ── TaskStatusUpdateEvent ─────────────────────────────────────────────────────

/// A streaming event that reports a change in task state.
///
/// The `r#final` field uses the raw identifier syntax because `final` is a
/// reserved keyword in Rust; it serializes as `"final"` in JSON.
///
/// The wire `kind` field (`"status-update"`) is injected by the enclosing
/// [`StreamResponse`] discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusUpdateEvent {
    /// The task whose status changed.
    pub task_id: TaskId,

    /// Conversation context the task belongs to.
    pub context_id: ContextId,

    /// New task state.
    pub state: TaskState,

    /// Optional agent message accompanying this status change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Whether this is the last event for this task stream.
    ///
    /// Uses the raw identifier `r#final` because `final` is a Rust keyword.
    #[serde(rename = "final")]
    pub r#final: bool,
}

// ── TaskArtifactUpdateEvent ───────────────────────────────────────────────────

/// A streaming event that delivers a new or updated artifact.
///
/// The wire `kind` field (`"artifact-update"`) is injected by the enclosing
/// [`StreamResponse`] discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArtifactUpdateEvent {
    /// The task that produced the artifact.
    pub task_id: TaskId,

    /// Conversation context the task belongs to.
    pub context_id: ContextId,

    /// The artifact being delivered.
    pub artifact: Artifact,

    /// If `true`, this event's artifact parts should be appended to the
    /// previously-received artifact with the same ID rather than replacing it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub append: Option<bool>,

    /// If `true`, this is the final chunk for the artifact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_chunk: Option<bool>,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── StreamResponse ────────────────────────────────────────────────────────────

/// A single event payload in an A2A streaming response.
///
/// Discriminated on the `"kind"` field. Clients receive these events via
/// Server-Sent Events (SSE).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StreamResponse {
    /// A complete task object (initial response before streaming begins).
    Task(Task),

    /// A complete message (returned synchronously for short responses).
    Message(Message),

    /// A task state change event.
    #[serde(rename = "status-update")]
    StatusUpdate(TaskStatusUpdateEvent),

    /// An artifact delivery event.
    #[serde(rename = "artifact-update")]
    ArtifactUpdate(TaskArtifactUpdateEvent),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::ArtifactId;
    use crate::message::{Part, TextPart};
    use crate::task::TaskStatus;

    #[test]
    fn status_update_event_roundtrip() {
        let event = TaskStatusUpdateEvent {
            task_id: TaskId::new("task-1"),
            context_id: ContextId::new("ctx-1"),
            state: TaskState::Completed,
            message: None,
            metadata: None,
            r#final: true,
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(json.contains("\"final\":true"), "final field: {json}");

        let back: TaskStatusUpdateEvent = serde_json::from_str(&json).expect("deserialize");
        assert!(back.r#final);
        assert_eq!(back.state, TaskState::Completed);
    }

    #[test]
    fn artifact_update_event_roundtrip() {
        let event = TaskArtifactUpdateEvent {
            task_id: TaskId::new("task-1"),
            context_id: ContextId::new("ctx-1"),
            artifact: Artifact::new(
                ArtifactId::new("art-1"),
                vec![Part::Text(TextPart::new("output"))],
            ),
            append: Some(false),
            last_chunk: Some(true),
            metadata: None,
        };
        let json = serde_json::to_string(&event).expect("serialize");
        let back: TaskArtifactUpdateEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.last_chunk, Some(true));
    }

    #[test]
    fn stream_response_task_variant() {
        let task = Task {
            id: TaskId::new("t1"),
            context_id: ContextId::new("c1"),
            status: TaskStatus::new(TaskState::Working),
            history: None,
            artifacts: None,
            metadata: None,
        };
        let resp = StreamResponse::Task(task);
        let json = serde_json::to_string(&resp).expect("serialize");
        // StreamResponse::Task adds kind="task" via the outer enum tag
        assert!(
            json.contains("\"kind\":\"task\""),
            "expected task kind: {json}"
        );

        let back: StreamResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, StreamResponse::Task(_)));
    }

    #[test]
    fn stream_response_status_update_variant() {
        let event = TaskStatusUpdateEvent {
            task_id: TaskId::new("t1"),
            context_id: ContextId::new("c1"),
            state: TaskState::Failed,
            message: None,
            metadata: None,
            r#final: true,
        };
        let resp = StreamResponse::StatusUpdate(event);
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            json.contains("\"kind\":\"status-update\""),
            "expected status-update kind: {json}"
        );

        let back: StreamResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, StreamResponse::StatusUpdate(_)));
    }
}
