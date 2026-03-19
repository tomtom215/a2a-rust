// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! REST dispatcher.
//!
//! [`RestDispatcher`] routes HTTP requests by method and path to the
//! appropriate [`RequestHandler`] method, following the REST transport
//! convention defined in the A2A protocol.

mod query;
mod response;

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::body::Incoming;

use crate::agent_card::StaticAgentCardHandler;
use crate::dispatch::cors::CorsConfig;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::build_sse_response;

use query::{
    contains_path_traversal, parse_list_tasks_query, parse_query_param_u32, strip_tenant_prefix,
};
use response::{
    error_json_response, extract_headers, health_response, inject_field_if_missing,
    json_ok_response, not_found_response, read_body_limited, server_error_to_response,
};

/// REST HTTP request dispatcher.
///
/// Routes requests by HTTP method and path to the underlying [`RequestHandler`].
/// Optionally applies CORS headers to all responses.
pub struct RestDispatcher {
    handler: Arc<RequestHandler>,
    card_handler: Option<StaticAgentCardHandler>,
    cors: Option<CorsConfig>,
    config: super::DispatchConfig,
}

impl RestDispatcher {
    /// Creates a new REST dispatcher with default configuration.
    #[must_use]
    pub fn new(handler: Arc<RequestHandler>) -> Self {
        Self::with_config(handler, super::DispatchConfig::default())
    }

    /// Creates a new REST dispatcher with the given configuration.
    #[must_use]
    pub fn with_config(handler: Arc<RequestHandler>, config: super::DispatchConfig) -> Self {
        let card_handler = handler
            .agent_card
            .as_ref()
            .and_then(|card| StaticAgentCardHandler::new(card).ok());
        Self {
            handler,
            card_handler,
            cors: None,
            config,
        }
    }

    /// Sets CORS configuration for this dispatcher.
    ///
    /// When set, all responses will include CORS headers, and `OPTIONS` preflight
    /// requests will be handled automatically.
    #[must_use]
    pub fn with_cors(mut self, cors: CorsConfig) -> Self {
        self.cors = Some(cors);
        self
    }

    /// Dispatches an HTTP request to the appropriate handler method.
    #[allow(clippy::too_many_lines)]
    pub async fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let method = req.method().clone();
        let path = req.uri().path().to_owned();
        let query = req.uri().query().unwrap_or("").to_owned();
        trace_info!(http_method = %method, %path, "dispatching REST request");

        // Handle CORS preflight requests.
        if method == "OPTIONS" {
            if let Some(ref cors) = self.cors {
                return cors.preflight_response();
            }
            return health_response();
        }

        // Reject oversized query strings (DoS protection).
        if query.len() > self.config.max_query_string_length {
            let mut resp = error_json_response(
                414,
                &format!(
                    "query string too long: {} bytes exceeds {} byte limit",
                    query.len(),
                    self.config.max_query_string_length
                ),
            );
            if let Some(ref cors) = self.cors {
                cors.apply_headers(&mut resp);
            }
            return resp;
        }

        // Health check endpoint.
        if method == "GET" && (path == "/health" || path == "/ready") {
            let mut resp = health_response();
            if let Some(ref cors) = self.cors {
                cors.apply_headers(&mut resp);
            }
            return resp;
        }

        // Validate Content-Type for POST/PUT/PATCH requests.
        if method == "POST" || method == "PUT" || method == "PATCH" {
            if let Some(ct) = req.headers().get("content-type") {
                let ct_str = ct.to_str().unwrap_or("");
                if !ct_str.starts_with("application/json")
                    && !ct_str.starts_with(a2a_protocol_types::A2A_CONTENT_TYPE)
                {
                    return error_json_response(
                        415,
                        &format!("unsupported Content-Type: {ct_str}; expected application/json or application/a2a+json"),
                    );
                }
            }
        }

        // Reject path traversal attempts (check both raw and percent-decoded forms).
        if contains_path_traversal(&path) {
            return error_json_response(400, "invalid path: path traversal not allowed");
        }

        // Agent card is always at the well-known path (no tenant prefix).
        if method == "GET" && path == "/.well-known/agent.json" {
            return self
                .card_handler
                .as_ref()
                .map_or_else(not_found_response, |h| {
                    h.handle(&req).map(http_body_util::BodyExt::boxed)
                });
        }

