// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Axum framework integration for A2A servers.
//!
//! Provides [`A2aRouter`], which builds an [`axum::Router`] that handles all
//! A2A v1.0 methods using the existing [`RequestHandler`].
//!
//! # Quick start
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use a2a_protocol_server::dispatch::axum_adapter::A2aRouter;
//! use a2a_protocol_server::RequestHandlerBuilder;
//! # struct MyExecutor;
//! # impl a2a_protocol_server::executor::AgentExecutor for MyExecutor {
//! #     fn execute<'a>(&'a self, _ctx: &'a a2a_protocol_server::request_context::RequestContext,
//! #         _queue: &'a dyn a2a_protocol_server::streaming::EventQueueWriter,
//! #     ) -> std::pin::Pin<Box<dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>> + Send + 'a>> {
//! #         Box::pin(async { Ok(()) })
//! #     }
//! # }
//!
//! # async fn example() {
//! let handler = Arc::new(
//!     RequestHandlerBuilder::new(MyExecutor)
//!         .build()
//!         .expect("build handler"),
//! );
//!
//! let app = A2aRouter::new(handler).into_router();
//!
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
//! axum::serve(listener, app).await.unwrap();
//! # }
//! ```
//!
//! # Composability
//!
//! The returned router can be merged with other Axum routes, middleware, and
//! layers:
//!
//! ```rust,ignore
//! let app = axum::Router::new()
//!     .merge(A2aRouter::new(handler).into_router())
//!     .layer(tower_http::cors::CorsLayer::permissive())
//!     .route("/custom", get(custom_handler));
//! ```

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;

use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::build_sse_response;

// ── A2aRouter ────────────────────────────────────────────────────────────────

/// Builder for an Axum [`Router`] that serves all A2A v1.0 protocol methods.
///
/// Wraps an existing [`RequestHandler`] — all business logic, storage, and
/// interceptors are inherited. This is a thin HTTP routing layer only.
///
/// # REST routes
///
/// | Method | Path | A2A Method |
/// |--------|------|------------|
/// | `POST` | `/message:send` | `SendMessage` |
/// | `POST` | `/message:stream` | `SendStreamingMessage` |
/// | `GET` | `/tasks` | `ListTasks` |
/// | `GET` | `/tasks/:id` | `GetTask` |
/// | `POST` | `/tasks/:id:cancel` | `CancelTask` |
/// | `POST` or `GET` | `/tasks/:id:subscribe` | `SubscribeToTask` |
/// | `POST` | `/tasks/:task_id/pushNotificationConfigs` | `CreateTaskPushNotificationConfig` |
/// | `GET` | `/tasks/:task_id/pushNotificationConfigs` | `ListTaskPushNotificationConfigs` |
/// | `GET` | `/tasks/:task_id/pushNotificationConfigs/:id` | `GetTaskPushNotificationConfig` |
/// | `DELETE` | `/tasks/:task_id/pushNotificationConfigs/:id` | `DeleteTaskPushNotificationConfig` |
/// | `GET` | `/extendedAgentCard` | `GetExtendedAgentCard` |
/// | `GET` | `/.well-known/agent.json` | Agent Card Discovery |
/// | `GET` | `/health` | Health check |
pub struct A2aRouter {
    handler: Arc<RequestHandler>,
    config: super::DispatchConfig,
}

impl A2aRouter {
    /// Creates a new [`A2aRouter`] wrapping the given handler.
    #[must_use]
    pub fn new(handler: Arc<RequestHandler>) -> Self {
        Self {
            handler,
            config: super::DispatchConfig::default(),
        }
    }

    /// Creates a new [`A2aRouter`] with custom dispatch configuration.
    #[must_use]
    pub const fn with_config(handler: Arc<RequestHandler>, config: super::DispatchConfig) -> Self {
        Self { handler, config }
    }

