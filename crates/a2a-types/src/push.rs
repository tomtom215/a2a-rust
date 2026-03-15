// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration types.
//!
//! Push notifications allow an agent to deliver task updates to a client-owned
//! HTTPS webhook endpoint rather than requiring the client to poll. A client
//! registers a [`TaskPushNotificationConfig`] for a specific task via the
//! `CreateTaskPushNotificationConfig` method.

use serde::{Deserialize, Serialize};

// ── AuthenticationInfo ──────────────────────────────────────────────────────

/// Authentication information used by an agent when calling a push webhook.
///
/// In v1.0, this uses singular `scheme` (not `schemes`) and required
/// `credentials`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationInfo {
    /// Authentication scheme (e.g. `"bearer"`).
    pub scheme: String,

    /// Credential value (e.g. a static token).
    pub credentials: String,
}

// ── TaskPushNotificationConfig ──────────────────────────────────────────────

/// Configuration for delivering task updates to a webhook endpoint.
///
/// In v1.0, this is a single flat type combining the previous
/// `PushNotificationConfig` and `TaskPushNotificationConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
    /// Optional tenant identifier for multi-tenancy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,

    /// Server-assigned configuration identifier.
    ///
    /// Absent when first creating the config; populated in the server response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// The task for which push notifications are configured.
    pub task_id: String,

    /// HTTPS URL of the client's webhook endpoint.
    pub url: String,

    /// Optional shared secret for request verification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Authentication details the agent should use when calling the webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthenticationInfo>,
}

impl TaskPushNotificationConfig {
    /// Creates a minimal config with a task ID and URL.
    #[must_use]
    pub fn new(task_id: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            tenant: None,
            id: None,
            task_id: task_id.into(),
            url: url.into(),
            token: None,
            authentication: None,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_config_minimal_roundtrip() {
        let cfg = TaskPushNotificationConfig::new("task-1", "https://example.com/webhook");
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(json.contains("\"url\""));
        assert!(json.contains("\"taskId\""));
        assert!(!json.contains("\"id\""), "id should be omitted when None");

        let back: TaskPushNotificationConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.url, "https://example.com/webhook");
        assert_eq!(back.task_id, "task-1");
    }

    #[test]
    fn push_config_full_roundtrip() {
        let cfg = TaskPushNotificationConfig {
            tenant: Some("tenant-1".into()),
            id: Some("cfg-1".into()),
            task_id: "task-1".into(),
            url: "https://example.com/webhook".into(),
            token: Some("secret".into()),
            authentication: Some(AuthenticationInfo {
                scheme: "bearer".into(),
                credentials: "my-token".into(),
            }),
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: TaskPushNotificationConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.task_id, "task-1");
        assert_eq!(back.url, "https://example.com/webhook");
        assert_eq!(back.authentication.unwrap().scheme, "bearer");
    }
}
