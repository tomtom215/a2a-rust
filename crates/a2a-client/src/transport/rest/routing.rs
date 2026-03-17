// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! HTTP method routing for the REST transport.
//!
//! Maps A2A method names to [`Route`] descriptors containing the HTTP verb,
//! path template, and path parameter names.

// ── Route ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HttpMethod {
    Get,
    Post,
    Delete,
}

#[derive(Debug)]
pub(super) struct Route {
    pub(super) http_method: HttpMethod,
    pub(super) path_template: &'static str,
    /// Names of params that are path parameters (extracted from JSON params).
    pub(super) path_params: &'static [&'static str],
    /// Whether the response is SSE (used in tests).
    #[allow(dead_code)]
    pub(super) streaming: bool,
}

// ── Method routing ────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub(super) fn route_for(method: &str) -> Option<Route> {
    match method {
        "SendMessage" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/message:send",
            path_params: &[],
            streaming: false,
        }),
        "SendStreamingMessage" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/message:stream",
            path_params: &[],
            streaming: true,
        }),
        "GetTask" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks/{id}",
            path_params: &["id"],
            streaming: false,
        }),
        "CancelTask" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/tasks/{id}:cancel",
            path_params: &["id"],
            streaming: false,
        }),
        "ListTasks" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks",
            path_params: &[],
            streaming: false,
        }),
        "SubscribeToTask" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/tasks/{id}:subscribe",
            path_params: &["id"],
            streaming: true,
        }),
        "CreateTaskPushNotificationConfig" => Some(Route {
            http_method: HttpMethod::Post,
            path_template: "/tasks/{taskId}/pushNotificationConfigs",
            path_params: &["taskId"],
            streaming: false,
        }),
        "GetTaskPushNotificationConfig" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks/{taskId}/pushNotificationConfigs/{id}",
            path_params: &["taskId", "id"],
            streaming: false,
        }),
        "ListTaskPushNotificationConfigs" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/tasks/{taskId}/pushNotificationConfigs",
            path_params: &["taskId"],
            streaming: false,
        }),
        "DeleteTaskPushNotificationConfig" => Some(Route {
            http_method: HttpMethod::Delete,
            path_template: "/tasks/{taskId}/pushNotificationConfigs/{id}",
            path_params: &["taskId", "id"],
            streaming: false,
        }),
        "GetExtendedAgentCard" => Some(Route {
            http_method: HttpMethod::Get,
            path_template: "/extendedAgentCard",
            path_params: &[],
            streaming: false,
        }),
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_for_known_methods() {
        assert!(route_for("SendMessage").is_some());
        assert!(route_for("GetTask").is_some());
        assert!(route_for("ListTasks").is_some());
        assert!(route_for("SendStreamingMessage").is_some_and(|r| r.streaming));
    }

    #[test]
    fn route_for_unknown_method_returns_none() {
        assert!(route_for("unknown/method").is_none());
    }

    // ── Mutation-killing tests for route_for arms ─────────────────────────

    #[test]
    fn route_for_cancel_task() {
        let r = route_for("CancelTask").expect("CancelTask should have a route");
        assert_eq!(r.http_method, HttpMethod::Post);
        assert_eq!(r.path_template, "/tasks/{id}:cancel");
        assert_eq!(r.path_params, &["id"]);
        assert!(!r.streaming);
    }

    #[test]
    fn route_for_subscribe_to_task() {
        let r = route_for("SubscribeToTask").expect("SubscribeToTask should have a route");
        assert_eq!(r.http_method, HttpMethod::Post);
        assert_eq!(r.path_template, "/tasks/{id}:subscribe");
        assert_eq!(r.path_params, &["id"]);
        assert!(r.streaming);
    }

    #[test]
    fn route_for_create_task_push_notification_config() {
        let r = route_for("CreateTaskPushNotificationConfig")
            .expect("CreateTaskPushNotificationConfig should have a route");
        assert_eq!(r.http_method, HttpMethod::Post);
        assert_eq!(r.path_template, "/tasks/{taskId}/pushNotificationConfigs");
        assert_eq!(r.path_params, &["taskId"]);
        assert!(!r.streaming);
    }

    #[test]
    fn route_for_get_task_push_notification_config() {
        let r = route_for("GetTaskPushNotificationConfig")
            .expect("GetTaskPushNotificationConfig should have a route");
        assert_eq!(r.http_method, HttpMethod::Get);
        assert_eq!(
            r.path_template,
            "/tasks/{taskId}/pushNotificationConfigs/{id}"
        );
        assert_eq!(r.path_params, &["taskId", "id"]);
        assert!(!r.streaming);
    }

    #[test]
    fn route_for_list_task_push_notification_configs() {
        let r = route_for("ListTaskPushNotificationConfigs")
            .expect("ListTaskPushNotificationConfigs should have a route");
        assert_eq!(r.http_method, HttpMethod::Get);
        assert_eq!(r.path_template, "/tasks/{taskId}/pushNotificationConfigs");
        assert_eq!(r.path_params, &["taskId"]);
        assert!(!r.streaming);
    }

    #[test]
    fn route_for_delete_task_push_notification_config() {
        let r = route_for("DeleteTaskPushNotificationConfig")
            .expect("DeleteTaskPushNotificationConfig should have a route");
        assert_eq!(r.http_method, HttpMethod::Delete);
        assert_eq!(
            r.path_template,
            "/tasks/{taskId}/pushNotificationConfigs/{id}"
        );
        assert_eq!(r.path_params, &["taskId", "id"]);
        assert!(!r.streaming);
    }

    #[test]
    fn route_for_get_extended_agent_card() {
        let r =
            route_for("GetExtendedAgentCard").expect("GetExtendedAgentCard should have a route");
        assert_eq!(r.http_method, HttpMethod::Get);
        assert_eq!(r.path_template, "/extendedAgentCard");
        assert!(r.path_params.is_empty());
        assert!(!r.streaming);
    }
}