    /// Builds the Axum [`Router`] with all A2A REST routes.
    ///
    /// The router uses `Arc<RequestHandler>` as shared state (via Axum's
    /// `State` extractor). Returns the configured `Router`.
    pub fn into_router(self) -> Router {
        let state = A2aState {
            handler: self.handler,
            config: Arc::new(self.config),
        };

        Router::new()
            // Messaging (colon-suffixed paths are literal — no conflict)
            .route("/message:send", post(handle_send_message))
            .route("/message:stream", post(handle_stream_message))
            // Task lifecycle: list tasks (no path param)
            .route("/tasks", get(handle_list_tasks))
            // All /tasks/* routes go through a catch-all dispatcher because
            // Axum doesn't support {id}:action suffix patterns (e.g.
            // /tasks/{id}:cancel). The catch-all parses the path segments
            // and dispatches to the appropriate handler.
            .route("/tasks/{*rest}", axum::routing::any(handle_tasks_catchall))
            // Extended card
            .route("/extendedAgentCard", get(handle_extended_card))
            // Agent card discovery
            .route("/.well-known/agent.json", get(handle_agent_card))
            // Health check
            .route("/health", get(handle_health))
            .with_state(state)
    }
}

// ── Shared state ─────────────────────────────────────────────────────────────

#[derive(Clone)]
struct A2aState {
    handler: Arc<RequestHandler>,
    config: Arc<super::DispatchConfig>,
}

// ── Helper: extract headers as HashMap ───────────────────────────────────────

fn extract_headers(headers: &axum::http::HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|val| (k.as_str().to_lowercase(), val.to_owned()))
        })
        .collect()
}

// ── Helper: convert A2A errors to HTTP responses ─────────────────────────────

fn a2a_error_to_response(err: &dyn std::fmt::Display, status: u16) -> axum::response::Response {
    let body = serde_json::json!({ "error": err.to_string() });
    (
        axum::http::StatusCode::from_u16(status)
            .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
        axum::Json(body),
    )
        .into_response()
}

const fn server_error_status(err: &crate::error::ServerError) -> u16 {
    use crate::error::ServerError;

    match err {
        ServerError::TaskNotFound(_) | ServerError::MethodNotFound(_) => 404,
        ServerError::InvalidParams(_) | ServerError::Serialization(_) => 400,
        ServerError::InvalidStateTransition { .. } | ServerError::TaskNotCancelable(_) => 409,
        ServerError::PushNotSupported => 501,
        ServerError::PayloadTooLarge(_) => 413,
        _ => 500,
    }
}

fn handler_error_to_response(err: &crate::error::ServerError) -> axum::response::Response {
    a2a_error_to_response(err, server_error_status(err))
}

// ── Helper: convert SSE hyper response to axum response ──────────────────────

/// Converts a hyper `Response<BoxBody<Bytes, Infallible>>` (from SSE builder)
/// into an axum `Response`.
fn hyper_sse_to_axum(
    resp: hyper::Response<http_body_util::combinators::BoxBody<Bytes, Infallible>>,
) -> axum::response::Response {
    let (parts, body) = resp.into_parts();
    let axum_body = Body::new(body);
    axum::response::Response::from_parts(parts, axum_body)
}

// ── Tasks catch-all dispatcher ────────────────────────────────────────────────

/// Dispatches all `/tasks/*` routes by parsing the path tail.
///
/// Handles:
/// - `GET /tasks/{id}` → `GetTask`
/// - `POST /tasks/{id}:cancel` → `CancelTask`
/// - `GET|POST /tasks/{id}:subscribe` → `SubscribeToTask`
/// - `POST /tasks/{task_id}/pushNotificationConfigs` → `CreateTaskPushNotificationConfig`
/// - `GET /tasks/{task_id}/pushNotificationConfigs` → `ListTaskPushNotificationConfigs`
/// - `GET /tasks/{task_id}/pushNotificationConfigs/{id}` → `GetTaskPushNotificationConfig`
/// - `DELETE /tasks/{task_id}/pushNotificationConfigs/{id}` → `DeleteTaskPushNotificationConfig`
async fn handle_tasks_catchall(
    State(state): State<A2aState>,
    method: axum::http::Method,
    Path(rest): Path<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let hdrs = extract_headers(&headers);
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();

    match (method.as_str(), segments.as_slice()) {
        // GET /tasks/{id} (no colon action)
        ("GET", [id]) if !id.contains(':') => handle_get_task_inner(&state, id, &hdrs).await,

        // POST /tasks/{id}:cancel
        ("POST", [id_action]) if id_action.ends_with(":cancel") => {
            let id = &id_action[..id_action.len() - ":cancel".len()];
            handle_cancel_task_inner(&state, id, &hdrs).await
        }

        // GET|POST /tasks/{id}:subscribe
        ("GET" | "POST", [id_action]) if id_action.ends_with(":subscribe") => {
            let id = &id_action[..id_action.len() - ":subscribe".len()];
            handle_subscribe_inner(&state, id, &hdrs).await
        }

        // POST /tasks/{task_id}/pushNotificationConfigs
        ("POST", [task_id, "pushNotificationConfigs"]) => {
            handle_create_push_config_inner(&state, task_id, &hdrs, body).await
        }

        // GET /tasks/{task_id}/pushNotificationConfigs
        ("GET", [task_id, "pushNotificationConfigs"]) => {
            handle_list_push_configs_inner(&state, task_id, &hdrs).await
        }

        // GET /tasks/{task_id}/pushNotificationConfigs/{config_id}
        ("GET", [task_id, "pushNotificationConfigs", config_id]) => {
            handle_get_push_config_inner(&state, task_id, config_id, &hdrs).await
        }

        // DELETE /tasks/{task_id}/pushNotificationConfigs/{config_id}
        ("DELETE", [task_id, "pushNotificationConfigs", config_id]) => {
            handle_delete_push_config_inner(&state, task_id, config_id, &hdrs).await
        }

        _ => a2a_error_to_response(&"not found", 404),
    }
}

