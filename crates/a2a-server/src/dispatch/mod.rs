// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! HTTP dispatch layer — JSON-RPC and REST routing.

#[cfg(feature = "axum")]
pub mod axum_adapter;
pub mod cors;
#[cfg(feature = "grpc")]
pub mod grpc;
pub mod jsonrpc;
pub mod rest;
#[cfg(feature = "websocket")]
pub mod websocket;

pub use cors::CorsConfig;
#[cfg(feature = "grpc")]
pub use grpc::{GrpcConfig, GrpcDispatcher};
pub use jsonrpc::JsonRpcDispatcher;
pub use rest::RestDispatcher;
#[cfg(feature = "websocket")]
pub use websocket::WebSocketDispatcher;

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
    /// SSE keep-alive interval. Default: 30 seconds.
    ///
    /// Periodic `: keep-alive` comments are sent at this interval to prevent
    /// proxies and load balancers from closing idle SSE connections.
    pub sse_keep_alive_interval: std::time::Duration,
    /// SSE response body channel capacity. Default: 64.
    ///
    /// Controls backpressure between the event reader task and the HTTP
    /// response body. Higher values buffer more SSE frames in memory.
    pub sse_channel_capacity: usize,
}

impl Default for DispatchConfig {
    fn default() -> Self {
        Self {
            max_request_body_size: 4 * 1024 * 1024,
            body_read_timeout: std::time::Duration::from_secs(30),
            max_query_string_length: 4096,
            sse_keep_alive_interval: std::time::Duration::from_secs(30),
            sse_channel_capacity: 64,
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

    /// Sets the SSE keep-alive interval.
    #[must_use]
    pub const fn with_sse_keep_alive_interval(mut self, interval: std::time::Duration) -> Self {
        self.sse_keep_alive_interval = interval;
        self
    }

    /// Sets the SSE response body channel capacity.
    #[must_use]
    pub const fn with_sse_channel_capacity(mut self, capacity: usize) -> Self {
        self.sse_channel_capacity = capacity;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn default_values() {
        let config = DispatchConfig::default();
        assert_eq!(config.max_request_body_size, 4 * 1024 * 1024);
        assert_eq!(config.body_read_timeout, Duration::from_secs(30));
        assert_eq!(config.max_query_string_length, 4096);
        assert_eq!(config.sse_keep_alive_interval, Duration::from_secs(30));
        assert_eq!(config.sse_channel_capacity, 64);
    }

    #[test]
    fn with_max_request_body_size_sets_value() {
        let config = DispatchConfig::default().with_max_request_body_size(8 * 1024 * 1024);
        assert_eq!(config.max_request_body_size, 8 * 1024 * 1024);
    }

    #[test]
    fn with_body_read_timeout_sets_value() {
        let config = DispatchConfig::default().with_body_read_timeout(Duration::from_secs(60));
        assert_eq!(config.body_read_timeout, Duration::from_secs(60));
    }

    #[test]
    fn with_max_query_string_length_sets_value() {
        let config = DispatchConfig::default().with_max_query_string_length(8192);
        assert_eq!(config.max_query_string_length, 8192);
    }

    #[test]
    fn with_sse_keep_alive_interval_sets_value() {
        let config =
            DispatchConfig::default().with_sse_keep_alive_interval(Duration::from_secs(15));
        assert_eq!(config.sse_keep_alive_interval, Duration::from_secs(15));
    }

    #[test]
    fn with_sse_channel_capacity_sets_value() {
        let config = DispatchConfig::default().with_sse_channel_capacity(128);
        assert_eq!(config.sse_channel_capacity, 128);
    }

    #[test]
    fn builder_chaining() {
        let config = DispatchConfig::default()
            .with_max_request_body_size(1024)
            .with_body_read_timeout(Duration::from_secs(10))
            .with_max_query_string_length(2048)
            .with_sse_keep_alive_interval(Duration::from_secs(5))
            .with_sse_channel_capacity(32);

        assert_eq!(config.max_request_body_size, 1024);
        assert_eq!(config.body_read_timeout, Duration::from_secs(10));
        assert_eq!(config.max_query_string_length, 2048);
        assert_eq!(config.sse_keep_alive_interval, Duration::from_secs(5));
        assert_eq!(config.sse_channel_capacity, 32);
    }

    #[test]
    fn debug_format() {
        let config = DispatchConfig::default();
        let debug = format!("{config:?}");
        assert!(debug.contains("DispatchConfig"));
        assert!(debug.contains("max_request_body_size"));
        assert!(debug.contains("body_read_timeout"));
        assert!(debug.contains("max_query_string_length"));
        assert!(debug.contains("sse_keep_alive_interval"));
        assert!(debug.contains("sse_channel_capacity"));
    }
}
