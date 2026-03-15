// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Call context for server-side interceptors.
//!
//! [`CallContext`] carries metadata about the current JSON-RPC or REST call,
//! allowing [`ServerInterceptor`](crate::ServerInterceptor) implementations
//! to make access-control and auditing decisions.

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
}

impl CallContext {
    /// Creates a new [`CallContext`] for the given method.
    #[must_use]
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            caller_identity: None,
            extensions: Vec::new(),
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
}
