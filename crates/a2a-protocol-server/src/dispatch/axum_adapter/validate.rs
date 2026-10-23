// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The protocol checks the axum router makes before routing, and the record
//! of each request it refuses there. Moved out of `axum_adapter.rs`, which is
//! over the 500-line limit.

use std::time::Instant;

use axum::extract::State;

use super::{A2aState, handler_error_to_response};
use crate::rpc_span::{RpcSystem, record_if_refused, refused};

/// The protocol checks `RestDispatcher` makes before routing, made here too.
///
/// Until 2026-10-10 this router made neither: it served `A2A-Version: 2.0`
/// (and an absent version) with 1.0 semantics, against §3.6.2's MUST to
/// answer `VersionNotSupportedError`, and it accepted any request media type.
/// Discovery and the probes stay versionless, exactly as on the REST
/// dispatcher, so a client can read the card before it knows what to send.
///
/// A request refused here, or by a route that matches no A2A method, is
/// recorded in `rpc.server.call.duration` with the status it was answered,
/// as `RestDispatcher` records its own (`rpc_span::refusal`); until
/// 2026-10-20 neither was recorded.
pub(super) async fn validate_a2a_request(
    State(state): State<A2aState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let started = Instant::now();
    let mut resp = match refusal(&req, state.config.require_version_header) {
        Some(resp) => refused(resp),
        None => next.run(req).await,
    };
    record_if_refused(&state.handler, RpcSystem::HttpJson, started, &mut resp);
    resp
}

/// The response refusing `req` before routing, if it is refused.
fn refusal(
    req: &axum::extract::Request,
    require_version: bool,
) -> Option<axum::response::Response> {
    let path = req.uri().path();
    // The card's path is versionless for every method, so a method it does
    // not perform is its `405` with `Allow`, as on the other listeners, not a
    // version or media-type error.
    let versionless = path == "/.well-known/agent-card.json"
        || matches!(
            *req.method(),
            axum::http::Method::GET | axum::http::Method::HEAD
        ) && matches!(path, "/health" | "/ready");
    if versionless {
        return None;
    }
    if let Err(err) = super::super::check_request_content_type(req.method(), req.headers()) {
        return Some(handler_error_to_response(
            &crate::error::ServerError::Protocol(err),
        ));
    }
    let version = super::super::requested_version(req.headers(), req.uri());
    super::super::validate_version_header(version.as_deref(), require_version)
        .err()
        .map(|err| handler_error_to_response(&crate::error::ServerError::Protocol(err)))
}
