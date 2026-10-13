// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Answering a JSON-RPC request refused before it named a method: a body
//! that is not JSON (-32700), JSON that is not a Request object (-32600),
//! or an error found before routing, such as an unsupported `Content-Type`
//! or `A2A-Version` — each recorded as a failed call.

use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use a2a_protocol_types::error::{A2aError, ErrorCode};
use a2a_protocol_types::jsonrpc::JsonRpcErrorResponse;

use super::JsonRpcDispatcher;
use super::response::{
    error_response, invalid_request_response, json_response, parse_error_response,
};
use crate::error::ServerError;

/// The refusal of a `Content-Type` that is not JSON, if `headers` name one.
///
/// Spec §5.4 maps an unsupported media type to `ContentTypeNotSupportedError`
/// (-32005), not `ParseError` (-32700): the body was never parsed, so "parse
/// error" both misreports the cause and denies the client the
/// machine-readable `CONTENT_TYPE_NOT_SUPPORTED` reason. Answered through
/// `error_response`, it carries the §10.6 `ErrorInfo` detail like every
/// other A2A error.
pub(super) fn unsupported_content_type(headers: &hyper::HeaderMap) -> Option<ServerError> {
    let ct = headers.get("content-type")?.to_str().unwrap_or("");
    let json =
        ct.starts_with("application/json") || ct.starts_with(a2a_protocol_types::A2A_CONTENT_TYPE);
    (!json).then(|| {
        ServerError::Protocol(A2aError::content_type_not_supported(format!(
            "unsupported Content-Type: {ct}; expected application/json or application/a2a+json"
        )))
    })
}

impl JsonRpcDispatcher {
    /// Answers a request refused before it named a method, and records it
    /// as a failed call (audit O11).
    pub(super) fn refuse(
        &self,
        started: std::time::Instant,
        err: &ServerError,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        self.record_unrouted(started, err.to_a2a_error().code);
        error_response(None, err)
    }

    /// [`refuse`](Self::refuse) for a request refused on its headers (its
    /// `Content-Type` or `A2A-Version`), answered with the id of each
    /// request `body` holds: JSON-RPC 2.0 §5 has the response id be the
    /// request's, null only when it cannot be read, and a batch gets one
    /// error per request. Until 2026-10-13 both were refused before the
    /// body was read, so every such refusal answered null and a batch one
    /// object. A batch beyond `max_batch_size` still gets one, as it would
    /// with a supported version: one answer per item would let a body of
    /// `{}`s buy a response 94 times its size.
    pub(super) fn refuse_with_ids(
        &self,
        started: std::time::Instant,
        body: &[u8],
        err: &ServerError,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let id_of = |request: &serde_json::Value| request.get("id").cloned();
        match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(serde_json::Value::Array(batch))
                if !batch.is_empty() && batch.len() <= self.config.max_batch_size =>
            {
                let error = err.to_a2a_error();
                let answers: Vec<_> = batch
                    .iter()
                    .map(|request| {
                        self.record_unrouted(started, error.code);
                        JsonRpcErrorResponse::new(
                            id_of(request),
                            crate::streaming::sse::jsonrpc_error_from(&error),
                        )
                    })
                    .collect();
                json_response(200, serde_json::to_vec(&answers).unwrap_or_default())
            }
            Ok(request) => {
                self.record_unrouted(started, err.to_a2a_error().code);
                error_response(id_of(&request), err)
            }
            Err(_) => self.refuse(started, err),
        }
    }

    /// [`refuse`](Self::refuse) for a body that is not a JSON-RPC request.
    pub(super) fn refuse_unparsed(
        &self,
        started: std::time::Instant,
        message: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        self.record_unrouted(started, ErrorCode::ParseError);
        parse_error_response(None, message)
    }

    /// [`refuse`](Self::refuse) for a body that is JSON but not a valid
    /// Request object: JSON-RPC 2.0's Invalid Request (-32600), where
    /// [`refuse_unparsed`](Self::refuse_unparsed) is for a body that is not
    /// JSON at all. Until 2026-09-25 every such body was answered -32700.
    pub(super) fn refuse_invalid(
        &self,
        started: std::time::Instant,
        message: &str,
    ) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        self.record_unrouted(started, ErrorCode::InvalidRequest);
        invalid_request_response(None, message)
    }

    pub(super) fn record_unrouted(&self, started: std::time::Instant, code: ErrorCode) {
        crate::rpc_span::record_unrouted(
            &self.handler,
            crate::rpc_span::RpcSystem::JsonRpc,
            started,
            &code.as_i32().to_string(),
        );
    }
}
