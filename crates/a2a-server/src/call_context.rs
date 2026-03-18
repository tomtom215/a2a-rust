// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Call context for server-side interceptors.
//!
//! [`CallContext`] carries metadata about the current JSON-RPC or REST call,
//! allowing [`ServerInterceptor`](crate::ServerInterceptor) implementations
//! to make access-control and auditing decisions.
//!
//! # HTTP headers
//!
//! The [`http_headers`](CallContext::http_headers) field carries the raw HTTP
//! request headers (lowercased keys, last-value-wins for duplicates). This
//! enables interceptors to inspect `Authorization`, `X-Request-ID`, or any
//! other header without coupling the SDK to a specific HTTP library.
//!
//! ```rust,no_run
//! use a2a_protocol_server::CallContext;
//!
//! let ctx = CallContext::new("SendMessage")
//!     .with_http_header("authorization", "Bearer tok_abc123")
//!     .with_http_header("x-request-id", "req-42");
//!
//! assert_eq!(ctx.http_headers().get("authorization").map(String::as_str),
//!            Some("Bearer tok_abc123"));
//! ```

use std::collections::HashMap;

/// Metadata about the current server-side method call.
///
/// Passed to [`ServerInterceptor::before`](crate::ServerInterceptor::before)
/// and [`ServerInterceptor::after`](crate::ServerInterceptor::after).
#[derive(Debug, Clone)]
pub struct CallContext {
    /// The JSON-RPC method name (e.g. `"message/send"`).
    method: String,

    /// Optional caller identity extracted from authentication headers.
    caller_identity: Option<String>,

    /// Extension URIs active for this request.
    extensions: Vec<String>,

    /// First-class request/trace identifier for observability.
    request_id: Option<String>,

    /// HTTP request headers from the incoming request.
    ///
    /// Keys are lowercased for case-insensitive matching.
    http_headers: HashMap<String, String>,
}

impl CallContext {
    /// Returns the JSON-RPC method name.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Returns the optional caller identity.
    #[must_use]
    pub fn caller_identity(&self) -> Option<&str> {
        self.caller_identity.as_deref()
    }

    /// Returns the active extension URIs.
    #[must_use]
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Returns the request/trace ID if set.
    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    /// Returns the HTTP request headers (read-only).
    #[must_use]
    pub const fn http_headers(&self) -> &HashMap<String, String> {
        &self.http_headers
    }
}

impl CallContext {
    /// Creates a new [`CallContext`] for the given method.
    #[must_use]
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            caller_identity: None,
            extensions: Vec::new(),
            request_id: None,
            http_headers: HashMap::new(),
        }
    }

    /// Sets the caller identity.
    #[must_use]
    pub fn with_caller_identity(mut self, identity: String) -> Self {
        self.caller_identity = Some(identity);
        self
    }

    /// Sets the active extensions.
    #[must_use]
    pub fn with_extensions(mut self, extensions: Vec<String>) -> Self {
        self.extensions = extensions;
        self
    }

    /// Sets the request/trace ID explicitly.
    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Sets the HTTP headers map (replacing any existing headers).
    ///
    /// Automatically extracts `x-request-id` into [`request_id`](Self::request_id)
    /// if present.
    #[must_use]
    pub fn with_http_headers(mut self, headers: HashMap<String, String>) -> Self {
        if let Some(rid) = headers.get("x-request-id") {
            self.request_id = Some(rid.clone());
        }
        self.http_headers = headers;
        self
    }

    /// Adds a single HTTP header (key is lowercased for case-insensitive matching).
    ///
    /// If the key is `x-request-id`, also populates [`request_id`](Self::request_id).
    #[must_use]
    pub fn with_http_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into().to_ascii_lowercase();
        let value = value.into();
        if key == "x-request-id" {
            self.request_id = Some(value.clone());
        }
        self.http_headers.insert(key, value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_http_header_x_request_id_populates_request_id() {
        let ctx = CallContext::new("test").with_http_header("x-request-id", "req-42");
        assert_eq!(ctx.request_id(), Some("req-42"));
        assert_eq!(
            ctx.http_headers().get("x-request-id").map(String::as_str),
            Some("req-42")
        );
    }

    #[test]
    fn with_http_header_other_key_does_not_populate_request_id() {
        let ctx = CallContext::new("test").with_http_header("authorization", "Bearer tok");
        assert!(ctx.request_id().is_none());
        assert_eq!(
            ctx.http_headers().get("authorization").map(String::as_str),
            Some("Bearer tok")
        );
    }

    #[test]
    fn with_request_id_sets_field() {
        let ctx = CallContext::new("test").with_request_id("req-99");
        assert_eq!(ctx.request_id(), Some("req-99"));
    }

    #[test]
    fn with_http_headers_extracts_request_id() {
        let mut headers = HashMap::new();
        headers.insert("x-request-id".to_owned(), "trace-123".to_owned());
        headers.insert("content-type".to_owned(), "application/json".to_owned());

        let ctx = CallContext::new("test").with_http_headers(headers);
        assert_eq!(ctx.request_id(), Some("trace-123"));
        assert_eq!(
            ctx.http_headers().get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn with_http_headers_without_request_id() {
        let mut headers = HashMap::new();
        headers.insert("authorization".to_owned(), "Bearer tok".to_owned());

        let ctx = CallContext::new("test").with_http_headers(headers);
        assert!(ctx.request_id().is_none());
    }

    #[test]
    fn with_caller_identity_sets_field() {
        let ctx = CallContext::new("test").with_caller_identity("user@example.com".into());
        assert_eq!(ctx.caller_identity(), Some("user@example.com"));
    }

    #[test]
    fn with_extensions_sets_field() {
        let ctx = CallContext::new("test").with_extensions(vec!["ext1".into(), "ext2".into()]);
        assert_eq!(ctx.extensions(), &["ext1", "ext2"]);
    }

    #[test]
    fn new_defaults_are_empty() {
        let ctx = CallContext::new("method");
        assert_eq!(ctx.method(), "method");
        assert!(ctx.caller_identity().is_none());
        assert!(ctx.extensions().is_empty());
        assert!(ctx.request_id().is_none());
        assert!(ctx.http_headers().is_empty());
    }
}
