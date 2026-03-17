// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification sender trait and HTTP implementation.
//!
//! [`PushSender`] abstracts the delivery of streaming events to client webhook
//! endpoints. [`HttpPushSender`] uses hyper to POST events over HTTP(S).
//!
//! # Security
//!
//! [`HttpPushSender`] validates webhook URLs to reject private/loopback
//! addresses (SSRF protection) and sanitizes authentication credentials
//! to prevent HTTP header injection.

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use bytes::Bytes;
use http_body_util::Full;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;

/// Trait for delivering push notifications to client webhooks.
///
/// Object-safe; used as `Box<dyn PushSender>`.
pub trait PushSender: Send + Sync + 'static {
    /// Sends a streaming event to the client's webhook URL.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`] if delivery fails after all retries.
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}

/// Default per-request timeout for push notification delivery.
const DEFAULT_PUSH_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Retry policy for push notification delivery.
///
/// # Example
///
/// ```rust
/// use a2a_protocol_server::push::PushRetryPolicy;
///
/// let policy = PushRetryPolicy::default()
///     .with_max_attempts(5)
///     .with_backoff(vec![
///         std::time::Duration::from_millis(500),
///         std::time::Duration::from_secs(1),
///         std::time::Duration::from_secs(2),
///         std::time::Duration::from_secs(4),
///     ]);
/// ```
#[derive(Debug, Clone)]
pub struct PushRetryPolicy {
    /// Maximum number of delivery attempts before giving up. Default: 3.
    pub max_attempts: usize,
    /// Backoff durations between retry attempts. Default: `[1s, 2s]`.
    ///
    /// If there are fewer entries than `max_attempts - 1`, the last duration
    /// is repeated for remaining retries.
    pub backoff: Vec<std::time::Duration>,
}

impl Default for PushRetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            backoff: vec![
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(2),
            ],
        }
    }
}

impl PushRetryPolicy {
    /// Sets the maximum number of delivery attempts.
    #[must_use]
    pub const fn with_max_attempts(mut self, max: usize) -> Self {
        self.max_attempts = max;
        self
    }

    /// Sets the backoff schedule between retry attempts.
    #[must_use]
    pub fn with_backoff(mut self, backoff: Vec<std::time::Duration>) -> Self {
        self.backoff = backoff;
        self
    }
}

/// HTTP-based [`PushSender`] using hyper.
///
/// Retries failed deliveries according to a configurable [`PushRetryPolicy`].
///
/// # Security
///
/// - Rejects webhook URLs targeting private/loopback/link-local addresses
///   to prevent SSRF attacks.
/// - Validates authentication credentials to prevent HTTP header injection
///   (rejects values containing CR/LF characters).
#[derive(Debug)]
pub struct HttpPushSender {
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
    request_timeout: std::time::Duration,
    retry_policy: PushRetryPolicy,
    /// Whether to skip SSRF URL validation (for testing only).
    allow_private_urls: bool,
}

impl Default for HttpPushSender {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpPushSender {
    /// Creates a new [`HttpPushSender`] with the default 30-second request timeout
    /// and default retry policy.
    #[must_use]
    pub fn new() -> Self {
        Self::with_timeout(DEFAULT_PUSH_REQUEST_TIMEOUT)
    }

    /// Creates a new [`HttpPushSender`] with a custom per-request timeout.
    #[must_use]
    pub fn with_timeout(request_timeout: std::time::Duration) -> Self {
        let client = Client::builder(TokioExecutor::new()).build_http();
        Self {
            client,
            request_timeout,
            retry_policy: PushRetryPolicy::default(),
            allow_private_urls: false,
        }
    }

