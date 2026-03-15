// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! JSON-RPC 2.0 dispatcher.
//!
//! [`JsonRpcDispatcher`] reads JSON-RPC requests from HTTP bodies, routes
//! them to the appropriate [`RequestHandler`] method, and serializes the
//! response (or streams SSE for streaming methods).

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;

use a2a_types::jsonrpc::{
    JsonRpcError, JsonRpcErrorResponse, JsonRpcId, JsonRpcRequest, JsonRpcSuccessResponse,
    JsonRpcVersion,
};

use crate::error::ServerError;
use crate::executor::AgentExecutor;
use crate::handler::{RequestHandler, SendMessageResult};
use crate::streaming::build_sse_response;

/// JSON-RPC 2.0 request dispatcher.
///
/// Routes incoming JSON-RPC requests to the underlying [`RequestHandler`].
pub struct JsonRpcDispatcher<E: AgentExecutor> {
    handler: Arc<RequestHandler<E>>,
}

impl<E: AgentExecutor> JsonRpcDispatcher<E> {
    /// Creates a new dispatcher wrapping the given handler.
    #[must_use]
    pub const fn new(handler: Arc<RequestHandler<E>>) -> Self {
        Self { handler }
    }

    /// Dispatches a JSON-RPC request and returns an HTTP response.
    ///
    /// For `SendStreamingMessage` and `SubscribeToTask`, the response uses
    /// SSE (`text/event-stream`). All other methods return JSON.
    ///
    /// JSON-RPC errors are always returned as HTTP 200 with an error body.
    #[allow(clippy::too_many_lines)]
    pub async fn dispatch(
        &self,
        req: hyper::Request<Incoming>,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        // Validate Content-Type if present.
        if let Some(ct) = req.headers().get("content-type") {
            let ct_str = ct.to_str().unwrap_or("");
            if !ct_str.starts_with("application/json")
                && !ct_str.starts_with(a2a_types::A2A_CONTENT_TYPE)
            {
                return parse_error_response(
                    None,
                    &format!("unsupported Content-Type: {ct_str}; expected application/json or application/a2a+json"),
                );
            }
        }

        // Read body with size limit (default 4 MiB).
        let body_bytes = match read_body_limited(req.into_body(), MAX_REQUEST_BODY_SIZE).await {
            Ok(bytes) => bytes,
            Err(msg) => return parse_error_response(None, &msg),
        };

        // Deserialize JSON-RPC request.
        let rpc_req: JsonRpcRequest = match serde_json::from_slice(&body_bytes) {
            Ok(r) => r,
            Err(e) => return parse_error_response(None, &e.to_string()),
        };

        let id = rpc_req.id.clone();
        trace_info!(method = %rpc_req.method, "dispatching JSON-RPC request");

        match rpc_req.method.as_str() {
            "SendMessage" => self.dispatch_send_message(id, &rpc_req, false).await,
            "SendStreamingMessage" => self.dispatch_send_message(id, &rpc_req, true).await,
            "GetTask" => match parse_params::<a2a_types::params::TaskQueryParams>(&rpc_req) {
                Ok(p) => match self.handler.on_get_task(p).await {
                    Ok(r) => success_response(id, &r),
                    Err(e) => error_response(id, &e),
                },
                Err(e) => error_response(id, &e),
            },
            "ListTasks" => match parse_params::<a2a_types::params::ListTasksParams>(&rpc_req) {
                Ok(p) => match self.handler.on_list_tasks(p).await {
                    Ok(r) => success_response(id, &r),
                    Err(e) => error_response(id, &e),
                },
                Err(e) => error_response(id, &e),
            },
            "CancelTask" => match parse_params::<a2a_types::params::CancelTaskParams>(&rpc_req) {
                Ok(p) => match self.handler.on_cancel_task(p).await {
                    Ok(r) => success_response(id, &r),
                    Err(e) => error_response(id, &e),
                },
                Err(e) => error_response(id, &e),
            },
            "SubscribeToTask" => match parse_params::<a2a_types::params::TaskIdParams>(&rpc_req) {
                Ok(p) => match self.handler.on_resubscribe(p).await {
                    Ok(reader) => build_sse_response(reader, None),
                    Err(e) => error_response(id, &e),
                },
                Err(e) => error_response(id, &e),
            },
            "CreateTaskPushNotificationConfig" => {
                match parse_params::<a2a_types::push::TaskPushNotificationConfig>(&rpc_req) {
                    Ok(p) => match self.handler.on_set_push_config(p).await {
                        Ok(r) => success_response(id, &r),
                        Err(e) => error_response(id, &e),
                    },
                    Err(e) => error_response(id, &e),
                }
            }
            "GetTaskPushNotificationConfig" => {
                match parse_params::<a2a_types::params::GetPushConfigParams>(&rpc_req) {
                    Ok(p) => match self.handler.on_get_push_config(p).await {
                        Ok(r) => success_response(id, &r),
                        Err(e) => error_response(id, &e),
                    },
                    Err(e) => error_response(id, &e),
                }
            }
            "ListTaskPushNotificationConfigs" => {
                match parse_params::<a2a_types::params::TaskIdParams>(&rpc_req) {
                    Ok(p) => match self.handler.on_list_push_configs(&p.id).await {
                        Ok(r) => success_response(id, &r),
                        Err(e) => error_response(id, &e),
                    },
                    Err(e) => error_response(id, &e),
                }
            }
            "DeleteTaskPushNotificationConfig" => {
                match parse_params::<a2a_types::params::DeletePushConfigParams>(&rpc_req) {
                    Ok(p) => match self.handler.on_delete_push_config(p).await {
                        Ok(()) => success_response(id, &serde_json::json!({})),
                        Err(e) => error_response(id, &e),
                    },
                    Err(e) => error_response(id, &e),
                }
            }
            "GetExtendedAgentCard" => match self.handler.on_get_extended_agent_card().await {
                Ok(r) => success_response(id, &r),
                Err(e) => error_response(id, &e),
            },
            other => {
                let err = ServerError::MethodNotFound(other.to_owned());
                error_response(id, &err)
            }
        }
    }

