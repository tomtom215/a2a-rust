// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Answering a JSON-RPC request refused before it named a method: a body
//! that is not JSON (-32700), JSON that is not a Request object (-32600),
//! or an error found before routing — each recorded as a failed call.

use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use a2a_protocol_types::error::ErrorCode;

use super::JsonRpcDispatcher;
use super::response::{error_response, invalid_request_response, parse_error_response};
use crate::error::ServerError;

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