// ── Route handlers (Axum extractor-based) ────────────────────────────────────

async fn handle_send_message(
    State(state): State<A2aState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    handle_send_inner(&state, false, &headers, body).await
}

async fn handle_stream_message(
    State(state): State<A2aState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    handle_send_inner(&state, true, &headers, body).await
}

async fn handle_list_tasks(
    State(state): State<A2aState>,
    Query(query): Query<HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let hdrs = extract_headers(&headers);
    let params = a2a_protocol_types::params::ListTasksParams {
        tenant: None,
        context_id: query.get("contextId").cloned(),
        status: query
            .get("status")
            .and_then(|s| serde_json::from_value(serde_json::Value::String(s.clone())).ok()),
        page_size: query.get("pageSize").and_then(|v| v.parse().ok()),
        page_token: query.get("pageToken").cloned(),
        status_timestamp_after: query.get("statusTimestampAfter").cloned(),
        include_artifacts: query.get("includeArtifacts").and_then(|v| v.parse().ok()),
        history_length: query.get("historyLength").and_then(|v| v.parse().ok()),
    };
    match state.handler.on_list_tasks(params, Some(&hdrs)).await {
        Ok(result) => axum::Json(result).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_extended_card(
    State(state): State<A2aState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let hdrs = extract_headers(&headers);
    match state.handler.on_get_extended_agent_card(Some(&hdrs)).await {
        Ok(card) => axum::Json(card).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_agent_card(State(state): State<A2aState>) -> axum::response::Response {
    state.handler.agent_card.as_ref().map_or_else(
        || a2a_error_to_response(&"agent card not configured", 404),
        |card| axum::Json(card).into_response(),
    )
}

async fn handle_health() -> axum::response::Response {
    axum::Json(serde_json::json!({"status": "ok"})).into_response()
}

// ── Inner handlers (shared by route handlers and catch-all) ──────────────────

async fn handle_send_inner(
    state: &A2aState,
    streaming: bool,
    headers: &axum::http::HeaderMap,
    body: Bytes,
) -> axum::response::Response {
    let hdrs = extract_headers(headers);
    let params: a2a_protocol_types::params::MessageSendParams = match serde_json::from_slice(&body)
    {
        Ok(p) => p,
        Err(e) => return a2a_error_to_response(&e, 400),
    };
    match state
        .handler
        .on_send_message(params, streaming, Some(&hdrs))
        .await
    {
        Ok(SendMessageResult::Response(resp)) => axum::Json(resp).into_response(),
        Ok(SendMessageResult::Stream(reader)) => hyper_sse_to_axum(build_sse_response(
            reader,
            Some(state.config.sse_keep_alive_interval),
            Some(state.config.sse_channel_capacity),
        )),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_get_task_inner(
    state: &A2aState,
    id: &str,
    hdrs: &HashMap<String, String>,
) -> axum::response::Response {
    let params = a2a_protocol_types::params::TaskQueryParams {
        tenant: None,
        id: id.to_owned(),
        history_length: None,
    };
    match state.handler.on_get_task(params, Some(hdrs)).await {
        Ok(task) => axum::Json(task).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_cancel_task_inner(
    state: &A2aState,
    id: &str,
    hdrs: &HashMap<String, String>,
) -> axum::response::Response {
    let params = a2a_protocol_types::params::CancelTaskParams {
        tenant: None,
        id: id.to_owned(),
        metadata: None,
    };
    match state.handler.on_cancel_task(params, Some(hdrs)).await {
        Ok(task) => axum::Json(task).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_subscribe_inner(
    state: &A2aState,
    id: &str,
    hdrs: &HashMap<String, String>,
) -> axum::response::Response {
    let params = a2a_protocol_types::params::TaskIdParams {
        tenant: None,
        id: id.to_owned(),
    };
    match state.handler.on_resubscribe(params, Some(hdrs)).await {
        Ok(reader) => hyper_sse_to_axum(build_sse_response(
            reader,
            Some(state.config.sse_keep_alive_interval),
            Some(state.config.sse_channel_capacity),
        )),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_create_push_config_inner(
    state: &A2aState,
    task_id: &str,
    hdrs: &HashMap<String, String>,
    body: Bytes,
) -> axum::response::Response {
    let mut value: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return a2a_error_to_response(&e, 400),
    };
    if let Some(obj) = value.as_object_mut() {
        obj.entry("taskId")
            .or_insert_with(|| serde_json::Value::String(task_id.to_owned()));
    }
    let config: a2a_protocol_types::push::TaskPushNotificationConfig =
        match serde_json::from_value(value) {
            Ok(c) => c,
            Err(e) => return a2a_error_to_response(&e, 400),
        };
    match state.handler.on_set_push_config(config, Some(hdrs)).await {
        Ok(result) => axum::Json(result).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_get_push_config_inner(
    state: &A2aState,
    task_id: &str,
    config_id: &str,
    hdrs: &HashMap<String, String>,
) -> axum::response::Response {
    let params = a2a_protocol_types::params::GetPushConfigParams {
        tenant: None,
        task_id: task_id.to_owned(),
        id: config_id.to_owned(),
    };
    match state.handler.on_get_push_config(params, Some(hdrs)).await {
        Ok(config) => axum::Json(config).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_list_push_configs_inner(
    state: &A2aState,
    task_id: &str,
    hdrs: &HashMap<String, String>,
) -> axum::response::Response {
    match state
        .handler
        .on_list_push_configs(task_id, None, Some(hdrs))
        .await
    {
        Ok(configs) => {
            let resp = a2a_protocol_types::responses::ListPushConfigsResponse {
                configs,
                next_page_token: None,
            };
            axum::Json(resp).into_response()
        }
        Err(e) => handler_error_to_response(&e),
    }
}

async fn handle_delete_push_config_inner(
    state: &A2aState,
    task_id: &str,
    config_id: &str,
    hdrs: &HashMap<String, String>,
) -> axum::response::Response {
    let params = a2a_protocol_types::params::DeletePushConfigParams {
        tenant: None,
        task_id: task_id.to_owned(),
        id: config_id.to_owned(),
    };
    match state
        .handler
        .on_delete_push_config(params, Some(hdrs))
        .await
    {
        Ok(()) => axum::Json(serde_json::json!({})).into_response(),
        Err(e) => handler_error_to_response(&e),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_headers_lowercases_names() {
        let mut map = axum::http::HeaderMap::new();
        map.insert("X-Request-ID", "abc".parse().unwrap());
        map.insert("content-type", "application/json".parse().unwrap());

        let result = extract_headers(&map);
        assert_eq!(result.get("x-request-id").unwrap(), "abc");
        assert_eq!(result.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn extract_headers_skips_non_utf8_values() {
        let mut map = axum::http::HeaderMap::new();
        map.insert("good", "valid".parse().unwrap());
        // Non-UTF8 values are filtered out by to_str().ok()
        let result = extract_headers(&map);
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("good").unwrap(), "valid");
    }

    #[test]
    fn extract_headers_empty_map() {
        let map = axum::http::HeaderMap::new();
        let result = extract_headers(&map);
        assert!(result.is_empty());
    }

    #[test]
    fn a2a_state_is_clone() {
        fn assert_clone<T: Clone>() {}
        assert_clone::<A2aState>();
    }

    #[test]
    fn server_error_status_task_not_found() {
        use crate::error::ServerError;
        assert_eq!(
            server_error_status(&ServerError::TaskNotFound("t".into())),
            404
        );
    }

    #[test]
    fn server_error_status_method_not_found() {
        use crate::error::ServerError;
        assert_eq!(
            server_error_status(&ServerError::MethodNotFound("m".into())),
            404
        );
    }

    #[test]
    fn server_error_status_invalid_params() {
        use crate::error::ServerError;
        assert_eq!(
            server_error_status(&ServerError::InvalidParams("p".into())),
            400
        );
    }

    #[test]
    fn server_error_status_serialization() {
        use crate::error::ServerError;
        let err = ServerError::Serialization(serde_json::from_str::<String>("bad").unwrap_err());
        assert_eq!(server_error_status(&err), 400);
    }

    #[test]
    fn server_error_status_task_not_cancelable() {
        use crate::error::ServerError;
        assert_eq!(
            server_error_status(&ServerError::TaskNotCancelable("t".into())),
            409
        );
    }

    #[test]
    fn server_error_status_invalid_state_transition() {
        use crate::error::ServerError;
        let err = ServerError::InvalidStateTransition {
            task_id: "t".into(),
            from: a2a_protocol_types::task::TaskState::Working,
            to: a2a_protocol_types::task::TaskState::Submitted,
        };
        assert_eq!(server_error_status(&err), 409);
    }

    #[test]
    fn server_error_status_push_not_supported() {
        use crate::error::ServerError;
        assert_eq!(server_error_status(&ServerError::PushNotSupported), 501);
    }

    #[test]
    fn server_error_status_payload_too_large() {
        use crate::error::ServerError;
        assert_eq!(
            server_error_status(&ServerError::PayloadTooLarge("big".into())),
            413
        );
    }

    #[test]
    fn server_error_status_internal() {
        use crate::error::ServerError;
        assert_eq!(
            server_error_status(&ServerError::Internal("oops".into())),
            500
        );
    }

    #[test]
    fn a2a_error_to_response_returns_correct_status() {
        let resp = a2a_error_to_response(&"test error", 400);
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[test]
    fn a2a_error_to_response_returns_json_body() {
        let resp = a2a_error_to_response(&"not found", 404);
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[test]
    fn a2a_error_to_response_invalid_status_falls_back_to_500() {
        // HTTP status codes are valid 100-999; 1000+ is invalid
        let resp = a2a_error_to_response(&"bad status", 1000);
        assert_eq!(resp.status().as_u16(), 500);
    }

    #[test]
    fn handler_error_to_response_maps_correctly() {
        use crate::error::ServerError;
        let resp = handler_error_to_response(&ServerError::TaskNotFound("t1".into()));
        assert_eq!(resp.status().as_u16(), 404);

        let resp = handler_error_to_response(&ServerError::InvalidParams("bad".into()));
        assert_eq!(resp.status().as_u16(), 400);

        let resp = handler_error_to_response(&ServerError::Internal("oops".into()));
        assert_eq!(resp.status().as_u16(), 500);
    }

    #[test]
    fn a2a_router_new_creates_with_defaults() {
        // Verify A2aRouter::new doesn't panic and uses default DispatchConfig
        use crate::builder::RequestHandlerBuilder;

        struct NoopExecutor;
        impl crate::executor::AgentExecutor for NoopExecutor {
            fn execute<'a>(
                &'a self,
                _ctx: &'a crate::request_context::RequestContext,
                _queue: &'a dyn crate::streaming::EventQueueWriter,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>>
                        + Send
                        + 'a,
                >,
            > {
                Box::pin(async { Ok(()) })
            }
        }

        let handler = Arc::new(RequestHandlerBuilder::new(NoopExecutor).build().unwrap());
        let router = A2aRouter::new(handler);
        // Should not panic when building the router
        let _axum_router = router.into_router();
    }

    #[test]
    fn a2a_router_with_config() {
        use crate::builder::RequestHandlerBuilder;

        struct NoopExecutor;
        impl crate::executor::AgentExecutor for NoopExecutor {
            fn execute<'a>(
                &'a self,
                _ctx: &'a crate::request_context::RequestContext,
                _queue: &'a dyn crate::streaming::EventQueueWriter,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = a2a_protocol_types::error::A2aResult<()>>
                        + Send
                        + 'a,
                >,
            > {
                Box::pin(async { Ok(()) })
            }
        }

        let handler = Arc::new(RequestHandlerBuilder::new(NoopExecutor).build().unwrap());
        let config =
            super::super::DispatchConfig::default().with_max_request_body_size(8 * 1024 * 1024);
        let router = A2aRouter::with_config(handler, config);
        let _axum_router = router.into_router();
    }
}
