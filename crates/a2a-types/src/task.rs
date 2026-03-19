// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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
///
/// IDs are compared as raw byte strings (via the derived [`PartialEq`] on
/// the inner `String`). No Unicode normalization is applied, so two IDs
/// that look identical but use different Unicode representations (e.g.
/// NFC vs. NFD) will be considered distinct.
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
    #[inline]
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
    #[inline]
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ── ContextId ─────────────────────────────────────────────────────────────────

/// Opaque unique identifier for a conversation context.
///
/// A context groups related tasks under a single logical conversation thread.
///
/// Like [`TaskId`], IDs are compared as raw byte strings without Unicode
/// normalization.
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
    #[inline]
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
    #[inline]
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
/// Serializes as lowercase kebab-case (e.g. `"completed"`, `"input-required"`).
/// Also accepts the legacy `TASK_STATE_*` format on deserialization for
/// backward compatibility.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskState {
    /// Proto default (0-value); should not appear in normal usage.
    #[serde(rename = "unspecified", alias = "TASK_STATE_UNSPECIFIED")]
    Unspecified,
    /// Task received, not yet started.
    #[serde(rename = "submitted", alias = "TASK_STATE_SUBMITTED")]
    Submitted,
    /// Task is actively being processed.
    #[serde(rename = "working", alias = "TASK_STATE_WORKING")]
    Working,
    /// Agent requires additional input from the client to proceed.
    #[serde(rename = "input-required", alias = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    /// Agent requires the client to complete an authentication step.
    #[serde(rename = "auth-required", alias = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
    /// Task finished successfully.
    #[serde(rename = "completed", alias = "TASK_STATE_COMPLETED")]
    Completed,
    /// Task finished with an error.
    #[serde(rename = "failed", alias = "TASK_STATE_FAILED")]
    Failed,
    /// Task was canceled by the client.
    #[serde(rename = "canceled", alias = "TASK_STATE_CANCELED")]
    Canceled,
    /// Task was rejected by the agent before execution.
    #[serde(rename = "rejected", alias = "TASK_STATE_REJECTED")]
    Rejected,
}

impl TaskState {
    /// Returns `true` if this state is a terminal (final) state.
    ///
    /// Terminal states: `Completed`, `Failed`, `Canceled`, `Rejected`.
    #[inline]
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Canceled | Self::Rejected
        )
    }

    /// Returns `true` if transitioning from `self` to `next` is a valid
    /// state transition per the A2A protocol.
    ///
    /// Terminal states cannot transition to any other state.
    /// `Unspecified` can transition to any state.
    #[inline]
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        // Terminal states are final — no transitions allowed.
        if self.is_terminal() {
            return false;
        }
        // Allow any transition from Unspecified (proto default).
        if matches!(self, Self::Unspecified) {
            return true;
        }
        matches!(
            (self, next),
            // Submitted → Working, Failed, Canceled, Rejected
            (Self::Submitted, Self::Working | Self::Failed | Self::Canceled | Self::Rejected)
            // Working → Completed, Failed, Canceled, InputRequired, AuthRequired
            | (Self::Working,
               Self::Completed | Self::Failed | Self::Canceled | Self::InputRequired | Self::AuthRequired)
            // InputRequired / AuthRequired → Working, Failed, Canceled
            | (Self::InputRequired | Self::AuthRequired,
               Self::Working | Self::Failed | Self::Canceled)
        )
    }
}

impl std::fmt::Display for TaskState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Unspecified => "unspecified",
            Self::Submitted => "submitted",
            Self::Working => "working",
            Self::InputRequired => "input-required",
            Self::AuthRequired => "auth-required",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Rejected => "rejected",
        };
        f.write_str(s)
    }
}

// ── TaskStatus ────────────────────────────────────────────────────────────────

