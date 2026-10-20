// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `SERVER` span a routed HTTP+JSON call runs in (ADR 0013). A request
//! refused before routing has no span; `RestDispatcher::dispatch` records it
//! (`rpc_span::record_if_refused`). Moved out of `mod.rs`, which is over the
//! 500-line limit.

use std::collections::HashMap;
use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use super::RestDispatcher;

/// What an HTTP+JSON call's span records about the HTTP request.
pub(super) struct HttpRoute<'a> {
    pub(super) method: &'a str,
    /// The path began with a tenant segment, which the route template shows.
    pub(super) tenant_in_path: bool,
}

impl RestDispatcher {
    /// Runs one routed call in its `SERVER` span, recorded with the HTTP
    /// status it answered (ADR 0013).
    ///
    /// Not `async`: an `async fn` awaiting `call` would hold that future twice,
    /// and a route's future is the largest thing a request allocates.
    pub(super) fn call(
        &self,
        http: &HttpRoute<'_>,
        method: &str,
        route: &str,
        headers: &HashMap<String, String>,
        call: impl std::future::Future<Output = hyper::Response<BoxBody<Bytes, Infallible>>>,
    ) -> impl std::future::Future<Output = hyper::Response<BoxBody<Bytes, Infallible>>> {
        let prefixed;
        let route = if http.tenant_in_path {
            prefixed = format!("/{{tenant}}{route}");
            prefixed.as_str()
        } else {
            route
        };
        crate::rpc_span::ServerSpan::open(
            &self.handler,
            crate::rpc_span::RpcSystem::HttpJson,
            method,
            Some(headers),
        )
        .with_http(http.method, route)
        .run_response(call)
    }
}
