// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Push notification sender trait and HTTP implementation.
//!
//! [`PushSender`] abstracts the delivery of streaming events to client webhook
//! endpoints. [`HttpPushSender`] uses hyper to POST events over HTTP(S).

use std::future::Future;
use std::pin::Pin;

use a2a_types::error::{A2aError, A2aResult};
use a2a_types::events::StreamResponse;
use a2a_types::push::TaskPushNotificationConfig;
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

/// HTTP-based [`PushSender`] using hyper.
///
/// Retries up to 3 times with 1s, 2s backoff on transient HTTP errors.
#[derive(Debug)]
pub struct HttpPushSender {
    client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
}

impl Default for HttpPushSender {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpPushSender {
    /// Creates a new [`HttpPushSender`].
    #[must_use]
    pub fn new() -> Self {
        let client = Client::builder(TokioExecutor::new()).build_http();
        Self { client }
    }
}

#[allow(clippy::manual_async_fn)]
impl PushSender for HttpPushSender {
    fn send<'a>(
        &'a self,
        url: &'a str,
        event: &'a StreamResponse,
        config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            trace_info!(url, "delivering push notification");
            let body_bytes = serde_json::to_vec(event)
                .map_err(|e| A2aError::internal(format!("push serialization: {e}")))?;

            let backoff = [
                std::time::Duration::from_secs(1),
                std::time::Duration::from_secs(2),
            ];

            let max_attempts: usize = 3;
            let mut last_err = String::new();

            for attempt in 0..max_attempts {
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
                        _ => {}
                    }
                }

                // Set notification token header if present.
                if let Some(ref token) = config.token {
                    builder = builder.header("a2a-notification-token", token.as_str());
                }

                let req = builder
                    .body(Full::new(Bytes::from(body_bytes.clone())))
                    .map_err(|e| A2aError::internal(format!("push request build: {e}")))?;

                match self.client.request(req).await {
                    Ok(resp) if resp.status().is_success() => {
                        trace_debug!(url, "push notification delivered");
                        return Ok(());
                    }
                    Ok(resp) => {
                        last_err = format!("push notification got HTTP {}", resp.status());
                        trace_warn!(url, attempt, status = %resp.status(), "push delivery failed");
                    }
                    Err(e) => {
                        last_err = format!("push notification failed: {e}");
                        trace_warn!(url, attempt, error = %e, "push delivery error");
                    }
                }

                // Retry with backoff (except on last attempt).
                if attempt < max_attempts - 1 {
                    if let Some(delay) = backoff.get(attempt) {
                        tokio::time::sleep(*delay).await;
                    }
                }
            }

            Err(A2aError::internal(last_err))
        })
    }
}
