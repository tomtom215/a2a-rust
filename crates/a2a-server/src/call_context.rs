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
//! assert_eq!(ctx.http_headers.get("authorization").map(String::as_str),
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
    pub method: String,

    /// Optional caller identity extracted from authentication headers.
    pub caller_identity: Option<String>,

    /// Extension URIs active for this request.
    pub extensions: Vec<String>,

    /// HTTP request headers from the incoming request.
    ///
    /// Keys are lowercased for case-insensitive matching. For headers with
    /// multiple values, only the last value is stored. Interceptors can use
    /// this to inspect `Authorization`, `X-Forwarded-For`, `X-Request-ID`,
    /// and any other HTTP headers for access control or auditing.
    pub http_headers: HashMap<String, String>,
}

impl CallContext {
    /// Creates a new [`CallContext`] for the given method.
    #[must_use]
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            caller_identity: None,
            extensions: Vec::new(),
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

    /// Sets the HTTP headers map (replacing any existing headers).
    #[must_use]
    pub fn with_http_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.http_headers = headers;
        self
    }

    /// Adds a single HTTP header (key is lowercased for case-insensitive matching).
    #[must_use]
    pub fn with_http_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.http_headers
            .insert(key.into().to_ascii_lowercase(), value.into());
        self
    }
}
