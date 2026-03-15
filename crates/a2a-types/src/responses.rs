// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! RPC method response types.
//!
//! These types appear as the `result` field of a
//! [`crate::jsonrpc::JsonRpcSuccessResponse`].
//!
//! | Method | Response type |
//! |---|---|
//! | `message/send` | [`SendMessageResponse`] |
//! | `tasks/list` | [`TaskListResponse`] |
//! | `agent/authenticatedExtendedCard` | [`AgentCard`] (re-exported as [`AuthenticatedExtendedCardResponse`]) |

use serde::{Deserialize, Serialize};

use crate::agent_card::AgentCard;
use crate::message::Message;
use crate::task::Task;

// ── SendMessageResponse ───────────────────────────────────────────────────────

/// The result of a `message/send` call: either a completed [`Task`] or an
/// immediate [`Message`] response.
///
/// Discriminated on the `"kind"` field: `"task"` or `"message"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SendMessageResponse {
    /// The agent accepted the message and created (or updated) a task.
    Task(Task),

    /// The agent responded immediately with a message (no task created).
    Message(Message),
}

// ── TaskListResponse ──────────────────────────────────────────────────────────

/// The result of a `tasks/list` call: a page of tasks with an optional
/// continuation token.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskListResponse {
    /// The tasks in this page of results.
    pub tasks: Vec<Task>,

    /// Pagination token for the next page; absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

impl TaskListResponse {
    /// Creates a single-page response with no next-page token.
    #[must_use]
    pub const fn new(tasks: Vec<Task>) -> Self {
        Self {
            tasks,
            next_page_token: None,
        }
    }
}

// ── AuthenticatedExtendedCardResponse ─────────────────────────────────────────

/// The full (private) agent card returned by `agent/authenticatedExtendedCard`.
///
/// This is structurally identical to the public [`AgentCard`]; the type alias
/// signals intent and may gain additional fields in a future spec revision.
pub type AuthenticatedExtendedCardResponse = AgentCard;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MessageId, MessageRole, Part, TextPart};
    use crate::task::{ContextId, TaskId, TaskState, TaskStatus};

    fn make_task() -> Task {
        Task {
            id: TaskId::new("t1"),
            context_id: ContextId::new("c1"),
            status: TaskStatus::new(TaskState::Completed),
            history: None,
            artifacts: None,
            metadata: None,
        }
    }

    fn make_message() -> Message {
        Message {
            id: MessageId::new("m1"),
            role: MessageRole::Agent,
            parts: vec![Part::Text(TextPart::new("hi"))],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        }
    }

    #[test]
    fn send_message_response_task_variant() {
        let resp = SendMessageResponse::Task(make_task());
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"kind\":\"task\""));

        let back: SendMessageResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, SendMessageResponse::Task(_)));
    }

    #[test]
    fn send_message_response_message_variant() {
        let resp = SendMessageResponse::Message(make_message());
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"kind\":\"message\""));

        let back: SendMessageResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(back, SendMessageResponse::Message(_)));
    }

    #[test]
    fn task_list_response_roundtrip() {
        let resp = TaskListResponse {
            tasks: vec![make_task()],
            next_page_token: Some("cursor-abc".into()),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"nextPageToken\":\"cursor-abc\""));

        let back: TaskListResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.tasks.len(), 1);
        assert_eq!(back.next_page_token.as_deref(), Some("cursor-abc"));
    }

    #[test]
    fn task_list_response_no_token_omitted() {
        let resp = TaskListResponse::new(vec![]);
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            !json.contains("\"nextPageToken\""),
            "token should be absent: {json}"
        );
    }
}