    async fn dispatch_send_message(
        &self,
        id: JsonRpcId,
        rpc_req: &JsonRpcRequest,
        streaming: bool,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let params = match parse_params::<a2a_types::params::MessageSendParams>(rpc_req) {
            Ok(p) => p,
            Err(e) => return error_response(id, &e),
        };
        match self.handler.on_send_message(params, streaming).await {
            Ok(SendMessageResult::Response(resp)) => success_response(id, &resp),
            Ok(SendMessageResult::Stream(reader)) => build_sse_response(reader, None),
            Err(e) => error_response(id, &e),
        }
    }
}

impl<E: AgentExecutor> std::fmt::Debug for JsonRpcDispatcher<E> {
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
        id,
        result: serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
    };
    let body = serde_json::to_vec(&resp).unwrap_or_default();
    json_response(200, body)
}

fn error_response(id: JsonRpcId, err: &ServerError) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let a2a_err = err.to_a2a_error();
    let resp = JsonRpcErrorResponse::new(
        id,
        JsonRpcError::new(a2a_err.code.as_i32(), a2a_err.message),
    );
    let body = serde_json::to_vec(&resp).unwrap_or_default();
    json_response(200, body)
}

fn parse_error_response(
    id: JsonRpcId,
    message: &str,
) -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let resp = JsonRpcErrorResponse::new(
        id,
        JsonRpcError::new(
            a2a_types::error::ErrorCode::ParseError.as_i32(),
            format!("Parse error: {message}"),
        ),
    );
    let body = serde_json::to_vec(&resp).unwrap_or_default();
    json_response(200, body)
}

/// Maximum request body size in bytes (4 MiB).
const MAX_REQUEST_BODY_SIZE: usize = 4 * 1024 * 1024;

/// Reads a request body with a size limit.
///
/// Returns an error message if the body exceeds the limit or cannot be read.
async fn read_body_limited(body: Incoming, max_size: usize) -> Result<Bytes, String> {
    // Check Content-Length header upfront if present.
    let size_hint = <Incoming as hyper::body::Body>::size_hint(&body);
    if let Some(upper) = size_hint.upper() {
        if upper > max_size as u64 {
            return Err(format!(
                "request body too large: {upper} bytes exceeds {max_size} byte limit"
            ));
        }
    }

    let collected = body.collect().await.map_err(|e| e.to_string())?;
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
        .header("content-type", a2a_types::A2A_CONTENT_TYPE)
        .header(a2a_types::A2A_VERSION_HEADER, a2a_types::A2A_VERSION)
        .body(Full::new(Bytes::from(body)).boxed())
        .expect("response builder should not fail with valid headers")
}
