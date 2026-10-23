// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Requests a binding refuses before a call's span opens — an unsupported
//! `A2A-Version`, a body or request that does not decode, a content type,
//! a path no route serves — recorded in `rpc.server.call.duration` with the
//! status they were answered with, so a client sending every request with a
//! stale version shows up as what it is rather than as silence. No span: the
//! peer's trace context has not been read, and for most of these there is
//! no method to name one for.
//!
//! The JSON-RPC and WebSocket bindings have recorded theirs since ADR 0013;
//! gRPC, HTTP+JSON and the axum router have since 2026-10-20.

use std::time::Instant;

use super::RpcSystem;
use crate::handler::RequestHandler;
use crate::metrics::RpcCall;

/// Records a request refused before it named a method — a body that is not
/// JSON-RPC, a batch over the limit, an unsupported content type — as a
/// failed call without `rpc.method`, which the conventions require only "if
/// available".
pub fn record_unrouted(
    handler: &RequestHandler,
    system: RpcSystem,
    started: Instant,
    status: &str,
) {
    record(handler, system, None, started, status);
}

/// Records a call of `method` refused before its span opened: one whose
/// binding knows the method from the route, as gRPC does, but refused the
/// request's metadata or body.
#[cfg(feature = "grpc")]
pub fn record_refused(
    handler: &RequestHandler,
    system: RpcSystem,
    method: &str,
    started: Instant,
    status: &str,
) {
    record(
        handler,
        system,
        Some(super::a2a_method(method)),
        started,
        status,
    );
}

fn record(
    handler: &RequestHandler,
    system: RpcSystem,
    method: Option<&str>,
    started: Instant,
    status: &str,
) {
    handler.metrics.on_rpc_call(&RpcCall {
        system: system.name(),
        method,
        duration: started.elapsed(),
        status_code: Some(status),
        error_type: system.is_server_error(status).then_some(status),
    });
}

/// Marks an HTTP response as a refusal of the request before routing, for
/// [`record_if_refused`] to record where the request's clock started.
#[derive(Clone, Copy, Debug)]
struct Refused;

/// `resp`, marked as a refusal before routing.
pub fn refused<B>(mut resp: hyper::Response<B>) -> hyper::Response<B> {
    resp.extensions_mut().insert(Refused);
    resp
}

/// Records `resp` as an unrouted call with its HTTP status if it was
/// marked [`refused`], and removes the mark.
pub fn record_if_refused<B>(
    handler: &RequestHandler,
    system: RpcSystem,
    started: Instant,
    resp: &mut hyper::Response<B>,
) {
    if resp.extensions_mut().remove::<Refused>().is_some() {
        record_unrouted(handler, system, started, resp.status().as_str());
    }
}
