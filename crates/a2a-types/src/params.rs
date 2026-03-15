// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC method parameter types.
//!
//! Each A2A v1.0 method has a corresponding `Params` struct that maps to the
//! `params` field of a [`crate::jsonrpc::JsonRpcRequest`].
//!
//! | Method | Params type |
//! |---|---|
//! | `SendMessage` | [`MessageSendParams`] |
//! | `SendStreamingMessage` | [`MessageSendParams`] |
//! | `GetTask` | [`TaskQueryParams`] |
//! | `CancelTask` | [`CancelTaskParams`] |
//! | `ListTasks` | [`ListTasksParams`] |
//! | `SubscribeToTask` | [`TaskIdParams`] |
//! | `CreateTaskPushNotificationConfig` | [`crate::push::TaskPushNotificationConfig`] |
//! | `GetTaskPushNotificationConfig` | [`GetPushConfigParams`] |
//! | `DeleteTaskPushNotificationConfig` | [`DeletePushConfigParams`] |

use serde::{Deserialize, Serialize};

use crate::message::Message;
use crate::push::TaskPushNotificationConfig;
use crate::task::TaskState;

// ── SendMessageConfiguration ──────────────────────────────────────────────────

/// Optional configuration for a `SendMessage` or `SendStreamingMessage` call.
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

/// Parameters for the `SendMessage` and `SendStreamingMessage` methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSendParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

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

/// Parameters for the `GetTask` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskQueryParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// ID of the task to retrieve.
    pub id: String,

    /// Number of historical messages to include in the response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_length: Option<u32>,
}

// ── TaskIdParams ──────────────────────────────────────────────────────────────

/// Minimal parameters identifying a single task by ID.
///
/// Used for `SubscribeToTask`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// ID of the target task.
    pub id: String,
}

// ── CancelTaskParams ────────────────────────────────────────────────────────

/// Parameters for the `CancelTask` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelTaskParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// ID of the task to cancel.
    pub id: String,

    /// Arbitrary metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ── ListTasksParams ───────────────────────────────────────────────────────────

/// Parameters for the `ListTasks` method.
///
/// All fields are optional filters; omitting them returns all tasks visible to
/// the caller (subject to the server's default page size).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListTasksParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// Filter to tasks belonging to this conversation context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,

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

/// Parameters for the `GetTaskPushNotificationConfig` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPushConfigParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// The task whose push config to retrieve.
    pub task_id: String,

    /// The server-assigned push config identifier.
    pub id: String,
}

// ── DeletePushConfigParams ────────────────────────────────────────────────────

/// Parameters for the `DeleteTaskPushNotificationConfig` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePushConfigParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// The task whose push config to delete.
    pub task_id: String,

    /// The server-assigned push config identifier.
    pub id: String,
}

// ── GetExtendedAgentCardParams ──────────────────────────────────────────────

/// Parameters for the `GetExtendedAgentCard` method.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetExtendedAgentCardParams {
    /// Optional tenant for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{MessageId, MessageRole, Part};

    fn make_message() -> Message {
        Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text("hello")],
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
            tenant: None,
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
            tenant: None,
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
            tenant: None,
            id: "task-1".into(),
            history_length: Some(5),
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: TaskQueryParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, "task-1");
        assert_eq!(back.history_length, Some(5));
    }

    #[test]
    fn cancel_task_params_roundtrip() {
        let params = CancelTaskParams {
            tenant: Some("my-tenant".into()),
            id: "task-1".into(),
            metadata: Some(serde_json::json!({"reason": "no longer needed"})),
        };
        let json = serde_json::to_string(&params).expect("serialize");
        let back: CancelTaskParams = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, "task-1");
        assert_eq!(back.tenant.as_deref(), Some("my-tenant"));
        assert!(back.metadata.is_some());
    }
}
