// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC 2.0 dispatcher.
//!
//! [`JsonRpcDispatcher`] reads JSON-RPC requests from HTTP bodies, routes
//! them to the appropriate [`RequestHandler`] method, and serializes the
//! response (or streams SSE for streaming methods).

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;

use a2a_protocol_types::jsonrpc::{
    JsonRpcError, JsonRpcErrorResponse, JsonRpcId, JsonRpcRequest, JsonRpcSuccessResponse,
    JsonRpcVersion,
};

use crate::agent_card::StaticAgentCardHandler;
use crate::dispatch::cors::CorsConfig;
use crate::error::ServerError;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::serve::Dispatcher;
use crate::streaming::build_sse_response;

/// JSON-RPC 2.0 request dispatcher.
///
/// Routes incoming JSON-RPC requests to the underlying [`RequestHandler`].
/// Optionally applies CORS headers to all responses.
///
/// Also serves the agent card at `GET /.well-known/agent.json` so that
/// JSON-RPC servers can participate in agent card discovery (spec §8.3).
pub struct JsonRpcDispatcher {
    handler: Arc<RequestHandler>,
    card_handler: Option<StaticAgentCardHandler>,
    cors: Option<CorsConfig>,
    config: super::DispatchConfig,
}

impl JsonRpcDispatcher {
    /// Creates a new dispatcher wrapping the given handler with default
    /// configuration.
    #[must_use]
    pub fn new(handler: Arc<RequestHandler>) -> Self {
        Self::with_config(handler, super::DispatchConfig::default())
    }

    /// Creates a new dispatcher with the given configuration.
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

    /// Dispatches a JSON-RPC request and returns an HTTP response.
    ///
    /// For `SendStreamingMessage` and `SubscribeToTask`, the response uses
    /// SSE (`text/event-stream`). All other methods return JSON.
    ///
    /// JSON-RPC errors are always returned as HTTP 200 with an error body.
    pub async fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        // Handle CORS preflight requests.
        if req.method() == "OPTIONS" {
            if let Some(ref cors) = self.cors {
                return cors.preflight_response();
            }
            return json_response(204, Vec::new());
        }

        // Serve the agent card at the well-known discovery path (spec §8.3).
        // This must be handled before JSON-RPC body parsing since it's a GET.
        if req.method() == "GET" && req.uri().path() == "/.well-known/agent.json" {
            let mut resp = self.card_handler.as_ref().map_or_else(
                || json_response(404, br#"{"error":"agent card not configured"}"#.to_vec()),
                |h| h.handle(&req).map(http_body_util::BodyExt::boxed),
            );
            if let Some(ref cors) = self.cors {
                cors.apply_headers(&mut resp);
            }
            return resp;
        }

        let mut resp = self.dispatch_inner(req).await;
        if let Some(ref cors) = self.cors {
            cors.apply_headers(&mut resp);
        }
        resp
    }

    /// Inner dispatch logic (separated to allow CORS wrapping).
    #[allow(clippy::too_many_lines)]
    async fn dispatch_inner(
        &self,
        req: hyper::Request<Incoming>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        // Validate Content-Type if present.
        if let Some(ct) = req.headers().get("content-type") {
            let ct_str = ct.to_str().unwrap_or("");
            if !ct_str.starts_with("application/json")
                && !ct_str.starts_with(a2a_protocol_types::A2A_CONTENT_TYPE)
            {
                return parse_error_response(
                    None,
                    &format!("unsupported Content-Type: {ct_str}; expected application/json or application/a2a+json"),
                );
            }
        }

        // Extract HTTP headers BEFORE consuming the body.
        let headers = extract_headers(req.headers());

        // Read body with size limit (default 4 MiB).
        let body_bytes = match read_body_limited(
            req.into_body(),
            self.config.max_request_body_size,
            self.config.body_read_timeout,
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(msg) => return parse_error_response(None, &msg),
        };

        // JSON-RPC 2.0 §6.3: detect batch (array) vs single (object) request.
        let raw: serde_json::Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => return parse_error_response(None, &e.to_string()),
        };

