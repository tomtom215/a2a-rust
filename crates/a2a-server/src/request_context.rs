// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Request context passed to the [`AgentExecutor`](crate::AgentExecutor).
//!
//! [`RequestContext`] bundles together the incoming message, task identifiers,
//! and any previously stored task snapshot so that the executor has all the
//! information it needs to process a request.

use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::{Task, TaskId};
use tokio_util::sync::CancellationToken;

/// Context for a single agent execution request.
///
/// Built by the [`RequestHandler`](crate::RequestHandler) and passed to
/// [`AgentExecutor::execute`](crate::AgentExecutor::execute).
///
/// The [`cancellation_token`](Self::cancellation_token) allows executors to
/// observe cancellation requests and abort work cooperatively.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// The incoming user message.
    pub message: Message,

    /// The task identifier for this execution.
    pub task_id: TaskId,

    /// The conversation context identifier.
    pub context_id: String,

    /// The previously stored task snapshot, if this is a continuation.
    pub stored_task: Option<Task>,

    /// Arbitrary metadata from the request.
    pub metadata: Option<serde_json::Value>,

    /// Cancellation token for cooperative task cancellation.
    ///
    /// Executors should check [`CancellationToken::is_cancelled`] or
    /// `.cancelled().await` to stop work when the task is cancelled.
    pub cancellation_token: CancellationToken,
}

impl RequestContext {
    /// Creates a new [`RequestContext`].
    #[must_use]
    pub fn new(message: Message, task_id: TaskId, context_id: String) -> Self {
        Self {
            message,
            task_id,
            context_id,
            stored_task: None,
            metadata: None,
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Sets the stored task snapshot for continuation requests.
    #[must_use]
    pub fn with_stored_task(mut self, task: Task) -> Self {
        self.stored_task = Some(task);
        self
    }

    /// Sets request metadata.
    #[must_use]
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a2a_protocol_types::message::{MessageId, MessageRole, Part};
    use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

    /// Helper: creates a minimal user message.
    fn make_message(text: &str) -> Message {
        Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        }
    }

    /// Helper: creates a minimal task.
    fn make_task() -> Task {
        Task {
            id: TaskId::new("task-1"),
            context_id: ContextId::new("ctx-1"),
            status: TaskStatus::new(TaskState::Submitted),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    // ── new ────────────────────────────────────────────────────────────────

    #[test]
    fn new_sets_required_fields() {
        let msg = make_message("hello");
        let ctx = RequestContext::new(msg.clone(), TaskId::new("t-1"), "ctx-1".to_owned());

        assert_eq!(ctx.message, msg, "message should match the input");
        assert_eq!(ctx.task_id, TaskId::new("t-1"), "task_id should match");
        assert_eq!(ctx.context_id, "ctx-1", "context_id should match");
    }

    #[test]
    fn new_defaults_optional_fields_to_none() {
        let ctx = RequestContext::new(make_message("hi"), TaskId::new("t-2"), "ctx-2".to_owned());

        assert!(
            ctx.stored_task.is_none(),
            "stored_task should default to None"
        );
        assert!(ctx.metadata.is_none(), "metadata should default to None");
    }

    #[test]
    fn new_provides_uncancelled_token() {
        let ctx = RequestContext::new(make_message("hi"), TaskId::new("t-3"), "ctx-3".to_owned());
        assert!(
            !ctx.cancellation_token.is_cancelled(),
            "fresh token should not be cancelled"
        );
    }

    // ── with_stored_task ───────────────────────────────────────────────────

    #[test]
    fn with_stored_task_sets_task() {
        let task = make_task();
        let ctx = RequestContext::new(make_message("hi"), TaskId::new("t-4"), "ctx-4".to_owned())
            .with_stored_task(task.clone());

        assert_eq!(
            ctx.stored_task.as_ref().map(|t| &t.id),
            Some(&TaskId::new("task-1")),
            "stored_task should contain the provided task"
        );
    }

    #[test]
    fn with_stored_task_preserves_other_fields() {
        let ctx = RequestContext::new(make_message("hi"), TaskId::new("t-5"), "ctx-5".to_owned())
            .with_stored_task(make_task());

        assert_eq!(
            ctx.task_id,
            TaskId::new("t-5"),
            "task_id should be unchanged"
        );
        assert_eq!(ctx.context_id, "ctx-5", "context_id should be unchanged");
    }

    // ── with_metadata ──────────────────────────────────────────────────────

    #[test]
    fn with_metadata_sets_value() {
        let meta = serde_json::json!({"key": "value", "num": 42});
        let ctx = RequestContext::new(make_message("hi"), TaskId::new("t-6"), "ctx-6".to_owned())
            .with_metadata(meta.clone());

        assert_eq!(
            ctx.metadata.as_ref(),
            Some(&meta),
            "metadata should match the provided value"
        );
    }

    // ── builder chaining ───────────────────────────────────────────────────

    #[test]
    fn builder_methods_can_be_chained() {
        let task = make_task();
        let meta = serde_json::json!({"chained": true});
        let ctx = RequestContext::new(
            make_message("chain"),
            TaskId::new("t-7"),
            "ctx-7".to_owned(),
        )
        .with_stored_task(task.clone())
        .with_metadata(meta.clone());

        assert!(
            ctx.stored_task.is_some(),
            "stored_task should be set after chaining"
        );
        assert_eq!(
            ctx.metadata,
            Some(meta),
            "metadata should be set after chaining"
        );
    }

    // ── Clone / Debug ──────────────────────────────────────────────────────

    #[test]
    fn request_context_is_cloneable() {
        let ctx = RequestContext::new(
            make_message("clone me"),
            TaskId::new("t-8"),
            "ctx-8".to_owned(),
        );
        let cloned = ctx.clone();
        assert_eq!(
            cloned.task_id, ctx.task_id,
            "cloned context should have same task_id"
        );
    }

    #[test]
    fn request_context_is_debug() {
        let ctx = RequestContext::new(
            make_message("debug"),
            TaskId::new("t-9"),
            "ctx-9".to_owned(),
        );
        let debug_str = format!("{ctx:?}");
        assert!(
            debug_str.contains("RequestContext"),
            "Debug output should contain the struct name"
        );
    }
}
