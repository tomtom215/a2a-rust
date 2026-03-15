// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC method parameter types.
//!
//! Each A2A 0.3.0 method has a corresponding `Params` struct that maps to the
//! `params` field of a [`crate::jsonrpc::JsonRpcRequest`].
//!
//! | Method | Params type |
//! |---|---|
//! | `message/send` | [`MessageSendParams`] |
//! | `message/stream` | [`MessageSendParams`] |
//! | `tasks/get` | [`TaskQueryParams`] |
//! | `tasks/cancel` | [`TaskIdParams`] |
//! | `tasks/list` | [`ListTasksParams`] |
//! | `tasks/resubscribe` | [`TaskIdParams`] |
//! | `tasks/pushNotificationConfig/set` | [`crate::push::TaskPushNotificationConfig`] |
//! | `tasks/pushNotificationConfig/get` | [`GetPushConfigParams`] |
//! | `tasks/pushNotificationConfig/delete` | [`DeletePushConfigParams`] |

use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::push::TaskPushNotificationConfig;
use crate::task::{ContextId, TaskId, TaskState};

// ── SendMessageConfiguration ──────────────────────────────────────────────────

/// Optional configuration for a `message/send` or `message/stream` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageConfiguration {
    /// MIME types the client can accept as output (e.g. `["text/plain"]`).
    pub accepted_output_modes: Vec<String>,

    /// Push notification config to register alongside this message send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_push_notification_config: Option<TaskPushNotificationConfig>,

    /// Number of historical messages to include in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,

    /// If `true`, return immediately with the task object rather than waiting
    /// for completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_immediately: Option<bool>,
}

// ── MessageSendParams ─────────────────────────────────────────────────────────

/// Parameters for the `message/send` and `message/stream` methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendParams {
    /// The message to send to the agent.
    pub message: Message,

    /// Optional send configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<SendMessageConfiguration>,

    /// Arbitrary caller metadata attached to the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── TaskQueryParams ───────────────────────────────────────────────────────────

/// Parameters for the `tasks/get` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueryParams {
    /// ID of the task to retrieve.
    pub id: TaskId,

    /// Number of historical messages to include in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
}

// ── TaskIdParams ──────────────────────────────────────────────────────────────

/// Minimal parameters identifying a single task by ID.
///
/// Used for `tasks/cancel` and `tasks/resubscribe`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdParams {
    /// ID of the target task.
    pub id: TaskId,
}

// ── ListTasksParams ───────────────────────────────────────────────────────────

/// Parameters for the `tasks/list` method.
///
/// All fields are optional filters; omitting them returns all tasks visible to
/// the caller (subject to the server's default page size).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksParams {
    /// Filter to tasks belonging to this conversation context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<ContextId>,

    /// Filter to tasks in this state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<TaskState>,

    /// Maximum number of tasks to return per page (1–100, default 50).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,

    /// Pagination cursor returned by the previous response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,

    /// Return only tasks whose status changed after this ISO 8601 timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_timestamp_after: Option<String>,

    /// If `true`, include artifact data in the returned tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_artifacts: Option<bool>,
}

// ── GetPushConfigParams ───────────────────────────────────────────────────────

/// Parameters for the `tasks/pushNotificationConfig/get` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPushConfigParams {
    /// The task whose push config to retrieve.
    pub task_id: TaskId,

    /// The server-assigned push config identifier.
    pub id: String,
}

// ── DeletePushConfigParams ────────────────────────────────────────────────────

/// Parameters for the `tasks/pushNotificationConfig/delete` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePushConfigParams {
    /// The task whose push config to delete.
    pub task_id: TaskId,

    /// The server-assigned push config identifier.
    pub id: String,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MessageId, MessageRole, Part, TextPart};

    fn make_message() -> Message {
        Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::Text(TextPart::new("hello"))],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        }
    }

    #[test]
    fn message_send_params_roundtrip() {
        let params = MessageSendParams {
            message: make_message(),
            configuration: Some(SendMessageConfiguration {
                accepted_output_modes: vec!["text/plain".into()],
                task_push_notification_config: None,
                history_length: Some(10),
                return_immediately: None,
            }),
            metadata: None,
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: MessageSendParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.message.id, MessageId::new("msg-1"));
    }

    #[test]
    fn list_tasks_params_empty_roundtrip() {
        let params = ListTasksParams {
            context_id: None,
            status: None,
            page_size: None,
            page_token: None,
            status_timestamp_after: None,
            include_artifacts: None,
        };
        let json = serde_json::to_string(&params).expect("serialize");
        // All optional fields should be absent
        assert_eq!(json, "{}", "empty params should serialize to {{}}");
    }

    #[test]
    fn task_query_params_roundtrip() {
        let params = TaskQueryParams {
            id: TaskId::new("task-1"),
            history_length: Some(5),
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: TaskQueryParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, TaskId::new("task-1"));
        assert_eq!(back.history_length, Some(5));
    }
}
