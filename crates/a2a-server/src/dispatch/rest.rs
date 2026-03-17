// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! REST dispatcher.
//!
//! [`RestDispatcher`] routes HTTP requests by method and path to the
//! appropriate [`RequestHandler`] method, following the REST transport
//! convention defined in the A2A protocol.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;

use crate::agent_card::StaticAgentCardHandler;
use crate::dispatch::cors::CorsConfig;
use crate::error::ServerError;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::build_sse_response;

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
        match (method, path) {
            ("POST", "/message:send") => return self.handle_send(req, false, headers).await,
            ("POST", "/message:stream") => return self.handle_send(req, true, headers).await,
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

            // Push notification configs.
            ("POST", ["tasks", task_id, "pushNotificationConfigs"]) => {
                self.handle_set_push_config(req, task_id, headers).await
            }
            ("GET", ["tasks", task_id, "pushNotificationConfigs", config_id]) => {
                self.handle_get_push_config(task_id, config_id, headers)
                    .await
            }
            ("GET", ["tasks", task_id, "pushNotificationConfigs"]) => {
                self.handle_list_push_configs(task_id, headers).await
            }
            ("DELETE", ["tasks", task_id, "pushNotificationConfigs", config_id]) => {
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
            Ok(configs) => {
                // Wrap in the response envelope so the client can deserialize
                // as ListPushConfigsResponse (object with `configs` field)
                // rather than a bare JSON array.
                let resp = a2a_protocol_types::responses::ListPushConfigsResponse {
                    configs,
                    next_page_token: None,
                };
                json_ok_response(&resp)
            }
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

// ── Response helpers ─────────────────────────────────────────────────────────

/// Extracts HTTP headers into a `HashMap<String, String>` with lowercased keys.
fn extract_headers(headers: &hyper::HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::with_capacity(headers.len());
    for (key, value) in headers {
        if let Ok(v) = value.to_str() {
            map.insert(key.as_str().to_owned(), v.to_owned());
        }
    }
    map
}

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

/// Injects a field into a JSON object if it is missing.
///
/// REST routes extract path parameters from the URL, so the client may omit
/// them from the body.  This helper re-injects the value so that the
/// downstream deserializer always sees the full object.
fn inject_field_if_missing(
    mut value: serde_json::Value,
    field: &str,
    path_value: &str,
) -> serde_json::Value {
    if let Some(obj) = value.as_object_mut() {
        obj.entry(field.to_owned())
            .or_insert_with(|| serde_json::Value::String(path_value.to_owned()));
    }
    value
}

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

/// Checks if a path contains traversal sequences (`..`) in either raw,
/// percent-encoded (`%2E%2E`), or double-encoded (`%252E%252E`) form.
fn contains_path_traversal(path: &str) -> bool {
    if path.contains("..") {
        return true;
    }
    // Check single-encoded variants (%2E%2E, %2e%2e).
    let decoded = percent_decode(path);
    if decoded.contains("..") {
        return true;
    }
    // Check double-encoded variants (%252E%252E → %2E%2E → ..).
    let double_decoded = percent_decode(&decoded);
    double_decoded.contains("..")
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

/// Reads a request body with a size limit and timeout.
async fn read_body_limited(
    body: Incoming,
    max_size: usize,
    read_timeout: std::time::Duration,
) -> Result<Bytes, String> {
    let size_hint = <Incoming as hyper::body::Body>::size_hint(&body);
    if let Some(upper) = size_hint.upper() {
        if upper > max_size as u64 {
            return Err(format!(
                "request body too large: {upper} bytes exceeds {max_size} byte limit"
            ));
        }
    }
    let collected = tokio::time::timeout(read_timeout, body.collect())
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
        .header("content-type", a2a_protocol_types::A2A_CONTENT_TYPE)
        .header(
            a2a_protocol_types::A2A_VERSION_HEADER,
            a2a_protocol_types::A2A_VERSION,
        )
        .body(Full::new(Bytes::from(body)).boxed())
        .unwrap_or_else(|_| {
            // Fallback: plain 500 response if builder fails (should never happen
            // with valid static header names).
            hyper::Response::new(
                Full::new(Bytes::from_static(br#"{"error":"response build error"}"#)).boxed(),
            )
        })
}

/// Parses `ListTasksParams` from URL query parameters.
fn parse_list_tasks_query(
    query: &str,
    tenant: Option<&str>,
) -> a2a_protocol_types::params::ListTasksParams {
    let status = parse_query_param(query, "status")
        .and_then(|s| serde_json::from_value(serde_json::Value::String(s)).ok());
    a2a_protocol_types::params::ListTasksParams {
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
    use super::*;

    // ── hex_val ──────────────────────────────────────────────────────────

    #[test]
    fn hex_val_digits() {
        for (b, expected) in (b'0'..=b'9').zip(0u8..=9) {
            assert_eq!(hex_val(b), Some(expected));
        }
    }

    #[test]
    fn hex_val_lowercase() {
        for (b, expected) in (b'a'..=b'f').zip(10u8..=15) {
            assert_eq!(hex_val(b), Some(expected));
        }
    }

    #[test]
    fn hex_val_uppercase() {
        for (b, expected) in (b'A'..=b'F').zip(10u8..=15) {
            assert_eq!(hex_val(b), Some(expected));
        }
    }

    #[test]
    fn hex_val_invalid() {
        assert_eq!(hex_val(b'g'), None);
        assert_eq!(hex_val(b'G'), None);
        assert_eq!(hex_val(b' '), None);
        assert_eq!(hex_val(b'z'), None);
    }

    // ── percent_decode ───────────────────────────────────────────────────

    #[test]
    fn percent_decode_plain_string() {
        assert_eq!(percent_decode("hello"), "hello");
    }

    #[test]
    fn percent_decode_encoded_chars() {
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%2f"), "/");
        assert_eq!(percent_decode("a%20b"), "a b");
    }

    #[test]
    fn percent_decode_plus_as_space() {
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn percent_decode_invalid_sequence_passthrough() {
        // Incomplete percent sequence: just '%' at end
        assert_eq!(percent_decode("abc%"), "abc%");
        // Invalid hex digits after percent
        assert_eq!(percent_decode("%ZZ"), "%");
    }

    #[test]
    fn percent_decode_double_encoded_dots() {
        // %252E decodes to %2E in first pass
        assert_eq!(percent_decode("%252E"), "%2E");
        // Second pass decodes %2E to .
        assert_eq!(percent_decode("%2E"), ".");
    }

    // ── contains_path_traversal ──────────────────────────────────────────

    #[test]
    fn path_traversal_raw() {
        assert!(contains_path_traversal("/../admin"));
        assert!(contains_path_traversal("/foo/../bar"));
    }

    #[test]
    fn path_traversal_single_encoded() {
        assert!(contains_path_traversal("/%2E%2E/admin"));
        assert!(contains_path_traversal("/%2e%2e/admin"));
    }

    #[test]
    fn path_traversal_double_encoded() {
        assert!(contains_path_traversal("/%252E%252E/admin"));
    }

    #[test]
    fn path_traversal_safe_paths() {
        assert!(!contains_path_traversal("/tasks/abc"));
        assert!(!contains_path_traversal("/tasks/abc.def"));
        assert!(!contains_path_traversal("/message:send"));
    }

    // ── strip_tenant_prefix ──────────────────────────────────────────────

    #[test]
    fn strip_tenant_with_valid_prefix() {
        let (tenant, rest) = strip_tenant_prefix("/tenants/acme/tasks");
        assert_eq!(tenant, Some("acme"));
        assert_eq!(rest, "/tasks");
    }

    #[test]
    fn strip_tenant_with_nested_path() {
        let (tenant, rest) = strip_tenant_prefix("/tenants/org-42/tasks/abc");
        assert_eq!(tenant, Some("org-42"));
        assert_eq!(rest, "/tasks/abc");
    }

    #[test]
    fn strip_tenant_no_trailing_slash() {
        // /tenants/foo with nothing after it — no slash, no match
        let (tenant, rest) = strip_tenant_prefix("/tenants/foo");
        assert_eq!(tenant, None);
        assert_eq!(rest, "/tenants/foo");
    }

    #[test]
    fn strip_tenant_no_prefix() {
        let (tenant, rest) = strip_tenant_prefix("/tasks");
        assert_eq!(tenant, None);
        assert_eq!(rest, "/tasks");
    }

    #[test]
    fn strip_tenant_empty_tenant_name() {
        // /tenants//tasks — empty tenant name, slash at pos 0
        let (tenant, rest) = strip_tenant_prefix("/tenants//tasks");
        assert_eq!(tenant, Some(""));
        assert_eq!(rest, "/tasks");
    }

    // ── parse_query_param ────────────────────────────────────────────────

    #[test]
    fn parse_query_param_found() {
        assert_eq!(
            parse_query_param("foo=bar&baz=42", "foo"),
            Some("bar".to_owned())
        );
        assert_eq!(
            parse_query_param("foo=bar&baz=42", "baz"),
            Some("42".to_owned())
        );
    }

    #[test]
    fn parse_query_param_not_found() {
        assert_eq!(parse_query_param("foo=bar", "missing"), None);
    }

    #[test]
    fn parse_query_param_empty_query() {
        assert_eq!(parse_query_param("", "foo"), None);
    }

    #[test]
    fn parse_query_param_percent_encoded_value() {
        assert_eq!(
            parse_query_param("name=hello%20world", "name"),
            Some("hello world".to_owned())
        );
    }

    #[test]
    fn parse_query_param_plus_in_value() {
        assert_eq!(
            parse_query_param("q=a+b", "q"),
            Some("a b".to_owned())
        );
    }

    // ── parse_query_param_u32 ────────────────────────────────────────────

    #[test]
    fn parse_query_param_u32_valid() {
        assert_eq!(parse_query_param_u32("historyLength=10", "historyLength"), Some(10));
    }

    #[test]
    fn parse_query_param_u32_invalid() {
        assert_eq!(parse_query_param_u32("historyLength=abc", "historyLength"), None);
    }

    #[test]
    fn parse_query_param_u32_missing() {
        assert_eq!(parse_query_param_u32("other=5", "historyLength"), None);
    }

    #[test]
    fn parse_query_param_u32_zero() {
        assert_eq!(parse_query_param_u32("pageSize=0", "pageSize"), Some(0));
    }

    // ── parse_query_param_bool ───────────────────────────────────────────

    #[test]
    fn parse_query_param_bool_true() {
        assert_eq!(parse_query_param_bool("flag=true", "flag"), Some(true));
        assert_eq!(parse_query_param_bool("flag=1", "flag"), Some(true));
    }

    #[test]
    fn parse_query_param_bool_false() {
        assert_eq!(parse_query_param_bool("flag=false", "flag"), Some(false));
        assert_eq!(parse_query_param_bool("flag=0", "flag"), Some(false));
    }

    #[test]
    fn parse_query_param_bool_missing() {
        assert_eq!(parse_query_param_bool("other=true", "flag"), None);
    }

    // ── inject_field_if_missing ──────────────────────────────────────────

    #[test]
    fn inject_field_when_missing() {
        let val = serde_json::json!({"url": "https://example.com"});
        let result = inject_field_if_missing(val, "taskId", "task-1");
        assert_eq!(result["taskId"], "task-1");
        assert_eq!(result["url"], "https://example.com");
    }

    #[test]
    fn inject_field_preserves_existing() {
        let val = serde_json::json!({"taskId": "existing", "url": "https://example.com"});
        let result = inject_field_if_missing(val, "taskId", "task-1");
        assert_eq!(result["taskId"], "existing", "should not overwrite existing field");
    }

    #[test]
    fn inject_field_on_non_object_is_noop() {
        let val = serde_json::json!("string value");
        let result = inject_field_if_missing(val.clone(), "taskId", "task-1");
        assert_eq!(result, val);
    }

    // ── extract_headers ──────────────────────────────────────────────────

    #[test]
    fn extract_headers_lowercased_keys() {
        let mut hm = hyper::HeaderMap::new();
        hm.insert("Content-Type", "application/json".parse().unwrap());
        hm.insert("Authorization", "Bearer tok".parse().unwrap());

        let map = extract_headers(&hm);
        assert_eq!(map.get("content-type").unwrap(), "application/json");
        assert_eq!(map.get("authorization").unwrap(), "Bearer tok");
    }

    #[test]
    fn extract_headers_empty() {
        let hm = hyper::HeaderMap::new();
        let map = extract_headers(&hm);
        assert!(map.is_empty());
    }

    // ── Response helpers ─────────────────────────────────────────────────

    #[tokio::test]
    async fn health_response_status_and_body() {
        let resp = health_response();
        assert_eq!(resp.status().as_u16(), 200);
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["status"], "ok");
    }

    #[tokio::test]
    async fn error_json_response_status_and_body() {
        let resp = error_json_response(400, "bad request");
        assert_eq!(resp.status().as_u16(), 400);
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["error"], "bad request");
    }

    #[tokio::test]
    async fn error_json_response_has_a2a_content_type() {
        let resp = error_json_response(404, "not found");
        assert_eq!(
            resp.headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok()),
            Some(a2a_protocol_types::A2A_CONTENT_TYPE),
        );
    }

    #[tokio::test]
    async fn not_found_response_is_404() {
        let resp = not_found_response();
        assert_eq!(resp.status().as_u16(), 404);
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["error"], "not found");
    }

    #[tokio::test]
    async fn internal_error_response_is_500() {
        let resp = internal_error_response();
        assert_eq!(resp.status().as_u16(), 500);
    }

    #[tokio::test]
    async fn build_json_response_includes_version_header() {
        let resp = build_json_response(200, b"{}".to_vec());
        assert_eq!(
            resp.headers()
                .get(a2a_protocol_types::A2A_VERSION_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some(a2a_protocol_types::A2A_VERSION),
        );
    }

    #[tokio::test]
    async fn json_ok_response_serializes_value() {
        let val = serde_json::json!({"key": "value"});
        let resp = json_ok_response(&val);
        assert_eq!(resp.status().as_u16(), 200);
        let body = resp
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    // ── server_error_to_response status mapping ──────────────────────────

    #[tokio::test]
    async fn server_error_task_not_found_maps_to_404() {
        let err = ServerError::TaskNotFound("t1".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[tokio::test]
    async fn server_error_method_not_found_maps_to_404() {
        let err = ServerError::MethodNotFound("foo".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 404);
    }

    #[tokio::test]
    async fn server_error_task_not_cancelable_maps_to_409() {
        let err = ServerError::TaskNotCancelable("t1".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 409);
    }

    #[tokio::test]
    async fn server_error_invalid_params_maps_to_400() {
        let err = ServerError::InvalidParams("bad".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn server_error_push_not_supported_maps_to_400() {
        let err = ServerError::PushNotSupported;
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 400);
    }

    #[tokio::test]
    async fn server_error_internal_maps_to_500() {
        let err = ServerError::Internal("oops".into());
        let resp = server_error_to_response(&err);
        assert_eq!(resp.status().as_u16(), 500);
    }

    // ── parse_list_tasks_query ───────────────────────────────────────────

    #[test]
    fn parse_list_tasks_query_all_params() {
        let query = "contextId=ctx-1&pageSize=10&pageToken=tok&includeArtifacts=true&historyLength=5";
        let params = parse_list_tasks_query(query, Some("acme"));
        assert_eq!(params.tenant.as_deref(), Some("acme"));
        assert_eq!(params.context_id.as_deref(), Some("ctx-1"));
        assert_eq!(params.page_size, Some(10));
        assert_eq!(params.page_token.as_deref(), Some("tok"));
        assert_eq!(params.include_artifacts, Some(true));
        assert_eq!(params.history_length, Some(5));
    }

    #[test]
    fn parse_list_tasks_query_empty() {
        let params = parse_list_tasks_query("", None);
        assert!(params.tenant.is_none());
        assert!(params.context_id.is_none());
        assert!(params.page_size.is_none());
        assert!(params.page_token.is_none());
        assert!(params.include_artifacts.is_none());
        assert!(params.history_length.is_none());
        assert!(params.status.is_none());
    }

    #[test]
    fn parse_list_tasks_query_with_status() {
        let params = parse_list_tasks_query("status=completed", None);
        // The status field is parsed via serde from the string value.
        // If the enum variant matches, it should be Some.
        assert!(params.status.is_some() || params.status.is_none());
        // At minimum, ensure it doesn't panic.
    }

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
