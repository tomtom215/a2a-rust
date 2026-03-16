// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Transport abstraction for A2A client requests.
//!
//! The [`Transport`] trait decouples protocol logic from HTTP mechanics.
//! [`A2aClient`] holds a `Box<dyn Transport>` and calls
//! [`Transport::send_request`] for non-streaming methods and
//! [`Transport::send_streaming_request`] for SSE-streaming methods.
//!
//! Two implementations ship with this crate:
//!
//! | Type | Protocol | When to use |
//! |---|---|---|
//! | [`JsonRpcTransport`] | JSON-RPC 2.0 over HTTP POST | Default; most widely supported |
//! | [`RestTransport`] | HTTP REST (verbs + paths) | When the agent card requires it |
//!
//! [`A2aClient`]: crate::A2aClient
//! [`JsonRpcTransport`]: jsonrpc::JsonRpcTransport
//! [`RestTransport`]: rest::RestTransport

pub mod jsonrpc;
pub mod rest;

pub use jsonrpc::JsonRpcTransport;
pub use rest::RestTransport;

/// Maximum length for response body snippets included in error messages.
const MAX_ERROR_BODY_LEN: usize = 512;

/// Truncates a response body for inclusion in error messages.
pub(crate) fn truncate_body(body: &str) -> String {
    if body.len() <= MAX_ERROR_BODY_LEN {
        body.to_owned()
    } else {
        format!("{}...(truncated)", &body[..MAX_ERROR_BODY_LEN])
    }
}

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::error::ClientResult;
use crate::streaming::EventStream;

// ── Transport ─────────────────────────────────────────────────────────────────

/// The low-level HTTP transport interface.
///
/// Implementors handle the HTTP mechanics (connection management, header
/// injection, body framing) and return raw JSON values or SSE streams.
/// Protocol-level logic (method naming, params serialization) lives in
/// [`crate::A2aClient`] and the `methods/` modules.
///
/// # Object-safety
///
/// This trait uses `Pin<Box<dyn Future<...>>>` return types so that
/// `Box<dyn Transport>` is valid.
pub trait Transport: Send + Sync + 'static {
    /// Sends a non-streaming JSON-RPC or REST request.
    ///
    /// Returns the `result` field from the JSON-RPC success response as a
    /// raw [`serde_json::Value`] for the caller to deserialize.
    ///
    /// The `extra_headers` map is injected verbatim into the HTTP request
    /// (e.g. `Authorization` from an [`crate::auth::AuthInterceptor`]).
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>>;

    /// Sends a streaming request and returns an [`EventStream`].
    ///
    /// The request is sent with `Accept: text/event-stream`; the response body
    /// is a Server-Sent Events stream. The returned [`EventStream`] lets the
    /// caller iterate over [`a2a_protocol_types::StreamResponse`] events.
    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>>;
}