        if let Some(items) = raw.as_array() {
            // Batch request: dispatch each element, collect responses.
            if items.is_empty() {
                return parse_error_response(None, "empty batch request");
            }
            let mut responses: Vec<serde_json::Value> = Vec::with_capacity(items.len());
            for item in items {
                let rpc_req: JsonRpcRequest = match serde_json::from_value(item.clone()) {
                    Ok(r) => r,
                    Err(e) => {
                        // Invalid request within batch — return individual parse error.
                        let err_resp = JsonRpcErrorResponse::new(
                            None,
                            JsonRpcError::new(
                                a2a_protocol_types::error::ErrorCode::ParseError.as_i32(),
                                format!("Parse error: {e}"),
                            ),
                        );
                        if let Ok(v) = serde_json::to_value(&err_resp) {
                            responses.push(v);
                        }
                        continue;
                    }
                };
                let resp_body = self.dispatch_single_request(&rpc_req, &headers).await;
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&resp_body) {
                    responses.push(v);
                }
            }
            let body = serde_json::to_vec(&responses).unwrap_or_default();
            json_response(200, body)
        } else {
            // Single request.
            let rpc_req: JsonRpcRequest = match serde_json::from_value(raw) {
                Ok(r) => r,
                Err(e) => return parse_error_response(None, &e.to_string()),
            };
            self.dispatch_single_request_http(&rpc_req, &headers).await
        }
    }

    /// Dispatches a single JSON-RPC request and returns an HTTP response.
    ///
    /// For streaming methods, the response is SSE. For non-streaming, JSON.
    #[allow(clippy::too_many_lines)]
    async fn dispatch_single_request_http(
        &self,
        rpc_req: &JsonRpcRequest,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let id = rpc_req.id.clone();
        trace_info!(method = %rpc_req.method, "dispatching JSON-RPC request");

        // Streaming methods return SSE, not JSON.
        match rpc_req.method.as_str() {
            "SendStreamingMessage" => {
                return self.dispatch_send_message(id, rpc_req, true, headers).await;
            }
            "SubscribeToTask" => {
                return match parse_params::<a2a_protocol_types::params::TaskIdParams>(rpc_req) {
                    Ok(p) => match self.handler.on_resubscribe(p, Some(headers)).await {
                        Ok(reader) => build_sse_response(
                            reader,
                            Some(self.config.sse_keep_alive_interval),
                            Some(self.config.sse_channel_capacity),
                        ),
                        Err(e) => error_response(id, &e),
                    },
                    Err(e) => error_response(id, &e),
                };
            }
            _ => {}
        }

        let body = self.dispatch_single_request(rpc_req, headers).await;
        json_response(200, body)
    }

    /// Dispatches a single JSON-RPC request and returns the response body bytes.
    ///
    /// Used for both single and batch requests.
    #[allow(clippy::too_many_lines)]
    async fn dispatch_single_request(
        &self,
        rpc_req: &JsonRpcRequest,
        headers: &HashMap<String, String>,
    ) -> Vec<u8> {
        let id = rpc_req.id.clone();

        match rpc_req.method.as_str() {
            "SendMessage" => {
                match self
                    .dispatch_send_message_inner(id.clone(), rpc_req, false, headers)
                    .await
                {
                    Ok(resp) => serde_json::to_vec(&resp).unwrap_or_default(),
                    Err(body) => body,
                }
            }
            "SendStreamingMessage" => {
                // In batch context, streaming is not supported — return error.
                let err = ServerError::InvalidParams(
                    "SendStreamingMessage not supported in batch requests".into(),
                );
                let a2a_err = err.to_a2a_error();
                let resp = JsonRpcErrorResponse::new(
                    id,
                    JsonRpcError::new(a2a_err.code.as_i32(), a2a_err.message),
                );
                serde_json::to_vec(&resp).unwrap_or_default()
            }
            "GetTask" => {
                match parse_params::<a2a_protocol_types::params::TaskQueryParams>(rpc_req) {
                    Ok(p) => match self.handler.on_get_task(p, Some(headers)).await {
                        Ok(r) => success_response_bytes(id, &r),
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "ListTasks" => {
                match parse_params::<a2a_protocol_types::params::ListTasksParams>(rpc_req) {
                    Ok(p) => match self.handler.on_list_tasks(p, Some(headers)).await {
                        Ok(r) => success_response_bytes(id, &r),
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "CancelTask" => {
                match parse_params::<a2a_protocol_types::params::CancelTaskParams>(rpc_req) {
                    Ok(p) => match self.handler.on_cancel_task(p, Some(headers)).await {
                        Ok(r) => success_response_bytes(id, &r),
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "SubscribeToTask" => {
                let err = ServerError::InvalidParams(
                    "SubscribeToTask not supported in batch requests".into(),
                );
                error_response_bytes(id, &err)
            }
            "CreateTaskPushNotificationConfig" => {
                match parse_params::<a2a_protocol_types::push::TaskPushNotificationConfig>(rpc_req)
                {
                    Ok(p) => match self.handler.on_set_push_config(p, Some(headers)).await {
                        Ok(r) => success_response_bytes(id, &r),
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "GetTaskPushNotificationConfig" => {
                match parse_params::<a2a_protocol_types::params::GetPushConfigParams>(rpc_req) {
                    Ok(p) => match self.handler.on_get_push_config(p, Some(headers)).await {
                        Ok(r) => success_response_bytes(id, &r),
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "ListTaskPushNotificationConfigs" => {
                match parse_params::<a2a_protocol_types::params::ListPushConfigsParams>(rpc_req) {
                    Ok(p) => match self
                        .handler
                        .on_list_push_configs(&p.task_id, p.tenant.as_deref(), Some(headers))
                        .await
                    {
                        Ok(configs) => {
                            let resp = a2a_protocol_types::responses::ListPushConfigsResponse {
                                configs,
                                next_page_token: None,
                            };
                            success_response_bytes(id, &resp)
                        }
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "DeleteTaskPushNotificationConfig" => {
                match parse_params::<a2a_protocol_types::params::DeletePushConfigParams>(rpc_req) {
                    Ok(p) => match self.handler.on_delete_push_config(p, Some(headers)).await {
                        Ok(()) => success_response_bytes(id, &serde_json::json!({})),
                        Err(e) => error_response_bytes(id, &e),
                    },
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            "GetExtendedAgentCard" => {
                match self.handler.on_get_extended_agent_card(Some(headers)).await {
                    Ok(r) => success_response_bytes(id, &r),
                    Err(e) => error_response_bytes(id, &e),
                }
            }
            other => {
                let err = ServerError::MethodNotFound(other.to_owned());
                error_response_bytes(id, &err)
            }
        }
    }

    /// Helper for dispatching `SendMessage` that returns either a success response
    /// value (for batch) or the body bytes on error.
    async fn dispatch_send_message_inner(
        &self,
        id: JsonRpcId,
        rpc_req: &JsonRpcRequest,
        streaming: bool,
        headers: &HashMap<String, String>,
    ) -> Result<JsonRpcSuccessResponse<serde_json::Value>, Vec<u8>> {
        let params = match parse_params::<a2a_protocol_types::params::MessageSendParams>(rpc_req) {
            Ok(p) => p,
            Err(e) => return Err(error_response_bytes(id, &e)),
        };
        match self
            .handler
            .on_send_message(params, streaming, Some(headers))
            .await
        {
            Ok(SendMessageResult::Response(resp)) => {
                let result = serde_json::to_value(&resp).unwrap_or(serde_json::Value::Null);
                Ok(JsonRpcSuccessResponse {
                    jsonrpc: JsonRpcVersion,
                    id,
                    result,
                })
            }
            Ok(SendMessageResult::Stream(_)) => {
                // Shouldn't happen in non-streaming mode.
                let err = ServerError::Internal("unexpected stream response".into());
                Err(error_response_bytes(id, &err))
            }
            Err(e) => Err(error_response_bytes(id, &e)),
        }
    }

    async fn dispatch_send_message(
        &self,
        id: JsonRpcId,
        rpc_req: &JsonRpcRequest,
        streaming: bool,
        headers: &HashMap<String, String>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = match parse_params::<a2a_protocol_types::params::MessageSendParams>(rpc_req) {
            Ok(p) => p,
            Err(e) => return error_response(id, &e),
        };
        match self
            .handler
            .on_send_message(params, streaming, Some(headers))
            .await
        {
            Ok(SendMessageResult::Response(resp)) => success_response(id, &resp),
            Ok(SendMessageResult::Stream(reader)) => build_sse_response(
                reader,
                Some(self.config.sse_keep_alive_interval),
                Some(self.config.sse_channel_capacity),
            ),
            Err(e) => error_response(id, &e),
        }
    }
}

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

/// Serializes a success response to bytes (for batch request support).
fn success_response_bytes<T: serde::Serialize>(id: JsonRpcId, result: &T) -> Vec<u8> {
    let resp = JsonRpcSuccessResponse {
        jsonrpc: JsonRpcVersion,
        id,
        result: serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
    };
    serde_json::to_vec(&resp).unwrap_or_default()
}

/// Serializes an error response to bytes (for batch request support).
fn error_response_bytes(id: JsonRpcId, err: &ServerError) -> Vec<u8> {
    let a2a_err = err.to_a2a_error();
    let resp = JsonRpcErrorResponse::new(
        id,
        JsonRpcError::new(a2a_err.code.as_i32(), a2a_err.message),
    );
    serde_json::to_vec(&resp).unwrap_or_default()
}

impl std::fmt::Debug for JsonRpcDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonRpcDispatcher").finish()
    }
}

// ── Free functions ───────────────────────────────────────────────────────────

fn parse_params<T: serde::de::DeserializeOwned>(
    rpc_req: &JsonRpcRequest,
) -> Result<T, ServerError> {
    let params = rpc_req
        .params
        .as_ref()
        .ok_or_else(|| ServerError::InvalidParams("missing params".into()))?;
    serde_json::from_value(params.clone())
        .map_err(|e| ServerError::InvalidParams(format!("invalid params: {e}")))
}

fn success_response<T: serde::Serialize>(
    id: JsonRpcId,
    result: &T,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let resp = JsonRpcSuccessResponse {
        jsonrpc: JsonRpcVersion,
        id: id.clone(),
        result: serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
    };
    match serde_json::to_vec(&resp) {
        Ok(body) => json_response(200, body),
        Err(e) => internal_serialization_error(id, &e),
    }
}

fn error_response(id: JsonRpcId, err: &ServerError) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let a2a_err = err.to_a2a_error();
    let resp = JsonRpcErrorResponse::new(
        id.clone(),
        JsonRpcError::new(a2a_err.code.as_i32(), a2a_err.message),
    );
    match serde_json::to_vec(&resp) {
        Ok(body) => json_response(200, body),
        Err(e) => internal_serialization_error(id, &e),
    }
}

fn parse_error_response(
    id: JsonRpcId,
    message: &str,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let resp = JsonRpcErrorResponse::new(
        id.clone(),
        JsonRpcError::new(
            a2a_protocol_types::error::ErrorCode::ParseError.as_i32(),
            format!("Parse error: {message}"),
        ),
    );
    match serde_json::to_vec(&resp) {
        Ok(body) => json_response(200, body),
        Err(e) => internal_serialization_error(id, &e),
    }
}

/// Fallback response when JSON-RPC serialization itself fails.
fn internal_serialization_error(
    _id: JsonRpcId,
    _err: &serde_json::Error,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    trace_error!(error = %_err, "JSON-RPC response serialization failed");
    // Hand-craft a minimal JSON-RPC error to avoid further serialization failures.
    let body = br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"internal serialization error"}}"#;
    json_response(500, body.to_vec())
}

/// Reads a request body with a size limit and timeout.
///
/// Returns an error message if the body exceeds the limit, times out, or cannot be read.
async fn read_body_limited(
    body: Incoming,
    max_size: usize,
    read_timeout: std::time::Duration,
) -> Result<Bytes, String> {
    // Check Content-Length header upfront if present.
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

/// Builds a JSON HTTP response with the given status and body.
fn json_response(status: u16, body: Vec<u8>) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    hyper::Response::builder()
        .status(status)
        .header("content-type", a2a_protocol_types::A2A_CONTENT_TYPE)
        .header(a2a_protocol_types::A2A_VERSION_HEADER, a2a_protocol_types::A2A_VERSION)
        .body(Full::new(Bytes::from(body)).boxed())
        .unwrap_or_else(|_| {
            // Fallback: plain 500 response if builder fails (should never happen
            // with valid static header names).
            hyper::Response::new(
                Full::new(Bytes::from_static(
                    br#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"response build error"}}"#,
                ))
                .boxed(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::header::HeaderValue;

    // ── extract_headers ──────────────────────────────────────────────────────

    #[test]
    fn extract_headers_lowercases_keys() {
        // hyper HeaderMap normalises keys to lowercase internally, so
        // inserting via the typed `header::AUTHORIZATION` constant gives us
        // the lower-case key "authorization".
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer token"),
        );
        let map = extract_headers(&headers);
        assert_eq!(
            map.get("authorization").map(String::as_str),
            Some("Bearer token")
        );
    }

    #[test]
    fn extract_headers_skips_non_ascii_values() {
        // Build a raw HeaderValue that contains non-UTF-8 bytes so that
        // `to_str()` returns an error and the entry should be skipped.
        let mut headers = hyper::HeaderMap::new();
        let bad_value = HeaderValue::from_bytes(b"caf\xe9").unwrap();
        headers.insert(hyper::header::X_CONTENT_TYPE_OPTIONS, bad_value);
        let map = extract_headers(&headers);
        // The entry must NOT appear in the output map.
        assert!(!map.contains_key("x-content-type-options"));
    }

    #[test]
    fn extract_headers_empty() {
        let headers = hyper::HeaderMap::new();
        let map = extract_headers(&headers);
        assert!(map.is_empty());
    }

    // ── parse_params ─────────────────────────────────────────────────────────

    #[test]
    fn parse_params_missing_returns_invalid_params() {
        use a2a_protocol_types::params::TaskQueryParams;
        let req = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: None,
            method: "GetTask".to_owned(),
            params: None,
        };
        let result: Result<TaskQueryParams, _> = parse_params(&req);
        assert!(
            result.is_err(),
            "expected error when params are missing"
        );
        let err = result.unwrap_err();
        assert!(
            matches!(err, ServerError::InvalidParams(_)),
            "expected InvalidParams, got {err:?}"
        );
    }

    #[test]
    fn parse_params_invalid_type_returns_error() {
        use a2a_protocol_types::params::TaskQueryParams;
        // TaskQueryParams expects an object with an `id` field (string).
        // Passing a bare integer should produce an InvalidParams error.
        let req = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: Some(serde_json::json!(1)),
            method: "GetTask".to_owned(),
            params: Some(serde_json::json!(42)),
        };
        let result: Result<TaskQueryParams, _> = parse_params(&req);
        assert!(result.is_err(), "expected error for wrong params type");
    }

    // ── json_response ────────────────────────────────────────────────────────

    #[test]
    fn json_response_200_status() {
        let resp = json_response(200, b"{}".to_vec());
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[test]
    fn json_response_404_status() {
        let resp = json_response(404, b"not found".to_vec());
        assert_eq!(resp.status().as_u16(), 404);
    }

    // ── parse_error_response ─────────────────────────────────────────────────

    #[test]
    fn parse_error_response_returns_200_with_error_body() {
        let resp = parse_error_response(None, "bad json");
        assert_eq!(resp.status().as_u16(), 200);
    }

    #[tokio::test]
    async fn parse_error_response_has_error_code() {
        let resp = parse_error_response(None, "something went wrong");
        use http_body_util::BodyExt;
        let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let val: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        // JSON-RPC parse error code is -32700.
        assert_eq!(val["error"]["code"], -32700);
        assert!(val["error"]["message"].is_string());
    }

    // ── success_response_bytes ───────────────────────────────────────────────

    #[test]
    fn success_response_bytes_structure() {
        let id: JsonRpcId = Some(serde_json::json!(1));
        let bytes = success_response_bytes(id, &serde_json::json!({"key": "val"}));
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["result"]["key"], "val");
        assert_eq!(val["id"], 1);
    }

    #[test]
    fn success_response_includes_jsonrpc_version() {
        let id: JsonRpcId = Some(serde_json::json!(42));
        let bytes = success_response_bytes(id, &serde_json::json!(null));
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["jsonrpc"], "2.0");
    }

    // ── error_response_bytes ─────────────────────────────────────────────────

    #[test]
    fn error_response_bytes_contains_error_object() {
        let id: JsonRpcId = Some(serde_json::json!(1));
        let err = ServerError::MethodNotFound("Foo".into());
        let bytes = error_response_bytes(id, &err);
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(val["error"].is_object(), "expected 'error' key to be an object");
        assert!(val["error"]["code"].is_number());
        assert!(val["error"]["message"].is_string());
    }

    #[test]
    fn error_response_has_jsonrpc_version() {
        let id: JsonRpcId = Some(serde_json::json!(7));
        let err = ServerError::Internal("oops".into());
        let bytes = error_response_bytes(id, &err);
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["jsonrpc"], "2.0");
    }
}

// ── Dispatcher impl ──────────────────────────────────────────────────────────

impl Dispatcher for JsonRpcDispatcher {
    fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = crate::serve::DispatchResponse> + Send + '_>,
    > {
        Box::pin(self.dispatch(req))
    }
}
