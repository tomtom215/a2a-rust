// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification configuration types.
//!
//! Push notifications allow an agent to deliver task updates to a client-owned
//! HTTPS webhook endpoint rather than requiring the client to poll. A client
//! registers a [`PushNotificationConfig`] for a specific task via the
//! `tasks/pushNotificationConfig/set` method.

use serde::{Deserialize, Serialize};

use crate::task::TaskId;

// ── PushNotificationAuthInfo ──────────────────────────────────────────────────

/// Authentication information used by an agent when calling a push webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotificationAuthInfo {
    /// Supported authentication schemes (e.g. `["bearer"]`).
    pub schemes: Vec<String>,

    /// Optional pre-shared credential value (e.g. a static token).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<String>,
}

// ── PushNotificationConfig ────────────────────────────────────────────────────

/// Configuration for delivering task updates to a webhook endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PushNotificationConfig {
    /// Server-assigned configuration identifier.
    ///
    /// Absent when first creating the config; populated in the server response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// HTTPS URL of the client's webhook endpoint.
    pub url: String,

    /// Optional shared secret for request verification (HMAC or similar).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Authentication details the agent should use when calling the webhook.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<PushNotificationAuthInfo>,
}

impl PushNotificationConfig {
    /// Creates a minimal [`PushNotificationConfig`] with only a URL.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            id: None,
            url: url.into(),
            token: None,
            authentication: None,
        }
    }
}

// ── TaskPushNotificationConfig ────────────────────────────────────────────────

/// Associates a [`PushNotificationConfig`] with a specific task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPushNotificationConfig {
    /// The task for which push notifications are configured.
    pub task_id: TaskId,

    /// The push notification configuration.
    pub push_notification_config: PushNotificationConfig,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_config_minimal_roundtrip() {
        let cfg = PushNotificationConfig::new("https://example.com/webhook");
        let json = serde_json::to_string(&cfg).expect("serialize");
        assert!(json.contains("\"url\""));
        assert!(!json.contains("\"id\""), "id should be omitted when None");

        let back: PushNotificationConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.url, "https://example.com/webhook");
    }

    #[test]
    fn task_push_config_roundtrip() {
        let cfg = TaskPushNotificationConfig {
            task_id: TaskId::new("task-1"),
            push_notification_config: PushNotificationConfig {
                id: Some("cfg-1".into()),
                url: "https://example.com/webhook".into(),
                token: Some("secret".into()),
                authentication: Some(PushNotificationAuthInfo {
                    schemes: vec!["bearer".into()],
                    credentials: None,
                }),
            },
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: TaskPushNotificationConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.task_id, TaskId::new("task-1"));
        assert_eq!(
            back.push_notification_config.url,
            "https://example.com/webhook"
        );
    }
}
