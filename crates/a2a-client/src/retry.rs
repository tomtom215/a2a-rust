// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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
                    let jittered_backoff = jittered(backoff);
                    trace_info!(method, attempt, ?jittered_backoff, "retrying after backoff");
                    tokio::time::sleep(jittered_backoff).await;
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
                    let jittered_backoff = jittered(backoff);
                    trace_info!(
                        method,
                        attempt,
                        ?jittered_backoff,
                        "retrying stream connect after backoff"
                    );
                    tokio::time::sleep(jittered_backoff).await;
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
///
/// Handles overflow gracefully: if the multiplication produces infinity or NaN
/// (possible with extreme multipliers or near-`Duration::MAX` values), returns
/// `max` instead of panicking.
fn cap_backoff(current: Duration, multiplier: f64, max: Duration) -> Duration {
    let next_secs = current.as_secs_f64() * multiplier;
    if !next_secs.is_finite() || next_secs < 0.0 {
        return max;
    }
    let next = Duration::from_secs_f64(next_secs);
    if next > max {
        max
    } else {
        next
    }
}

/// Applies full jitter to a backoff duration: returns a random duration in
/// `[backoff/2, backoff)`.
///
/// Uses `std::hash::RandomState` for cheap, no-dependency randomness. This
/// prevents thundering-herd retry storms where all clients experiencing the
/// same transient failure retry at identical intervals.
fn jittered(backoff: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    // Mix in the backoff value for extra entropy.
    hasher.write_u128(backoff.as_nanos());
    let random_bits = hasher.finish();
    // Map to [0.5, 1.0) range.
    #[allow(clippy::cast_precision_loss)] // Precision loss is acceptable for jitter
    let factor = (random_bits as f64 / u64::MAX as f64).mul_add(0.5, 0.5);
    let jittered_secs = backoff.as_secs_f64() * factor;
    if !jittered_secs.is_finite() || jittered_secs < 0.0 {
        backoff
    } else {
        Duration::from_secs_f64(jittered_secs)
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

    #[test]
    fn status_502_is_retryable() {
        let e = ClientError::UnexpectedStatus {
            status: 502,
            body: "Bad Gateway".into(),
        };
        assert!(e.is_retryable());
    }

    #[test]
    fn status_504_is_retryable() {
        let e = ClientError::UnexpectedStatus {
            status: 504,
            body: "Gateway Timeout".into(),
        };
        assert!(e.is_retryable());
    }

    /// Status codes adjacent to retryable ones must NOT be retryable.
    #[test]
    fn status_boundary_not_retryable() {
        for status in [428, 430, 500, 501, 505] {
            let e = ClientError::UnexpectedStatus {
                status,
                body: String::new(),
            };
            assert!(!e.is_retryable(), "status {status} should not be retryable");
        }
    }

    #[test]
    fn retry_policy_builder_methods() {
        let p = RetryPolicy::default()
            .with_max_retries(5)
            .with_initial_backoff(Duration::from_secs(1))
            .with_max_backoff(Duration::from_secs(60))
            .with_backoff_multiplier(3.0);
        assert_eq!(p.max_retries, 5);
        assert_eq!(p.initial_backoff, Duration::from_secs(1));
        assert_eq!(p.max_backoff, Duration::from_secs(60));
        assert!((p.backoff_multiplier - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cap_backoff_exact_boundary() {
        // When next == max, should return next (not max via the > branch).
        let result = cap_backoff(Duration::from_secs(5), 1.0, Duration::from_secs(5));
        assert_eq!(result, Duration::from_secs(5));

        // When next < max, should return next.
        let result = cap_backoff(Duration::from_millis(1), 2.0, Duration::from_secs(5));
        assert_eq!(result, Duration::from_millis(2));
    }

    #[test]
    fn cap_backoff_infinity_returns_max() {
        // Extreme multiplier that would produce infinity.
        let max = Duration::from_secs(30);
        let result = cap_backoff(Duration::from_secs(u64::MAX / 2), f64::MAX, max);
        assert_eq!(result, max, "infinity should clamp to max");
    }

    #[test]
    fn cap_backoff_nan_returns_max() {
        let max = Duration::from_secs(30);
        let result = cap_backoff(Duration::from_secs(0), f64::NAN, max);
        assert_eq!(result, max, "NaN should clamp to max");
    }

    // ── Mock transport for retry tests ────────────────────────────────────

    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::streaming::EventStream;

    /// A transport that fails N times with a retryable error, then succeeds.
    struct FailNTransport {
        failures_remaining: Arc<AtomicUsize>,
        success_response: serde_json::Value,
        call_count: Arc<AtomicUsize>,
    }

    impl FailNTransport {
        fn new(fail_count: usize, response: serde_json::Value) -> Self {
            Self {
                failures_remaining: Arc::new(AtomicUsize::new(fail_count)),
                success_response: response,
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl crate::transport::Transport for FailNTransport {
        fn send_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let remaining = self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
            let resp = self.success_response.clone();
            Box::pin(async move {
                if remaining > 0 {
                    Err(ClientError::Timeout("transient".into()))
                } else {
                    Ok(resp)
                }
            })
        }

        fn send_streaming_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let remaining = self.failures_remaining.fetch_sub(1, Ordering::SeqCst);
            Box::pin(async move {
                if remaining > 0 {
                    Err(ClientError::Timeout("transient".into()))
                } else {
                    Err(ClientError::Transport("streaming not mocked".into()))
                }
            })
        }
    }

    /// A transport that always fails with a non-retryable error.
    struct NonRetryableErrorTransport {
        call_count: Arc<AtomicUsize>,
    }

    impl NonRetryableErrorTransport {
        fn new() -> Self {
            Self {
                call_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl crate::transport::Transport for NonRetryableErrorTransport {
        fn send_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<serde_json::Value>> + Send + 'a>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Err(ClientError::InvalidEndpoint("bad url".into())) })
        }

        fn send_streaming_request<'a>(
            &'a self,
            _method: &'a str,
            _params: serde_json::Value,
            _extra_headers: &'a HashMap<String, String>,
        ) -> Pin<Box<dyn Future<Output = ClientResult<EventStream>> + Send + 'a>> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Err(ClientError::InvalidEndpoint("bad url".into())) })
        }
    }

    #[tokio::test]
    async fn retry_transport_retries_on_transient_error() {
        let inner = FailNTransport::new(2, serde_json::json!({"ok": true}));
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(3),
        );

        let headers = HashMap::new();
        let result = transport
            .send_request("test", serde_json::Value::Null, &headers)
            .await;
        assert!(result.is_ok(), "should succeed after retries");
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            3,
            "should have made 3 attempts (2 failures + 1 success)"
        );
    }

    #[tokio::test]
    async fn retry_transport_gives_up_after_max_retries() {
        // Fail more times than max_retries allows.
        let inner = FailNTransport::new(10, serde_json::json!({"ok": true}));
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(2),
        );

        let headers = HashMap::new();
        let result = transport
            .send_request("test", serde_json::Value::Null, &headers)
            .await;
        assert!(result.is_err(), "should fail after exhausting retries");
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            3,
            "should have made 3 attempts (initial + 2 retries)"
        );
    }

    #[tokio::test]
    async fn retry_transport_no_retry_on_non_retryable() {
        let inner = NonRetryableErrorTransport::new();
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(3),
        );

        let headers = HashMap::new();
        let result = transport
            .send_request("test", serde_json::Value::Null, &headers)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ClientError::InvalidEndpoint(_)
        ));
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "non-retryable error should not be retried"
        );
    }

    #[tokio::test]
    async fn retry_transport_streaming_retries() {
        let inner = FailNTransport::new(1, serde_json::json!(null));
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(2),
        );

        let headers = HashMap::new();
        let result = transport
            .send_streaming_request("test", serde_json::Value::Null, &headers)
            .await;
        // After 1 transient failure, the mock returns a Transport error
        // (non-retryable) on "success" path, but the point is it retried.
        assert!(result.is_err());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            2,
            "should have retried once for streaming"
        );
    }

    #[tokio::test]
    async fn retry_transport_streaming_no_retry_on_non_retryable() {
        let inner = NonRetryableErrorTransport::new();
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(3),
        );

        let headers = HashMap::new();
        let result = transport
            .send_streaming_request("test", serde_json::Value::Null, &headers)
            .await;
        assert!(matches!(
            result.unwrap_err(),
            ClientError::InvalidEndpoint(_)
        ));
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "non-retryable streaming error should not be retried"
        );
    }

    #[tokio::test]
    async fn retry_transport_streaming_exhausts_retries() {
        let inner = FailNTransport::new(10, serde_json::json!(null));
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(2),
        );

        let headers = HashMap::new();
        let result = transport
            .send_streaming_request("test", serde_json::Value::Null, &headers)
            .await;
        assert!(result.is_err());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            3,
            "should make 3 attempts total for streaming"
        );
    }

    #[tokio::test]
    async fn retry_transport_succeeds_without_retry_on_first_attempt() {
        let inner = FailNTransport::new(0, serde_json::json!({"ok": true}));
        let call_count = Arc::clone(&inner.call_count);
        let transport = RetryTransport::new(
            Box::new(inner),
            RetryPolicy::default()
                .with_initial_backoff(Duration::from_millis(1))
                .with_max_retries(3),
        );

        let headers = HashMap::new();
        let result = transport
            .send_request("test", serde_json::Value::Null, &headers)
            .await;
        assert!(result.is_ok());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            1,
            "should succeed on first try"
        );
    }
}
