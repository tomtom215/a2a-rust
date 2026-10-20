// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reading a frame's `params`, which happens before the handler — and so
//! before the interceptor chain — sees the call. Moved out of
//! `websocket.rs`, which is over the 500-line limit.
//!
//! Until 2026-10-20 a call whose params did not parse was answered `-32602`
//! there and then, so a caller with no credential learned which field it got
//! wrong where a well-formed call was refused (`docs/handoff.md`, "Open
//! engineering"), as over JSON-RPC until that morning. Such a call is now
//! admitted first (`RequestHandler::refuse_unreadable`), per frame, since a
//! frame is a call, and answered `-32602` only when the chain admits it.
//! A frame that is not JSON (`-32700`) or not a request (`-32600`) names no
//! method to admit it as, and is answered as before, as over JSON-RPC.

use std::collections::HashMap;

use a2a_protocol_types::error::A2aError;
use a2a_protocol_types::jsonrpc::JsonRpcRequest;

use crate::error::ServerError;
use crate::handler::RequestHandler;

/// A call's params as the frame carried them, with what it takes to admit
/// the call when they do not parse.
pub(super) struct Unread<'a> {
    handler: &'a RequestHandler,
    rpc_req: &'a JsonRpcRequest,
    headers: &'a HashMap<String, String>,
}

impl<'a> Unread<'a> {
    pub(super) const fn new(
        handler: &'a RequestHandler,
        rpc_req: &'a JsonRpcRequest,
        headers: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            handler,
            rpc_req,
            headers,
        }
    }

    /// The params, or the error to answer the call with: `InvalidParams`
    /// with a `google.rpc.BadRequest` naming the field (§9.5), as on the
    /// HTTP binding, once the interceptor chain admits the caller, and the
    /// chain's refusal when it does not.
    pub(super) async fn read<T: serde::de::DeserializeOwned>(self) -> Result<T, A2aError> {
        parse_params(self.handler, self.rpc_req, self.headers)
            .await
            .map_err(|e| e.to_a2a_error())
    }
}

/// [`read_params`], with the interceptor chain's refusal in place of the
/// parse error when the chain does not admit the caller.
pub(super) async fn parse_params<T: serde::de::DeserializeOwned>(
    handler: &RequestHandler,
    rpc_req: &JsonRpcRequest,
    headers: &HashMap<String, String>,
) -> Result<T, ServerError> {
    match read_params(rpc_req.params.as_ref()) {
        Ok(params) => Ok(params),
        Err(unreadable) => Err(handler
            .refuse_unreadable(&rpc_req.method, Some(headers), unreadable)
            .await),
    }
}

/// Parses params from an optional JSON value, absent read as `{}` (JSON-RPC
/// 2.0 §4.2: `params` "MAY be omitted"), as over HTTP.
fn read_params<T: serde::de::DeserializeOwned>(
    params: Option<&serde_json::Value>,
) -> Result<T, ServerError> {
    let empty = serde_json::Value::Object(serde_json::Map::new());
    let value = params.unwrap_or(&empty);
    T::deserialize(value).map_err(|e| crate::streaming::sse::invalid_params_for::<T>(value, &e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_params_with_valid_json() {
        let value = Some(serde_json::json!({"id": "task-1"}));
        let result: Result<a2a_protocol_types::params::TaskQueryParams, _> =
            read_params(value.as_ref());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "task-1");
    }

    #[test]
    fn parse_params_with_none_returns_error() {
        let result: Result<a2a_protocol_types::params::TaskQueryParams, _> = read_params(None);
        assert!(result.is_err());
    }

    #[test]
    fn parse_params_with_wrong_type_returns_error() {
        let value = Some(serde_json::json!("not an object"));
        let result: Result<a2a_protocol_types::params::TaskQueryParams, _> =
            read_params(value.as_ref());
        assert!(result.is_err());
    }
}