/// The current status of a [`Task`], combining state with an optional message
/// and timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Creates a [`TaskStatus`] with just a state and no timestamp.
    ///
    /// Prefer [`TaskStatus::with_timestamp`] in production code so that
    /// status changes carry an ISO 8601 timestamp.
    #[must_use]
    pub const fn new(state: TaskState) -> Self {
        Self {
            state,
            message: None,
            timestamp: None,
        }
    }

    /// Creates a [`TaskStatus`] with a state and the current UTC timestamp.
    #[must_use]
    pub fn with_timestamp(state: TaskState) -> Self {
        Self {
            state,
            message: None,
            timestamp: Some(crate::utc_now_iso8601()),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    fn task_state_lowercase_serde() {
        assert_eq!(
            serde_json::to_string(&TaskState::InputRequired).expect("ser"),
            "\"input-required\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::AuthRequired).expect("ser"),
            "\"auth-required\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::Submitted).expect("ser"),
            "\"submitted\""
        );
        assert_eq!(
            serde_json::to_string(&TaskState::Unspecified).expect("ser"),
            "\"unspecified\""
        );
        // Legacy aliases still deserialize
        let back: TaskState = serde_json::from_str("\"TASK_STATE_COMPLETED\"").unwrap();
        assert_eq!(back, TaskState::Completed);
    }

    #[test]
    fn task_state_is_terminal() {
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
        assert!(TaskState::Rejected.is_terminal());
        assert!(!TaskState::Working.is_terminal());
        assert!(!TaskState::Submitted.is_terminal());
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

    #[test]
    fn wire_format_submitted_state() {
        let json = serde_json::to_string(&TaskState::Submitted).unwrap();
        assert_eq!(json, "\"submitted\"");

        // Both formats deserialize
        let back: TaskState = serde_json::from_str("\"submitted\"").unwrap();
        assert_eq!(back, TaskState::Submitted);
        let back: TaskState = serde_json::from_str("\"TASK_STATE_SUBMITTED\"").unwrap();
        assert_eq!(back, TaskState::Submitted);
    }

    #[test]
    fn task_version_serde_roundtrip() {
        let v = TaskVersion::new(42);
        let json = serde_json::to_string(&v).expect("serialize");
        assert_eq!(json, "42");

        let back: TaskVersion = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, TaskVersion::new(42));

        // Also test zero
        let v0 = TaskVersion::new(0);
        let json0 = serde_json::to_string(&v0).expect("serialize zero");
        assert_eq!(json0, "0");
        let back0: TaskVersion = serde_json::from_str(&json0).expect("deserialize zero");
        assert_eq!(back0, TaskVersion::new(0));

        // And u64::MAX
        let vmax = TaskVersion::new(u64::MAX);
        let json_max = serde_json::to_string(&vmax).expect("serialize max");
        let back_max: TaskVersion = serde_json::from_str(&json_max).expect("deserialize max");
        assert_eq!(back_max, vmax);
    }

    #[test]
    fn empty_string_ids_work() {
        let tid = TaskId::new("");
        let json = serde_json::to_string(&tid).expect("serialize empty TaskId");
        assert_eq!(json, "\"\"");
        let back: TaskId = serde_json::from_str(&json).expect("deserialize empty TaskId");
        assert_eq!(back, TaskId::new(""));

        let cid = ContextId::new("");
        let json = serde_json::to_string(&cid).expect("serialize empty ContextId");
        assert_eq!(json, "\"\"");
        let back: ContextId = serde_json::from_str(&json).expect("deserialize empty ContextId");
        assert_eq!(back, ContextId::new(""));

        // A task with empty IDs should still roundtrip.
        let task = Task {
            id: TaskId::new(""),
            context_id: ContextId::new(""),
            status: TaskStatus::new(TaskState::Submitted),
            history: None,
            artifacts: None,
            metadata: None,
        };
        let json = serde_json::to_string(&task).expect("serialize task with empty ids");
        let back: Task = serde_json::from_str(&json).expect("deserialize task with empty ids");
        assert_eq!(back.id, TaskId::new(""));
        assert_eq!(back.context_id, ContextId::new(""));
    }

    #[test]
    fn task_state_display_trait() {
        assert_eq!(TaskState::Working.to_string(), "working");
        assert_eq!(TaskState::Completed.to_string(), "completed");
        assert_eq!(TaskState::Failed.to_string(), "failed");
        assert_eq!(TaskState::Canceled.to_string(), "canceled");
        assert_eq!(TaskState::Rejected.to_string(), "rejected");
        assert_eq!(TaskState::Submitted.to_string(), "submitted");
        assert_eq!(TaskState::InputRequired.to_string(), "input-required");
        assert_eq!(TaskState::AuthRequired.to_string(), "auth-required");
        assert_eq!(TaskState::Unspecified.to_string(), "unspecified");
    }

    // ── is_terminal exhaustive ────────────────────────────────────────────

    #[test]
    fn is_terminal_all_variants() {
        assert!(!TaskState::Unspecified.is_terminal());
        assert!(!TaskState::Submitted.is_terminal());
        assert!(!TaskState::Working.is_terminal());
        assert!(!TaskState::InputRequired.is_terminal());
        assert!(!TaskState::AuthRequired.is_terminal());
        assert!(TaskState::Completed.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        assert!(TaskState::Canceled.is_terminal());
        assert!(TaskState::Rejected.is_terminal());
    }

    // ── can_transition_to exhaustive ──────────────────────────────────────

    /// All valid transitions per A2A protocol spec.
    #[test]
    fn can_transition_to_valid_transitions() {
        use TaskState::*;

        // Unspecified → anything is valid
        for &target in &[
            Unspecified,
            Submitted,
            Working,
            InputRequired,
            AuthRequired,
            Completed,
            Failed,
            Canceled,
            Rejected,
        ] {
            assert!(
                Unspecified.can_transition_to(target),
                "Unspecified → {target:?} should be valid"
            );
        }

        // Submitted → Working, Failed, Canceled, Rejected
        assert!(Submitted.can_transition_to(Working));
        assert!(Submitted.can_transition_to(Failed));
        assert!(Submitted.can_transition_to(Canceled));
        assert!(Submitted.can_transition_to(Rejected));

        // Working → Completed, Failed, Canceled, InputRequired, AuthRequired
        assert!(Working.can_transition_to(Completed));
        assert!(Working.can_transition_to(Failed));
        assert!(Working.can_transition_to(Canceled));
        assert!(Working.can_transition_to(InputRequired));
        assert!(Working.can_transition_to(AuthRequired));

        // InputRequired → Working, Failed, Canceled
        assert!(InputRequired.can_transition_to(Working));
        assert!(InputRequired.can_transition_to(Failed));
        assert!(InputRequired.can_transition_to(Canceled));

        // AuthRequired → Working, Failed, Canceled
        assert!(AuthRequired.can_transition_to(Working));
        assert!(AuthRequired.can_transition_to(Failed));
        assert!(AuthRequired.can_transition_to(Canceled));
    }

    /// All invalid transitions per A2A protocol spec.
    #[test]
    fn can_transition_to_invalid_transitions() {
        use TaskState::*;

        // Terminal states cannot transition anywhere (including to themselves)
        for &terminal in &[Completed, Failed, Canceled, Rejected] {
            for &target in &[
                Unspecified,
                Submitted,
                Working,
                InputRequired,
                AuthRequired,
                Completed,
                Failed,
                Canceled,
                Rejected,
            ] {
                assert!(
                    !terminal.can_transition_to(target),
                    "{terminal:?} → {target:?} should be invalid (terminal state)"
                );
            }
        }

        // Submitted cannot go to Completed, InputRequired, AuthRequired, Submitted, Unspecified
        assert!(!Submitted.can_transition_to(Completed));
        assert!(!Submitted.can_transition_to(InputRequired));
        assert!(!Submitted.can_transition_to(AuthRequired));
        assert!(!Submitted.can_transition_to(Submitted));
        assert!(!Submitted.can_transition_to(Unspecified));

        // Working cannot go to Submitted, Working, Unspecified, Rejected
        assert!(!Working.can_transition_to(Submitted));
        assert!(!Working.can_transition_to(Working));
        assert!(!Working.can_transition_to(Unspecified));
        assert!(!Working.can_transition_to(Rejected));

        // InputRequired cannot go to Completed, Submitted, InputRequired, AuthRequired, Unspecified, Rejected
        assert!(!InputRequired.can_transition_to(Completed));
        assert!(!InputRequired.can_transition_to(Submitted));
        assert!(!InputRequired.can_transition_to(InputRequired));
        assert!(!InputRequired.can_transition_to(AuthRequired));
        assert!(!InputRequired.can_transition_to(Unspecified));
        assert!(!InputRequired.can_transition_to(Rejected));

        // AuthRequired cannot go to Completed, Submitted, InputRequired, AuthRequired, Unspecified, Rejected
        assert!(!AuthRequired.can_transition_to(Completed));
        assert!(!AuthRequired.can_transition_to(Submitted));
        assert!(!AuthRequired.can_transition_to(InputRequired));
        assert!(!AuthRequired.can_transition_to(AuthRequired));
        assert!(!AuthRequired.can_transition_to(Unspecified));
        assert!(!AuthRequired.can_transition_to(Rejected));
    }

    // ── Newtype coverage ──────────────────────────────────────────────────

    #[test]
    fn task_id_display_and_as_ref() {
        let id = TaskId::new("abc");
        assert_eq!(id.to_string(), "abc");
        assert_eq!(id.as_ref(), "abc");
    }

    #[test]
    fn task_id_from_impls() {
        let from_str: TaskId = "hello".into();
        assert_eq!(from_str, TaskId::new("hello"));

        let from_string: TaskId = String::from("world").into();
        assert_eq!(from_string, TaskId::new("world"));
    }

    #[test]
    fn context_id_display_and_as_ref() {
        let id = ContextId::new("ctx");
        assert_eq!(id.to_string(), "ctx");
        assert_eq!(id.as_ref(), "ctx");
    }

    #[test]
    fn context_id_from_impls() {
        let from_str: ContextId = "c1".into();
        assert_eq!(from_str, ContextId::new("c1"));

        let from_string: ContextId = String::from("c2").into();
        assert_eq!(from_string, ContextId::new("c2"));
    }

    #[test]
    fn task_version_display() {
        assert_eq!(TaskVersion::new(42).to_string(), "42");
        assert_eq!(TaskVersion::new(0).to_string(), "0");
    }

    #[test]
    fn task_version_from_u64() {
        let v: TaskVersion = 99u64.into();
        assert_eq!(v.get(), 99);
    }

    #[test]
    fn task_status_with_timestamp_has_timestamp() {
        let status = TaskStatus::with_timestamp(TaskState::Working);
        assert!(
            status.timestamp.is_some(),
            "with_timestamp should set timestamp"
        );
        assert!(status.message.is_none());
        assert_eq!(status.state, TaskState::Working);
    }

    #[test]
    fn task_status_new_has_no_timestamp() {
        let status = TaskStatus::new(TaskState::Submitted);
        assert!(status.timestamp.is_none());
        assert!(status.message.is_none());
        assert_eq!(status.state, TaskState::Submitted);
    }
}
