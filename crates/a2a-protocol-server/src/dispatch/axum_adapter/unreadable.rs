// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A request this router could not read, answered after admission, as
//! `RestDispatcher` answers one (`dispatch/rest/params.rs`).
//!
//! Until 2026-10-20 a body that was not JSON or did not fit the method's
//! params, or a query value that did not parse, was answered `400` before
//! the handler ran, so a caller with no credential learned which field it got
//! wrong where a well-formed request was refused `401`. The body's size cap
//! and read timeout (`TimedBody`) and the media-type and version checks
//! (`validate`) still come first: they refuse a well-formed request alike.

use std::collections::HashMap;

use super::{A2aState, hyper_to_axum};
use crate::dispatch::rest::Unreadable;

/// The answer to a `method` call whose request could not be read: the
/// interceptor chain's refusal when it does not admit the caller, and the
/// `400` (or the query error's own status) when it does.
pub(super) async fn refuse_unreadable(
    state: &A2aState,
    method: &str,
    hdrs: &HashMap<String, String>,
    unreadable: impl Into<Unreadable>,
) -> axum::response::Response {
    hyper_to_axum(unreadable.into().answer(&state.handler, method, hdrs).await)
}

/// The method a send is: `SendStreamingMessage` or `SendMessage`.
pub(super) const fn send_method(streaming: bool) -> &'static str {
    if streaming {
        "SendStreamingMessage"
    } else {
        "SendMessage"
    }
}