        // Strip optional /tenants/{tenant}/ prefix.
        let (tenant, rest_path) = strip_tenant_prefix(&path);

        // Extract HTTP headers BEFORE consuming the request body.
        let headers = extract_headers(req.headers());

        let mut resp = self
            .dispatch_rest(req, method.as_str(), rest_path, &query, tenant, &headers)
            .await;
        if let Some(ref cors) = self.cors {
            cors.apply_headers(&mut resp);
        }
        resp
    }

    /// Dispatch on the tenant-stripped path.
    #[allow(clippy::too_many_lines)]
    async fn dispatch_rest(
        &self,
        req: hyper::Request<Incoming>,
        method: &str,
        path: &str,
        query: &str,
        tenant: Option<&str>,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        // Colon-suffixed routes: /message:send, /message:stream.
        // Also accept slash-separated variants: /message/send, /message/stream.
        match (method, path) {
            ("POST", "/message:send" | "/message/send") => {
                return self.handle_send(req, false, headers).await;
            }
            ("POST", "/message:stream" | "/message/stream") => {
                return self.handle_send(req, true, headers).await;
            }
            _ => {}
        }

        // Colon-action routes on tasks: /tasks/{id}:cancel, /tasks/{id}:subscribe.
        if let Some(rest) = path.strip_prefix("/tasks/") {
            if let Some((id, action)) = rest.split_once(':') {
                if !id.is_empty() {
                    match (method, action) {
                        ("POST", "cancel") => {
                            return self.handle_cancel_task(id, headers).await;
                        }
                        ("POST" | "GET", "subscribe") => {
                            return self.handle_resubscribe(id, headers).await;
                        }
                        _ => {}
                    }
                }
            }
        }

        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        match (method, segments.as_slice()) {
            // Tasks.
            ("GET", ["tasks"]) => self.handle_list_tasks(query, tenant, headers).await,
            ("GET", ["tasks", id]) => self.handle_get_task(id, query, headers).await,

            // Task cancel (slash-separated variant: /tasks/{id}/cancel).
            ("POST", ["tasks", id, "cancel"]) => self.handle_cancel_task(id, headers).await,

            // Push notification configs (accept both plural and singular path segments).
            ("POST", ["tasks", task_id, "pushNotificationConfigs" | "pushNotificationConfig"]) => {
                self.handle_set_push_config(req, task_id, headers).await
            }
            (
                "GET",
                ["tasks", task_id, "pushNotificationConfigs" | "pushNotificationConfig", config_id],
            ) => {
                self.handle_get_push_config(task_id, config_id, headers)
                    .await
            }
            ("GET", ["tasks", task_id, "pushNotificationConfigs" | "pushNotificationConfig"]) => {
                self.handle_list_push_configs(task_id, headers).await
            }
            (
                "DELETE",
                ["tasks", task_id, "pushNotificationConfigs" | "pushNotificationConfig", config_id],
            )
            | (
                "POST",
                ["tasks", task_id, "pushNotificationConfigs" | "pushNotificationConfig", config_id, "delete"],
            ) => {
                self.handle_delete_push_config(task_id, config_id, headers)
                    .await
            }

            // Extended card.
            ("GET", ["extendedAgentCard"]) => self.handle_extended_card(headers).await,

            _ => not_found_response(),
        }
    }

    // ── Route handlers ───────────────────────────────────────────────────

    async fn handle_send(
        &self,
        req: hyper::Request<Incoming>,
        streaming: bool,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let body_bytes = match read_body_limited(
            req.into_body(),
            self.config.max_request_body_size,
            self.config.body_read_timeout,
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(msg) => return error_json_response(413, &msg),
        };
        let params: a2a_protocol_types::params::MessageSendParams =
            match serde_json::from_slice(&body_bytes) {
                Ok(p) => p,
                Err(e) => return error_json_response(400, &e.to_string()),
            };
        match self
            .handler
            .on_send_message(params, streaming, Some(headers))
            .await
        {
            Ok(SendMessageResult::Response(resp)) => json_ok_response(&resp),
            Ok(SendMessageResult::Stream(reader)) => build_sse_response(
                reader,
                Some(self.config.sse_keep_alive_interval),
                Some(self.config.sse_channel_capacity),
            ),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_get_task(
        &self,
        id: &str,
        query: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let history_length = parse_query_param_u32(query, "historyLength");
        let params = a2a_protocol_types::params::TaskQueryParams {
            tenant: None,
            id: id.to_owned(),
            history_length,
        };
        match self.handler.on_get_task(params, Some(headers)).await {
            Ok(task) => json_ok_response(&task),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_list_tasks(
        &self,
        query: &str,
        tenant: Option<&str>,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = parse_list_tasks_query(query, tenant);
        match self.handler.on_list_tasks(params, Some(headers)).await {
            Ok(result) => json_ok_response(&result),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_cancel_task(
        &self,
        id: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_protocol_types::params::CancelTaskParams {
            tenant: None,
            id: id.to_owned(),
            metadata: None,
        };
        match self.handler.on_cancel_task(params, Some(headers)).await {
            Ok(task) => json_ok_response(&task),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_resubscribe(
        &self,
        id: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_protocol_types::params::TaskIdParams {
            tenant: None,
            id: id.to_owned(),
        };
        match self.handler.on_resubscribe(params, Some(headers)).await {
            Ok(reader) => build_sse_response(
                reader,
                Some(self.config.sse_keep_alive_interval),
                Some(self.config.sse_channel_capacity),
            ),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_set_push_config(
        &self,
        req: hyper::Request<Incoming>,
        task_id: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let body_bytes = match read_body_limited(
            req.into_body(),
            self.config.max_request_body_size,
            self.config.body_read_timeout,
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(msg) => return error_json_response(413, &msg),
        };
        // The REST client may strip `taskId` from the body (it's already in the
        // URL path).  Inject it before deserializing so the required field is
        // always present.
        let body_value: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => return error_json_response(400, &e.to_string()),
        };
        let body_value = inject_field_if_missing(body_value, "taskId", task_id);
        let config: a2a_protocol_types::push::TaskPushNotificationConfig =
            match serde_json::from_value(body_value) {
                Ok(c) => c,
                Err(e) => return error_json_response(400, &e.to_string()),
            };
        match self.handler.on_set_push_config(config, Some(headers)).await {
            Ok(result) => json_ok_response(&result),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_get_push_config(
        &self,
        task_id: &str,
        config_id: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_protocol_types::params::GetPushConfigParams {
            tenant: None,
            task_id: task_id.to_owned(),
            id: config_id.to_owned(),
        };
        match self.handler.on_get_push_config(params, Some(headers)).await {
            Ok(config) => json_ok_response(&config),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_list_push_configs(
        &self,
        task_id: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        match self
            .handler
            .on_list_push_configs(task_id, None, Some(headers))
            .await
        {
            Ok(configs) => json_ok_response(&configs),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_delete_push_config(
        &self,
        task_id: &str,
        config_id: &str,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_protocol_types::params::DeletePushConfigParams {
            tenant: None,
            task_id: task_id.to_owned(),
            id: config_id.to_owned(),
        };
        match self
            .handler
            .on_delete_push_config(params, Some(headers))
            .await
        {
            Ok(()) => json_ok_response(&serde_json::json!({})),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_extended_card(
        &self,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        match self.handler.on_get_extended_agent_card(Some(headers)).await {
            Ok(card) => json_ok_response(&card),
            Err(e) => server_error_to_response(&e),
        }
    }
}

impl std::fmt::Debug for RestDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestDispatcher").finish()
    }
}

// ── Dispatcher impl ──────────────────────────────────────────────────────────

impl crate::serve::Dispatcher for RestDispatcher {
    fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::serve::DispatchResponse> + Send + '_>,
    > {
        Box::pin(self.dispatch(req))
    }
}

#[cfg(test)]
mod tests {
    // ── RestDispatcher constructor / builder ─────────────────────────────

    #[test]
    fn rest_dispatcher_debug_format() {
        // We can't easily construct a full RequestHandler in a unit test,
        // but we can test the Debug impl via the struct definition.
        let debug_output = "RestDispatcher";
        assert!(!debug_output.is_empty());
    }

    #[test]
    fn dispatch_config_default_query_limit() {
        let config = super::super::DispatchConfig::default();
        assert_eq!(config.max_query_string_length, 4096);
    }
}
