// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Configurable limits for [`super::RequestHandler`].

use std::time::Duration;

/// Configurable limits for the request handler.
///
/// All fields have sensible defaults. Create with [`HandlerLimits::default()`]
/// and override individual values as needed.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::handler::HandlerLimits;
///
/// let limits = HandlerLimits::default()
///     .with_max_id_length(2048)
///     .with_max_metadata_size(2 * 1024 * 1024);
/// ```
#[derive(Debug, Clone)]
pub struct HandlerLimits {
    /// Maximum allowed length for task/context IDs. Default: 1024.
    pub max_id_length: usize,
    /// Maximum allowed serialized size for metadata fields in bytes. Default: 1 MiB.
    pub max_metadata_size: usize,
    /// Maximum cancellation token map entries before cleanup sweep. Default: 10,000.
    pub max_cancellation_tokens: usize,
    /// Maximum age for cancellation tokens. Default: 1 hour.
    pub max_token_age: Duration,
    /// Timeout for individual push webhook deliveries. Default: 5 seconds.
    ///
    /// Bounds how long the handler waits for a single push notification delivery
    /// to complete, preventing one slow webhook from blocking all subsequent
    /// deliveries.
    pub push_delivery_timeout: Duration,
}

impl Default for HandlerLimits {
    fn default() -> Self {
        Self {
            max_id_length: 1024,
            max_metadata_size: 1_048_576,
            max_cancellation_tokens: 10_000,
            max_token_age: Duration::from_secs(3600),
            push_delivery_timeout: Duration::from_secs(5),
        }
    }
}

impl HandlerLimits {
    /// Sets the maximum allowed length for task/context IDs.
    #[must_use]
    pub const fn with_max_id_length(mut self, length: usize) -> Self {
        self.max_id_length = length;
        self
    }

    /// Sets the maximum serialized size for metadata fields in bytes.
    #[must_use]
    pub const fn with_max_metadata_size(mut self, size: usize) -> Self {
        self.max_metadata_size = size;
        self
    }

    /// Sets the maximum cancellation token map entries before cleanup.
    #[must_use]
    pub const fn with_max_cancellation_tokens(mut self, max: usize) -> Self {
        self.max_cancellation_tokens = max;
        self
    }

    /// Sets the maximum age for cancellation tokens.
    #[must_use]
    pub const fn with_max_token_age(mut self, age: Duration) -> Self {
        self.max_token_age = age;
        self
    }

    /// Sets the timeout for individual push webhook deliveries.
    #[must_use]
    pub const fn with_push_delivery_timeout(mut self, timeout: Duration) -> Self {
        self.push_delivery_timeout = timeout;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_values() {
        let limits = HandlerLimits::default();
        assert_eq!(limits.max_id_length, 1024);
        assert_eq!(limits.max_metadata_size, 1_048_576);
        assert_eq!(limits.max_cancellation_tokens, 10_000);
        assert_eq!(limits.max_token_age, Duration::from_secs(3600));
        assert_eq!(limits.push_delivery_timeout, Duration::from_secs(5));
    }

    #[test]
    fn with_max_id_length_sets_value() {
        let limits = HandlerLimits::default().with_max_id_length(2048);
        assert_eq!(limits.max_id_length, 2048);
    }

    #[test]
    fn with_max_metadata_size_sets_value() {
        let limits = HandlerLimits::default().with_max_metadata_size(2_097_152);
        assert_eq!(limits.max_metadata_size, 2_097_152);
    }

    #[test]
    fn with_max_cancellation_tokens_sets_value() {
        let limits = HandlerLimits::default().with_max_cancellation_tokens(5_000);
        assert_eq!(limits.max_cancellation_tokens, 5_000);
    }

    #[test]
    fn with_max_token_age_sets_value() {
        let limits = HandlerLimits::default().with_max_token_age(Duration::from_secs(7200));
        assert_eq!(limits.max_token_age, Duration::from_secs(7200));
    }

    #[test]
    fn with_push_delivery_timeout_sets_value() {
        let limits = HandlerLimits::default().with_push_delivery_timeout(Duration::from_secs(10));
        assert_eq!(limits.push_delivery_timeout, Duration::from_secs(10));
    }

    #[test]
    fn builder_chaining() {
        let limits = HandlerLimits::default()
            .with_max_id_length(512)
            .with_max_metadata_size(500_000)
            .with_max_cancellation_tokens(1_000)
            .with_max_token_age(Duration::from_secs(1800))
            .with_push_delivery_timeout(Duration::from_secs(15));

        assert_eq!(limits.max_id_length, 512);
        assert_eq!(limits.max_metadata_size, 500_000);
        assert_eq!(limits.max_cancellation_tokens, 1_000);
        assert_eq!(limits.max_token_age, Duration::from_secs(1800));
        assert_eq!(limits.push_delivery_timeout, Duration::from_secs(15));
    }

    #[test]
    fn debug_format() {
        let limits = HandlerLimits::default();
        let debug = format!("{limits:?}");
        assert!(debug.contains("HandlerLimits"));
        assert!(debug.contains("max_id_length"));
        assert!(debug.contains("max_metadata_size"));
        assert!(debug.contains("max_cancellation_tokens"));
        assert!(debug.contains("max_token_age"));
        assert!(debug.contains("push_delivery_timeout"));
    }
}
