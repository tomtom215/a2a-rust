// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! JSON-RPC 2.0 notifications: a Request object with no `id` member. "The
//! Server MUST NOT reply to a Notification, including those that are within
//! a batch request" (JSON-RPC 2.0 §4.1, which A2A §9 requires). Until
//! 2026-10-14 each was executed and answered with `"id": null` (audit
//! register R21). An explicit `"id": null` is a call, and is answered.

use std::collections::HashMap;
use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Empty};
use serde::Deserialize;

use a2a_protocol_types::jsonrpc::JsonRpcRequest;

use super::JsonRpcDispatcher;

impl JsonRpcDispatcher {
    /// Runs a notification and discards its outcome, which is still recorded
    /// on its span. It takes the batch path, so a streaming method is refused
    /// rather than run: a stream is the only answer it gives.
    pub(super) async fn notify(&self, rpc_req: &JsonRpcRequest, headers: &HashMap<String, String>) {
        let _ = self
            .rpc_span(rpc_req, headers)
            .run(self.call(None, rpc_req, headers))
            .await;
    }
}

/// Whether `request` is a notification: a valid Request object, as the
/// dispatcher parses one, with no `id` member.
pub(super) fn is_notification(request: &serde_json::Value) -> bool {
    JsonRpcRequest::deserialize(request).is_ok_and(|r| r.id.is_absent())
}

/// The HTTP answer to a body that holds only notifications: there is no
/// JSON-RPC response to send.
pub(super) fn no_content() -> hyper::Response<BoxBody<Bytes, Infallible>> {
    let mut resp = hyper::Response::new(Empty::new().boxed());
    *resp.status_mut() = hyper::StatusCode::NO_CONTENT;
    resp.headers_mut().insert(
        a2a_protocol_types::A2A_VERSION_HEADER,
        hyper::header::HeaderValue::from_static(a2a_protocol_types::A2A_VERSION),
    );
    resp
}
