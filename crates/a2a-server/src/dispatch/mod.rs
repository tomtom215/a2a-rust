// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! HTTP dispatch layer — JSON-RPC and REST routing.

pub mod cors;
pub mod jsonrpc;
pub mod rest;

pub use cors::CorsConfig;
pub use jsonrpc::JsonRpcDispatcher;
pub use rest::RestDispatcher;

/// Configuration for dispatch-layer limits shared by both JSON-RPC and REST
/// dispatchers.
///
/// All fields have sensible defaults. Create with [`DispatchConfig::default()`]
/// and override individual values as needed.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::dispatch::DispatchConfig;
///
/// let config = DispatchConfig::default()
///     .with_max_request_body_size(8 * 1024 * 1024)
///     .with_body_read_timeout(std::time::Duration::from_secs(60));
/// ```
#[derive(Debug, Clone)]
pub struct DispatchConfig {
    /// Maximum request body size in bytes. Default: 4 MiB.
    pub max_request_body_size: usize,
    /// Timeout for reading the full request body. Default: 30 seconds.
    pub body_read_timeout: std::time::Duration,
    /// Maximum query string length (REST only). Default: 4096.
    pub max_query_string_length: usize,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            max_request_body_size: 4 * 1024 * 1024,
            body_read_timeout: std::time::Duration::from_secs(30),
            max_query_string_length: 4096,
        }
    }
}

impl DispatchConfig {
    /// Sets the maximum request body size in bytes.
    #[must_use]
    pub const fn with_max_request_body_size(mut self, size: usize) -> Self {
        self.max_request_body_size = size;
        self
    }

    /// Sets the timeout for reading request bodies.
    #[must_use]
    pub const fn with_body_read_timeout(mut self, timeout: std::time::Duration) -> Self {
        self.body_read_timeout = timeout;
        self
    }

    /// Sets the maximum query string length (REST only).
    #[must_use]
    pub const fn with_max_query_string_length(mut self, length: usize) -> Self {
        self.max_query_string_length = length;
        self
    }
}
