// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Configuration for the gRPC dispatcher.

/// Configuration for the gRPC dispatcher.
///
/// Controls message size limits, compression, and concurrency settings.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::dispatch::grpc::GrpcConfig;
///
/// let config = GrpcConfig::default()
///     .with_max_message_size(8 * 1024 * 1024)
///     .with_concurrency_limit(128);
/// ```
#[derive(Debug, Clone)]
pub struct GrpcConfig {
    /// Maximum inbound message size in bytes. Default: 4 MiB.
    pub max_message_size: usize,
    /// Maximum number of concurrent gRPC requests. Default: 256.
    pub concurrency_limit: usize,
    /// Channel capacity for streaming responses. Default: 64.
    pub stream_channel_capacity: usize,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            max_message_size: 4 * 1024 * 1024,
            concurrency_limit: 256,
            stream_channel_capacity: 64,
        }
    }
}

impl GrpcConfig {
    /// Sets the maximum inbound message size.
    #[must_use]
    pub const fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Sets the maximum number of concurrent gRPC requests.
    #[must_use]
    pub const fn with_concurrency_limit(mut self, limit: usize) -> Self {
        self.concurrency_limit = limit;
        self
    }

    /// Sets the channel capacity for streaming responses.
    #[must_use]
    pub const fn with_stream_channel_capacity(mut self, capacity: usize) -> Self {
        self.stream_channel_capacity = capacity;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grpc_config_default_values() {
        let config = GrpcConfig::default();
        assert_eq!(config.max_message_size, 4 * 1024 * 1024);
        assert_eq!(config.concurrency_limit, 256);
        assert_eq!(config.stream_channel_capacity, 64);
    }

    #[test]
    fn grpc_config_builders() {
        let config = GrpcConfig::default()
            .with_max_message_size(8 * 1024 * 1024)
            .with_concurrency_limit(128)
            .with_stream_channel_capacity(32);
        assert_eq!(config.max_message_size, 8 * 1024 * 1024);
        assert_eq!(config.concurrency_limit, 128);
        assert_eq!(config.stream_channel_capacity, 32);
    }
}
