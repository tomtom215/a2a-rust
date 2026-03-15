// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Task types for the A2A protocol.
//!
//! A [`Task`] is the stateful unit of work managed by an agent. Its lifecycle
//! is tracked through [`TaskStatus`] and [`TaskState`]. The [`TaskState`] enum
//! uses `SCREAMING_SNAKE_CASE` with type prefix per `ProtoJSON` convention
//! (e.g. `"TASK_STATE_INPUT_REQUIRED"`).
//!
//! # ID newtypes
//!
//! [`TaskId`], [`ContextId`], and [`TaskVersion`] are newtypes over `String`
//! (or `u64`) for compile-time type safety.

use serde::{Deserialize, Serialize};

use crate::artifact::Artifact;
use crate::message::Message;

// ── TaskId ────────────────────────────────────────────────────────────────────

/// Opaque unique identifier for a [`Task`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub String);

impl TaskId {
    /// Creates a new [`TaskId`] from any string-like value.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for TaskId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for TaskId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for TaskId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── ContextId ─────────────────────────────────────────────────────────────────

/// Opaque unique identifier for a conversation context.
///
/// A context groups related tasks under a single logical conversation thread.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContextId(pub String);

impl ContextId {
    /// Creates a new [`ContextId`] from any string-like value.
    #[must_use]
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for ContextId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ContextId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ContextId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl AsRef<str> for ContextId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── TaskVersion ───────────────────────────────────────────────────────────────

/// Monotonically increasing version counter for optimistic concurrency control.
///
/// Incremented every time a [`Task`] is mutated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TaskVersion(pub u64);

impl TaskVersion {
    /// Creates a [`TaskVersion`] from a `u64`.
    #[must_use]
    pub const fn new(v: u64) -> Self {
        Self(v)
    }

    /// Returns the inner `u64` value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for TaskVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for TaskVersion {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

// ── TaskState ─────────────────────────────────────────────────────────────────

/// The lifecycle state of a [`Task`].
///
/// Uses `SCREAMING_SNAKE_CASE` with `TASK_STATE_` prefix per `ProtoJSON`
/// convention (e.g. `TaskState::InputRequired` ↔ `"TASK_STATE_INPUT_REQUIRED"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskState {
    /// Proto default (0-value); should not appear in normal usage.
    #[serde(rename = "TASK_STATE_UNSPECIFIED")]
    Unspecified,
    /// Task received, not yet started.
    #[serde(rename = "TASK_STATE_PENDING")]
    Pending,
    /// Task is actively being processed.
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    /// Agent requires additional input from the client to proceed.
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    /// Agent requires the client to complete an authentication step.
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
    /// Task finished successfully.
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    /// Task finished with an error.
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    /// Task was canceled by the client.
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    /// Task was rejected by the agent before execution.
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
}

impl TaskState {
    /// Returns `true` if this state is a terminal (final) state.
    ///
    /// Terminal states: `Completed`, `Failed`, `Canceled`, `Rejected`.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::Rejected
        )
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Use serde to produce the canonical SCREAMING_SNAKE_CASE representation.
        let s = serde_json::to_string(self).unwrap_or_else(|_| "TASK_STATE_UNSPECIFIED".into());
        // serde_json wraps strings in quotes; strip them.
        f.write_str(s.trim_matches('"'))
    }
}

// ── TaskStatus ────────────────────────────────────────────────────────────────

/// The current status of a [`Task`], combining state with an optional message
/// and timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatus {
    /// Current lifecycle state.
    pub state: TaskState,

    /// Optional agent message accompanying this status (e.g. error details).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,

    /// ISO 8601 timestamp of when this status was set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl TaskStatus {
    /// Creates a [`TaskStatus`] with just a state.
    #[must_use]
    pub const fn new(state: TaskState) -> Self {
        Self {
            state,
            message: None,
            timestamp: None,
        }
    }
}

// ── Task ──────────────────────────────────────────────────────────────────────

/// A unit of work managed by an A2A agent.
///
/// The wire `kind` field (`"task"`) is injected by enclosing discriminated
/// unions such as [`crate::events::StreamResponse`] and
/// [`crate::responses::SendMessageResponse`]. Standalone `Task` values received
/// over the wire may include `kind`; serde silently tolerates unknown fields, so
/// no action is needed on the receiving side.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Unique task identifier.
    pub id: TaskId,

    /// Conversation context this task belongs to.
    pub context_id: ContextId,

    /// Current status of the task.
    pub status: TaskStatus,

    /// Historical messages exchanged during this task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Message>>,

    /// Artifacts produced by this task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_task() -> Task {
        Task {
            id: TaskId::new("task-1"),
            context_id: ContextId::new("ctx-1"),
            status: TaskStatus::new(TaskState::Working),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    #[test]
    fn task_state_screaming_snake_case() {
        assert_eq!(
            serde_json::to_string(&TaskState::InputRequired).expect("ser"),
            "\"TASK_STATE_INPUT_REQUIRED\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::AuthRequired).expect("ser"),
            "\"TASK_STATE_AUTH_REQUIRED\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::Pending).expect("ser"),
            "\"TASK_STATE_PENDING\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::Unspecified).expect("ser"),
            "\"TASK_STATE_UNSPECIFIED\""
        );
    }

    #[test]
    fn task_state_is_terminal() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
        assert!(TaskState::Rejected.is_terminal());
        assert!(!TaskState::Working.is_terminal());
        assert!(!TaskState::Pending.is_terminal());
    }

    #[test]
    fn task_roundtrip() {
        let task = make_task();
        let json = serde_json::to_string(&task).expect("serialize");
        assert!(json.contains("\"id\":\"task-1\""));

        let back: Task = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, TaskId::new("task-1"));
        assert_eq!(back.context_id, ContextId::new("ctx-1"));
        assert_eq!(back.status.state, TaskState::Working);
    }

    #[test]
    fn optional_fields_omitted() {
        let task = make_task();
        let json = serde_json::to_string(&task).expect("serialize");
        assert!(!json.contains("\"history\""), "history should be omitted");
        assert!(
            !json.contains("\"artifacts\""),
            "artifacts should be omitted"
        );
        assert!(!json.contains("\"metadata\""), "metadata should be omitted");
    }

    #[test]
    fn task_version_ordering() {
        assert!(TaskVersion::new(2) > TaskVersion::new(1));
        assert_eq!(TaskVersion::new(5).get(), 5);
    }
}