    /// Sets a custom retry policy for push notification delivery.
    #[must_use]
    pub fn with_retry_policy(mut self, policy: PushRetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    /// Creates an [`HttpPushSender`] that allows private/loopback URLs.
    ///
    /// **Warning:** This disables SSRF protection and should only be used
    /// in testing or trusted environments.
    #[must_use]
    pub const fn allow_private_urls(mut self) -> Self {
        self.allow_private_urls = true;
        self
    }
}

/// Returns `true` if the given IP address is private, loopback, or link-local.
#[allow(clippy::missing_const_for_fn)] // IpAddr methods aren't const-stable everywhere
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()          // 127.0.0.0/8
                || v4.is_private()    // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local() // 169.254.0.0/16
                || v4.is_unspecified() // 0.0.0.0
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()          // ::1
                || v6.is_unspecified() // ::
                // fc00::/7 (unique local)
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 (link-local)
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Validates a webhook URL to prevent SSRF attacks.
///
/// Rejects URLs targeting private/loopback/link-local addresses.
#[allow(clippy::case_sensitive_file_extension_comparisons)] // host_lower is already lowercased
fn validate_webhook_url(url: &str) -> A2aResult<()> {
    // Parse the URL to extract the host.
    let uri: hyper::Uri = url
        .parse()
        .map_err(|e| A2aError::invalid_params(format!("invalid webhook URL: {e}")))?;

    // Require http or https scheme.
    match uri.scheme_str() {
        Some("http" | "https") => {}
        Some(other) => {
            return Err(A2aError::invalid_params(format!(
                "webhook URL has unsupported scheme: {other} (expected http or https)"
            )));
        }
        None => {
            return Err(A2aError::invalid_params(
                "webhook URL missing scheme (expected http:// or https://)",
            ));
        }
    }

    let host = uri
        .host()
        .ok_or_else(|| A2aError::invalid_params("webhook URL missing host"))?;

    // Strip brackets from IPv6 addresses (hyper::Uri returns "[::1]" as host).
    let host_bare = host.trim_start_matches('[').trim_end_matches(']');

    // Try to parse the host as an IP address directly.
    if let Ok(ip) = host_bare.parse::<IpAddr>() {
        if is_private_ip(ip) {
            return Err(A2aError::invalid_params(format!(
                "webhook URL targets private/loopback address: {host}"
            )));
        }
    }

    // Check for well-known private hostnames.
    let host_lower = host.to_ascii_lowercase();
    if host_lower == "localhost"
        || host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
    {
        return Err(A2aError::invalid_params(format!(
            "webhook URL targets local/internal hostname: {host}"
        )));
    }

    Ok(())
}

/// Validates that a header value contains no CR/LF characters.
fn validate_header_value(value: &str, name: &str) -> A2aResult<()> {
    if value.contains('\r') || value.contains('\n') {
        return Err(A2aError::invalid_params(format!(
            "{name} contains invalid characters (CR/LF)"
        )));
    }
    Ok(())
}

#[allow(clippy::manual_async_fn, clippy::too_many_lines)]
impl PushSender for HttpPushSender {
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            trace_info!(url, "delivering push notification");

            // SSRF protection: reject private/loopback addresses.
            if !self.allow_private_urls {
                validate_webhook_url(url)?;
            }

            // Header injection protection: validate credentials.
            if let Some(ref auth) = config.authentication {
                validate_header_value(&auth.credentials, "authentication credentials")?;
                validate_header_value(&auth.scheme, "authentication scheme")?;
            }
            if let Some(ref token) = config.token {
                validate_header_value(token, "notification token")?;
            }

            let body_bytes: Bytes = serde_json::to_vec(event)
                .map(Bytes::from)
                .map_err(|e| A2aError::internal(format!("push serialization: {e}")))?;

            let mut last_err = String::new();

            for attempt in 0..self.retry_policy.max_attempts {
                let mut builder = hyper::Request::builder()
                    .method(hyper::Method::POST)
                    .uri(url)
                    .header("content-type", "application/json");

                // Set authentication headers from config.
                if let Some(ref auth) = config.authentication {
                    match auth.scheme.as_str() {
                        "bearer" => {
                            builder = builder
                                .header("authorization", format!("Bearer {}", auth.credentials));
                        }
                        "basic" => {
                            builder = builder
                                .header("authorization", format!("Basic {}", auth.credentials));
                        }
                        _ => {
                            trace_warn!(
                                scheme = auth.scheme.as_str(),
                                "unknown authentication scheme; no auth header set"
                            );
                        }
                    }
                }

                // Set notification token header if present.
                if let Some(ref token) = config.token {
                    builder = builder.header("a2a-notification-token", token.as_str());
                }

                let req = builder
                    .body(Full::new(body_bytes.clone()))
                    .map_err(|e| A2aError::internal(format!("push request build: {e}")))?;

                let request_result =
                    tokio::time::timeout(self.request_timeout, self.client.request(req)).await;

                match request_result {
                    Ok(Ok(resp)) if resp.status().is_success() => {
                        trace_debug!(url, "push notification delivered");
                        return Ok(());
                    }
                    Ok(Ok(resp)) => {
                        last_err = format!("push notification got HTTP {}", resp.status());
                        trace_warn!(url, attempt, status = %resp.status(), "push delivery failed");
                    }
                    Ok(Err(e)) => {
                        last_err = format!("push notification failed: {e}");
                        trace_warn!(url, attempt, error = %e, "push delivery error");
                    }
                    Err(_) => {
                        last_err = format!(
                            "push notification timed out after {}s",
                            self.request_timeout.as_secs()
                        );
                        trace_warn!(url, attempt, "push delivery timed out");
                    }
                }

                // Retry with backoff (except on last attempt).
                if attempt < self.retry_policy.max_attempts - 1 {
                    let delay = self
                        .retry_policy
                        .backoff
                        .get(attempt)
                        .or_else(|| self.retry_policy.backoff.last());
                    if let Some(delay) = delay {
                        tokio::time::sleep(*delay).await;
                    }
                }
            }

            Err(A2aError::internal(last_err))
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Covers lines 89-92 (PushRetryPolicy::with_max_attempts).
    #[test]
    fn push_retry_policy_with_max_attempts() {
        let policy = PushRetryPolicy::default().with_max_attempts(5);
        assert_eq!(policy.max_attempts, 5);
        // Default backoff should be preserved
        assert_eq!(policy.backoff.len(), 2);
    }

