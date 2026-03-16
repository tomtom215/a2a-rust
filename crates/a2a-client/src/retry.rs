// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Configurable retry policy for transient client errors.
//!
//! Wraps any [`Transport`] to automatically retry on transient failures
//! (connection errors, timeouts, server 5xx responses) with exponential
//! backoff.
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_client::{ClientBuilder, RetryPolicy};
//!
//! # fn example() -> Result<(), a2a_protocol_client::error::ClientError> {
//! let client = ClientBuilder::new("http://localhost:8080")
//!     .with_retry_policy(RetryPolicy::default())
//!     .build()?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use crate::error::{ClientError, ClientResult};
use crate::streaming::EventStream;
use crate::transport::Transport;

// ── RetryPolicy ──────────────────────────────────────────────────────────────

/// Configuration for automatic retry with exponential backoff.
///
/// # Defaults
///
/// | Field | Default |
/// |---|---|
/// | `max_retries` | 3 |
/// | `initial_backoff` | 500 ms |
/// | `max_backoff` | 30 s |
/// | `backoff_multiplier` | 2.0 |
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (not counting the initial attempt).
    pub max_retries: u32,
    /// Initial backoff duration before the first retry.
    pub initial_backoff: Duration,
    /// Maximum backoff duration (caps exponential growth).
    pub max_backoff: Duration,
    /// Multiplier applied to the backoff after each retry.
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Creates a retry policy with the given maximum number of retries.
    #[must_use]
    pub const fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Sets the initial backoff duration.
    #[must_use]
    pub const fn with_initial_backoff(mut self, backoff: Duration) -> Self {
        self.initial_backoff = backoff;
        self
    }

    /// Sets the maximum backoff duration.
    #[must_use]
    pub const fn with_max_backoff(mut self, max: Duration) -> Self {
        self.max_backoff = max;
        self
    }

    /// Sets the backoff multiplier.
    #[must_use]
    pub const fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }
}

// ── is_retryable ─────────────────────────────────────────────────────────────

impl ClientError {
    /// Returns `true` if this error is transient and the request should be retried.
    ///
    /// Retryable errors include:
    /// - HTTP connection/transport errors
    /// - Timeouts
    /// - Server errors (HTTP 502, 503, 504, 429)
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::Http(_) | Self::HttpClient(_) | Self::Timeout(_) => true,
            Self::UnexpectedStatus { status, .. } => {
                matches!(status, 429 | 502 | 503 | 504)
            }
            // Non-retryable: serialization, protocol, config, auth errors
            Self::Serialization(_)
            | Self::Protocol(_)
            | Self::Transport(_)
            | Self::InvalidEndpoint(_)
            | Self::AuthRequired { .. }
            | Self::ProtocolBindingMismatch(_) => false,
        }
    }
}

// ── RetryTransport ───────────────────────────────────────────────────────────

/// A [`Transport`] wrapper that retries transient failures with exponential
/// backoff.
pub(crate) struct RetryTransport {
    inner: Box<dyn Transport>,
    policy: RetryPolicy,
}

impl RetryTransport {
    /// Creates a new retry transport wrapping the given inner transport.
    pub(crate) fn new(inner: Box<dyn Transport>, policy: RetryPolicy) -> Self {
        Self { inner, policy }
    }
}

impl Transport for RetryTransport {
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
        Box::pin(async move {
            let mut last_err = None;
            let mut backoff = self.policy.initial_backoff;

            for attempt in 0..=self.policy.max_retries {
                if attempt > 0 {
                    trace_info!(method, attempt, "retrying after backoff");
                    tokio::time::sleep(backoff).await;
                    backoff = cap_backoff(
                        backoff,
                        self.policy.backoff_multiplier,
                        self.policy.max_backoff,
                    );
                }

                match self
                    .inner
                    .send_request(method, params.clone(), extra_headers)
                    .await
                {
                    Ok(result) => return Ok(result),
                    Err(e) if e.is_retryable() => {
                        trace_warn!(method, attempt, error = %e, "transient error, will retry");
                        last_err = Some(e);
                    }
                    Err(e) => return Err(e),
                }
            }

            Err(last_err.expect("at least one attempt was made"))
        })
    }

    fn send_streaming_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
        extra_headers: &'a HashMap<String, String>,
    ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
        Box::pin(async move {
            let mut last_err = None;
            let mut backoff = self.policy.initial_backoff;

            for attempt in 0..=self.policy.max_retries {
                if attempt > 0 {
                    trace_info!(method, attempt, "retrying stream connect after backoff");
                    tokio::time::sleep(backoff).await;
                    backoff = cap_backoff(
                        backoff,
                        self.policy.backoff_multiplier,
                        self.policy.max_backoff,
                    );
                }

                match self
                    .inner
                    .send_streaming_request(method, params.clone(), extra_headers)
                    .await
                {
                    Ok(stream) => return Ok(stream),
                    Err(e) if e.is_retryable() => {
                        trace_warn!(method, attempt, error = %e, "transient error, will retry");
                        last_err = Some(e);
                    }
                    Err(e) => return Err(e),
                }
            }

            Err(last_err.expect("at least one attempt was made"))
        })
    }
}

/// Computes the next backoff duration, capped at `max`.
fn cap_backoff(current: Duration, multiplier: f64, max: Duration) -> Duration {
    let next = Duration::from_secs_f64(current.as_secs_f64() * multiplier);
    if next > max {
        max
    } else {
        next
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_errors_are_retryable() {
        let e = ClientError::HttpClient("connection refused".into());
        assert!(e.is_retryable());
    }

    #[test]
    fn timeout_is_retryable() {
        let e = ClientError::Timeout("request timed out".into());
        assert!(e.is_retryable());
    }

    #[test]
    fn status_503_is_retryable() {
        let e = ClientError::UnexpectedStatus {
            status: 503,
            body: "Service Unavailable".into(),
        };
        assert!(e.is_retryable());
    }

    #[test]
    fn status_429_is_retryable() {
        let e = ClientError::UnexpectedStatus {
            status: 429,
            body: "Too Many Requests".into(),
        };
        assert!(e.is_retryable());
    }

    #[test]
    fn status_404_is_not_retryable() {
        let e = ClientError::UnexpectedStatus {
            status: 404,
            body: "Not Found".into(),
        };
        assert!(!e.is_retryable());
    }

    #[test]
    fn serialization_error_is_not_retryable() {
        let e = ClientError::Serialization(serde_json::from_str::<String>("not json").unwrap_err());
        assert!(!e.is_retryable());
    }

    #[test]
    fn protocol_error_is_not_retryable() {
        let e = ClientError::Protocol(a2a_protocol_types::A2aError::task_not_found("t1"));
        assert!(!e.is_retryable());
    }

    #[test]
    fn default_retry_policy() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_retries, 3);
        assert_eq!(p.initial_backoff, Duration::from_millis(500));
        assert_eq!(p.max_backoff, Duration::from_secs(30));
        assert!((p.backoff_multiplier - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cap_backoff_works() {
        let result = cap_backoff(Duration::from_secs(1), 2.0, Duration::from_secs(5));
        assert_eq!(result, Duration::from_secs(2));

        let result = cap_backoff(Duration::from_secs(4), 2.0, Duration::from_secs(5));
        assert_eq!(result, Duration::from_secs(5));
    }
}
