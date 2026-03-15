// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! REST dispatcher.
//!
//! [`RestDispatcher`] routes HTTP requests by method and path to the
//! appropriate [`RequestHandler`] method, following the REST transport
//! convention defined in the A2A protocol.

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;

use crate::agent_card::StaticAgentCardHandler;
use crate::error::ServerError;
use crate::executor::AgentExecutor;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::build_sse_response;

/// REST HTTP request dispatcher.
///
/// Routes requests by HTTP method and path to the underlying [`RequestHandler`].
pub struct RestDispatcher<E: AgentExecutor> {
    handler: Arc<RequestHandler<E>>,
    card_handler: Option<StaticAgentCardHandler>,
}

impl<E: AgentExecutor> RestDispatcher<E> {
    /// Creates a new REST dispatcher.
    #[must_use]
    pub fn new(handler: Arc<RequestHandler<E>>) -> Self {
        let card_handler = handler
            .agent_card
            .as_ref()
            .and_then(|card| StaticAgentCardHandler::new(card).ok());
        Self {
            handler,
            card_handler,
        }
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

        // Health check endpoint.
        if method == "GET" && (path == "/health" || path == "/ready") {
            return health_response();
        }

        // Validate Content-Type for POST/PUT/PATCH requests.
        if method == "POST" || method == "PUT" || method == "PATCH" {
            if let Some(ct) = req.headers().get("content-type") {
                let ct_str = ct.to_str().unwrap_or("");
                if !ct_str.starts_with("application/json")
                    && !ct_str.starts_with(a2a_types::A2A_CONTENT_TYPE)
                {
                    return error_json_response(
                        415,
                        &format!("unsupported Content-Type: {ct_str}; expected application/json or application/a2a+json"),
                    );
                }
            }
        }

        // Reject path traversal attempts.
        if path.contains("..") {
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
        let (tenant, rest) = strip_tenant_prefix(&path);

        self.dispatch_rest(req, method.as_str(), rest, &query, tenant)
            .await
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
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        // Colon-suffixed routes: /message:send, /message:stream.
        match (method, path) {
            ("POST", "/message:send") => return self.handle_send(req, false).await,
            ("POST", "/message:stream") => return self.handle_send(req, true).await,
            _ => {}
        }

        // Colon-action routes on tasks: /tasks/{id}:cancel, /tasks/{id}:subscribe.
        if let Some(rest) = path.strip_prefix("/tasks/") {
            if let Some((id, action)) = rest.split_once(':') {
                if !id.is_empty() {
                    match (method, action) {
                        ("POST", "cancel") => return self.handle_cancel_task(id).await,
                        ("POST" | "GET", "subscribe") => {
                            return self.handle_resubscribe(id).await;
                        }
                        _ => {}
                    }
                }
            }
        }

        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        match (method, segments.as_slice()) {
            // Tasks.
            ("GET", ["tasks"]) => self.handle_list_tasks(query, tenant).await,
            ("GET", ["tasks", id]) => self.handle_get_task(id, query).await,

            // Push notification configs.
            ("POST", ["tasks", task_id, "pushNotificationConfigs"]) => {
                self.handle_set_push_config(req, task_id).await
            }
            ("GET", ["tasks", task_id, "pushNotificationConfigs", config_id]) => {
                self.handle_get_push_config(task_id, config_id).await
            }
            ("GET", ["tasks", task_id, "pushNotificationConfigs"]) => {
                self.handle_list_push_configs(task_id).await
            }
            ("DELETE", ["tasks", task_id, "pushNotificationConfigs", config_id]) => {
                self.handle_delete_push_config(task_id, config_id).await
            }

            // Extended card.
            ("GET", ["extendedAgentCard"]) => self.handle_extended_card().await,

            _ => not_found_response(),
        }
    }

    // ── Route handlers ───────────────────────────────────────────────────

    async fn handle_send(
        &self,
        req: hyper::Request<Incoming>,
        streaming: bool,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let body_bytes = match read_body_limited(req.into_body(), MAX_REQUEST_BODY_SIZE).await {
            Ok(bytes) => bytes,
            Err(msg) => return error_json_response(413, &msg),
        };
        let params: a2a_types::params::MessageSendParams = match serde_json::from_slice(&body_bytes)
        {
            Ok(p) => p,
            Err(e) => return error_json_response(400, &e.to_string()),
        };
        match self.handler.on_send_message(params, streaming).await {
            Ok(SendMessageResult::Response(resp)) => json_ok_response(&resp),
            Ok(SendMessageResult::Stream(reader)) => build_sse_response(reader, None),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_get_task(
        &self,
        id: &str,
        query: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let history_length = parse_query_param_u32(query, "historyLength");
        let params = a2a_types::params::TaskQueryParams {
            tenant: None,
            id: id.to_owned(),
            history_length,
        };
        match self.handler.on_get_task(params).await {
            Ok(task) => json_ok_response(&task),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_list_tasks(
        &self,
        query: &str,
        tenant: Option<&str>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = parse_list_tasks_query(query, tenant);
        match self.handler.on_list_tasks(params).await {
            Ok(result) => json_ok_response(&result),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_cancel_task(&self, id: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::CancelTaskParams {
            tenant: None,
            id: id.to_owned(),
            metadata: None,
        };
        match self.handler.on_cancel_task(params).await {
            Ok(task) => json_ok_response(&task),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_resubscribe(&self, id: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::TaskIdParams {
            tenant: None,
            id: id.to_owned(),
        };
        match self.handler.on_resubscribe(params).await {
            Ok(reader) => build_sse_response(reader, None),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_set_push_config(
        &self,
        req: hyper::Request<Incoming>,
        _task_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let body_bytes = match read_body_limited(req.into_body(), MAX_REQUEST_BODY_SIZE).await {
            Ok(bytes) => bytes,
            Err(msg) => return error_json_response(413, &msg),
        };
        let config: a2a_types::push::TaskPushNotificationConfig =
            match serde_json::from_slice(&body_bytes) {
                Ok(c) => c,
                Err(e) => return error_json_response(400, &e.to_string()),
            };
        match self.handler.on_set_push_config(config).await {
            Ok(result) => json_ok_response(&result),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_get_push_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::GetPushConfigParams {
            tenant: None,
            task_id: task_id.to_owned(),
            id: config_id.to_owned(),
        };
        match self.handler.on_get_push_config(params).await {
            Ok(config) => json_ok_response(&config),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_list_push_configs(
        &self,
        task_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        match self.handler.on_list_push_configs(task_id).await {
            Ok(configs) => json_ok_response(&configs),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_delete_push_config(
        &self,
        task_id: &str,
        config_id: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = a2a_types::params::DeletePushConfigParams {
            tenant: None,
            task_id: task_id.to_owned(),
            id: config_id.to_owned(),
        };
        match self.handler.on_delete_push_config(params).await {
            Ok(()) => json_ok_response(&serde_json::json!({})),
            Err(e) => server_error_to_response(&e),
        }
    }

    async fn handle_extended_card(&self) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        match self.handler.on_get_extended_agent_card().await {
            Ok(card) => json_ok_response(&card),
            Err(e) => server_error_to_response(&e),
        }
    }
}

impl<E: AgentExecutor> std::fmt::Debug for RestDispatcher<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestDispatcher").finish()
    }
}

// ── Response helpers ─────────────────────────────────────────────────────────

fn json_ok_response<T: serde::Serialize>(value: &T) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    match serde_json::to_vec(value) {
        Ok(body) => build_json_response(200, body),
        Err(_err) => {
            trace_error!(error = %_err, "REST response serialization failed");
            internal_error_response()
        }
    }
}

fn error_json_response(status: u16, message: &str) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let body = serde_json::json!({ "error": message });
    serde_json::to_vec(&body).map_or_else(
        |_| internal_error_response(),
        |bytes| build_json_response(status, bytes),
    )
}

/// Fallback when serialization itself fails.
fn internal_error_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let body = br#"{"error":"internal serialization error"}"#;
    build_json_response(500, body.to_vec())
}

fn not_found_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
    error_json_response(404, "not found")
}

fn server_error_to_response(err: &ServerError) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let status = match err {
        ServerError::TaskNotFound(_) | ServerError::MethodNotFound(_) => 404,
        ServerError::TaskNotCancelable(_) => 409,
        ServerError::InvalidParams(_)
        | ServerError::Serialization(_)
        | ServerError::PushNotSupported => 400,
        _ => 500,
    };
    let a2a_err = err.to_a2a_error();
    serde_json::to_vec(&a2a_err).map_or_else(
        |_| internal_error_response(),
        |body| build_json_response(status, body),
    )
}

// ── Query parsing helpers ───────────────────────────────────────────────────

/// Strips an optional `/tenants/{tenant}/` prefix, returning the tenant and
/// remaining path.
fn strip_tenant_prefix(path: &str) -> (Option<&str>, &str) {
    if let Some(rest) = path.strip_prefix("/tenants/") {
        if let Some(slash_pos) = rest.find('/') {
            let tenant = &rest[..slash_pos];
            let remaining = &rest[slash_pos..];
            return (Some(tenant), remaining);
        }
    }
    (None, path)
}

/// Parses a single query parameter value as `u32`.
fn parse_query_param_u32(query: &str, key: &str) -> Option<u32> {
    parse_query_param(query, key).and_then(|v| v.parse::<u32>().ok())
}

/// Parses a single query parameter value as a string, with percent-decoding.
fn parse_query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            Some(percent_decode(v))
        } else {
            None
        }
    })
}

/// Decodes percent-encoded characters in a query parameter value.
///
/// Handles `%XX` hex sequences and `+` as space (application/x-www-form-urlencoded).
fn percent_decode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut bytes = input.as_bytes().iter();
    while let Some(&b) = bytes.next() {
        match b {
            b'%' => {
                let hi = bytes.next().copied();
                let lo = bytes.next().copied();
                if let (Some(h), Some(l)) = (hi, lo) {
                    if let (Some(h), Some(l)) = (hex_val(h), hex_val(l)) {
                        output.push(char::from(h << 4 | l));
                        continue;
                    }
                }
                // Invalid percent sequence — pass through as-is.
                output.push('%');
            }
            b'+' => output.push(' '),
            _ => output.push(char::from(b)),
        }
    }
    output
}

/// Returns the numeric value of a hex digit, or `None` if invalid.
const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Parses a single query parameter value as `bool`.
fn parse_query_param_bool(query: &str, key: &str) -> Option<bool> {
    parse_query_param(query, key).map(|v| v == "true" || v == "1")
}

/// Maximum request body size in bytes (4 MiB).
const MAX_REQUEST_BODY_SIZE: usize = 4 * 1024 * 1024;

/// Maximum duration to read a complete request body (slow loris protection).
const BODY_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Reads a request body with a size limit and timeout.
async fn read_body_limited(body: Incoming, max_size: usize) -> Result<Bytes, String> {
    let size_hint = <Incoming as hyper::body::Body>::size_hint(&body);
    if let Some(upper) = size_hint.upper() {
        if upper > max_size as u64 {
            return Err(format!(
                "request body too large: {upper} bytes exceeds {max_size} byte limit"
            ));
        }
    }
    let collected = tokio::time::timeout(BODY_READ_TIMEOUT, body.collect())
        .await
        .map_err(|_| "request body read timed out".to_owned())?
        .map_err(|e| e.to_string())?;
    let bytes = collected.to_bytes();
    if bytes.len() > max_size {
        return Err(format!(
            "request body too large: {} bytes exceeds {max_size} byte limit",
            bytes.len()
        ));
    }
    Ok(bytes)
}

/// Returns a health check response.
fn health_response() -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let body = br#"{"status":"ok"}"#;
    build_json_response(200, body.to_vec())
}

/// Builds a JSON HTTP response with the given status and body.
fn build_json_response(status: u16, body: Vec<u8>) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    hyper::Response::builder()
        .status(status)
        .header("content-type", a2a_types::A2A_CONTENT_TYPE)
        .header(a2a_types::A2A_VERSION_HEADER, a2a_types::A2A_VERSION)
        .body(Full::new(Bytes::from(body)).boxed())
        .unwrap_or_else(|_| {
            // Fallback: plain 500 response if builder fails (should never happen
            // with valid static header names).
            hyper::Response::new(
                Full::new(Bytes::from_static(
                    br#"{"error":"response build error"}"#,
                ))
                .boxed(),
            )
        })
}

/// Parses `ListTasksParams` from URL query parameters.
fn parse_list_tasks_query(query: &str, tenant: Option<&str>) -> a2a_types::params::ListTasksParams {
    let status = parse_query_param(query, "status")
        .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok());
    a2a_types::params::ListTasksParams {
        tenant: tenant.map(str::to_owned),
        context_id: parse_query_param(query, "contextId"),
        status,
        page_size: parse_query_param_u32(query, "pageSize"),
        page_token: parse_query_param(query, "pageToken"),
        status_timestamp_after: parse_query_param(query, "statusTimestampAfter"),
        include_artifacts: parse_query_param_bool(query, "includeArtifacts"),
        history_length: parse_query_param_u32(query, "historyLength"),
    }
}