    /// Covers lines 96-99 (PushRetryPolicy::with_backoff).
    #[test]
    fn push_retry_policy_with_backoff() {
        let backoff = vec![
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(500),
            std::time::Duration::from_secs(1),
        ];
        let policy = PushRetryPolicy::default().with_backoff(backoff.clone());
        assert_eq!(policy.backoff, backoff);
        // Default max_attempts should be preserved
        assert_eq!(policy.max_attempts, 3);
    }

    /// Covers lines 149-152 (HttpPushSender::with_retry_policy).
    #[test]
    fn http_push_sender_with_retry_policy() {
        let policy = PushRetryPolicy::default().with_max_attempts(10);
        let sender = HttpPushSender::new().with_retry_policy(policy);
        assert_eq!(sender.retry_policy.max_attempts, 10);
    }

    /// Covers lines 206-208 (validate_webhook_url missing host).
    #[test]
    fn rejects_url_without_host() {
        assert!(validate_webhook_url("http:///path").is_err());
    }

    /// Covers lines 265 and related (HttpPushSender::allow_private_urls).
    #[test]
    fn http_push_sender_allow_private_urls() {
        let sender = HttpPushSender::new().allow_private_urls();
        assert!(sender.allow_private_urls);
    }

    /// Covers Default impl for HttpPushSender (line 122-124).
    #[test]
    fn http_push_sender_default() {
        let sender = HttpPushSender::default();
        assert_eq!(sender.request_timeout, DEFAULT_PUSH_REQUEST_TIMEOUT);
        assert!(!sender.allow_private_urls);
    }

    /// Covers PushRetryPolicy::default() (lines 74-84).
    #[test]
    fn push_retry_policy_default() {
        let policy = PushRetryPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.backoff.len(), 2);
        assert_eq!(policy.backoff[0], std::time::Duration::from_secs(1));
        assert_eq!(policy.backoff[1], std::time::Duration::from_secs(2));
    }

    #[test]
    fn rejects_loopback_ipv4() {
        assert!(validate_webhook_url("http://127.0.0.1:8080/webhook").is_err());
    }

    #[test]
    fn rejects_private_10_range() {
        assert!(validate_webhook_url("http://10.0.0.1/webhook").is_err());
    }

    #[test]
    fn rejects_private_172_range() {
        assert!(validate_webhook_url("http://172.16.0.1/webhook").is_err());
    }

    #[test]
    fn rejects_private_192_168_range() {
        assert!(validate_webhook_url("http://192.168.1.1/webhook").is_err());
    }

    #[test]
    fn rejects_link_local() {
        assert!(validate_webhook_url("http://169.254.169.254/latest").is_err());
    }

    #[test]
    fn rejects_localhost() {
        assert!(validate_webhook_url("http://localhost:8080/webhook").is_err());
    }

    #[test]
    fn rejects_dot_local() {
        assert!(validate_webhook_url("http://myservice.local/webhook").is_err());
    }

    #[test]
    fn rejects_dot_internal() {
        assert!(validate_webhook_url("http://metadata.internal/webhook").is_err());
    }

    #[test]
    fn rejects_ipv6_loopback() {
        assert!(validate_webhook_url("http://[::1]:8080/webhook").is_err());
    }

    #[test]
    fn accepts_public_url() {
        assert!(validate_webhook_url("https://example.com/webhook").is_ok());
    }

    #[test]
    fn accepts_public_ip() {
        assert!(validate_webhook_url("https://203.0.113.1/webhook").is_ok());
    }

    #[test]
    fn rejects_header_with_crlf() {
        assert!(validate_header_value("token\r\nX-Injected: value", "test").is_err());
    }

    #[test]
    fn rejects_header_with_cr() {
        assert!(validate_header_value("token\rvalue", "test").is_err());
    }

    #[test]
    fn rejects_header_with_lf() {
        assert!(validate_header_value("token\nvalue", "test").is_err());
    }

    #[test]
    fn accepts_clean_header_value() {
        assert!(validate_header_value("Bearer abc123+/=", "test").is_ok());
    }

    #[test]
    fn rejects_url_without_scheme() {
        assert!(validate_webhook_url("example.com/webhook").is_err());
    }

    #[test]
    fn rejects_ftp_scheme() {
        assert!(validate_webhook_url("ftp://example.com/webhook").is_err());
    }

    #[test]
    fn rejects_file_scheme() {
        assert!(validate_webhook_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn accepts_http_scheme() {
        assert!(validate_webhook_url("http://example.com/webhook").is_ok());
    }

    #[test]
    fn rejects_cgnat_range() {
        assert!(validate_webhook_url("http://100.64.0.1/webhook").is_err());
    }

    #[test]
    fn rejects_unspecified_ipv4() {
        assert!(validate_webhook_url("http://0.0.0.0/webhook").is_err());
    }

    #[test]
    fn rejects_ipv6_unique_local() {
        assert!(validate_webhook_url("http://[fc00::1]:8080/webhook").is_err());
    }

    #[test]
    fn rejects_ipv6_link_local() {
        assert!(validate_webhook_url("http://[fe80::1]:8080/webhook").is_err());
    }
}
