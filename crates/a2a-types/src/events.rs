// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Server-sent event types for A2A streaming.
//!
//! When a client calls `SendStreamingMessage` or `SubscribeToTask`, the server
//! responds with a stream of Server-Sent Events. Each event carries a
//! [`StreamResponse`] JSON payload discriminated by field presence (untagged).
//!
//! # Stream event variants
//!
//! | JSON field | Rust variant |
//! |---|---|
//! | `"task"` | [`crate::task::Task`] |
//! | `"message"` | [`crate::message::Message`] |
//! | `"statusUpdate"` | [`TaskStatusUpdateEvent`] |
//! | `"artifactUpdate"` | [`TaskArtifactUpdateEvent`] |

use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;
use crate::message::Message;
use crate::task::{Task, TaskId, TaskStatus};

// ── TaskStatusUpdateEvent ─────────────────────────────────────────────────────

/// A streaming event that reports a change in task state.
///
/// In v1.0, this wraps [`TaskStatus`] directly instead of separate
/// `state`/`message` fields. The `final` field has been removed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusUpdateEvent {
    /// The task whose status changed.
    pub task_id: TaskId,

    /// Conversation context the task belongs to.
    pub context_id: String,

    /// The new task status (state + optional message + timestamp).
    pub status: TaskStatus,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
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
    pub context_id: String,

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
/// Discriminated by field presence (untagged oneof). Exactly one of
/// `task`, `message`, `statusUpdate`, or `artifactUpdate` is present.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StreamResponse {
    /// A complete task object (initial response before streaming begins).
    Task(Task),

    /// A complete message (returned synchronously for short responses).
    Message(Message),

    /// A task state change event.
    StatusUpdate(TaskStatusUpdateEvent),

    /// An artifact delivery event.
    ArtifactUpdate(TaskArtifactUpdateEvent),
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::ArtifactId;
    use crate::message::Part;
    use crate::task::{ContextId, TaskState};

    #[test]
    fn status_update_event_roundtrip() {
        let event = TaskStatusUpdateEvent {
            task_id: TaskId::new("task-1"),
            context_id: "ctx-1".to_owned(),
            status: TaskStatus::new(TaskState::Completed),
            metadata: None,
        };
        let json = serde_json::to_string(&event).expect("serialize");
        assert!(!json.contains("\"final\""), "v1.0 removed final field");
        assert!(json.contains("\"status\""), "should have status field");

        let back: TaskStatusUpdateEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.status.state, TaskState::Completed);
    }

    #[test]
    fn artifact_update_event_roundtrip() {
        let event = TaskArtifactUpdateEvent {
            task_id: TaskId::new("task-1"),
            context_id: "ctx-1".to_owned(),
            artifact: Artifact::new(ArtifactId::new("art-1"), vec![Part::text("output")]),
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
        assert!(
            !json.contains("\"kind\""),
            "v1.0 should not have kind tag: {json}"
        );

        let back: StreamResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, StreamResponse::Task(_)));
    }

    #[test]
    fn stream_response_status_update_variant() {
        let event = TaskStatusUpdateEvent {
            task_id: TaskId::new("t1"),
            context_id: "c1".to_owned(),
            status: TaskStatus::new(TaskState::Failed),
            metadata: None,
        };
        let resp = StreamResponse::StatusUpdate(event);
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            !json.contains("\"kind\""),
            "v1.0 should not have kind tag: {json}"
        );

        let back: StreamResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, StreamResponse::StatusUpdate(_)));
    }
}
