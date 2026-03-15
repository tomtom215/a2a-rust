// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Dynamic agent card handler with HTTP caching.
//!
//! [`DynamicAgentCardHandler`] calls an [`AgentCardProducer`] on every request
//! to generate a fresh [`AgentCard`]. This is useful when the card contents
//! depend on runtime state (e.g. feature flags, authenticated context).
//! Supports HTTP caching via `ETag` and conditional request headers (spec §8.3).

use std::future::Future;
use std::pin::Pin;

use a2a_types::agent_card::AgentCard;
use a2a_types::error::A2aResult;
use bytes::Bytes;
use http_body_util::Full;

use crate::agent_card::caching::{format_http_date, make_etag, CacheConfig};
use crate::agent_card::CORS_ALLOW_ALL;

/// Trait for producing an [`AgentCard`] dynamically.
///
/// Object-safe; used behind `Arc<dyn AgentCardProducer>` or as a generic bound.
pub trait AgentCardProducer: Send + Sync + 'static {
    /// Produces an [`AgentCard`] for the current request.
    ///
    /// # Errors
    ///
    /// Returns an [`A2aError`](a2a_types::error::A2aError) if card generation fails.
    fn produce<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>>;
}

/// Serves a dynamically generated [`AgentCard`] as a JSON HTTP response.
#[derive(Debug)]
pub struct DynamicAgentCardHandler<P> {
    producer: P,
    cache_config: CacheConfig,
}

impl<P: AgentCardProducer> DynamicAgentCardHandler<P> {
    /// Creates a new handler with the given producer.
    #[must_use]
    pub fn new(producer: P) -> Self {
        Self {
            producer,
            cache_config: CacheConfig::default(),
        }
    }

    /// Sets the `Cache-Control` max-age in seconds.
    #[must_use]
    pub const fn with_max_age(mut self, seconds: u32) -> Self {
        self.cache_config = CacheConfig::with_max_age(seconds);
        self
    }

    /// Handles an agent card request with conditional caching support.
    ///
    /// Serializes the produced card, computes an `ETag`, and checks
    /// conditional request headers before returning the response.
    ///
    /// # Panics
    ///
    /// Panics if the response builder fails (should never happen).
    #[allow(clippy::future_not_send)] // Body impl may not be Sync
    pub async fn handle(
        &self,
        req: &hyper::Request<impl hyper::body::Body>,
    ) -> hyper::Response<Full<Bytes>> {
        // Extract conditional headers before the await point so the
        // non-Send `impl Body` reference doesn't cross it.
        let if_none_match = req
            .headers()
            .get("if-none-match")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let if_modified_since = req
            .headers()
            .get("if-modified-since")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        match self.producer.produce().await {
            Ok(card) => match serde_json::to_vec(&card) {
                Ok(json) => {
                    let etag = make_etag(&json);
                    let last_modified = format_http_date(std::time::SystemTime::now());

                    let not_modified = is_not_modified(
                        if_none_match.as_deref(),
                        if_modified_since.as_deref(),
                        &etag,
                        &last_modified,
                    );

                    if not_modified {
                        hyper::Response::builder()
                            .status(304)
                            .header("etag", &etag)
                            .header("last-modified", &last_modified)
                            .header("cache-control", self.cache_config.header_value())
                            .body(Full::new(Bytes::new()))
                            .expect("response builder should not fail with valid headers")
                    } else {
                        hyper::Response::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("access-control-allow-origin", CORS_ALLOW_ALL)
                            .header("etag", &etag)
                            .header("last-modified", &last_modified)
                            .header("cache-control", self.cache_config.header_value())
                            .body(Full::new(Bytes::from(json)))
                            .expect("response builder should not fail with valid headers")
                    }
                }
                Err(e) => error_response(500, &format!("serialization error: {e}")),
            },
            Err(e) => error_response(500, &format!("card producer error: {e}")),
        }
    }

    /// Handles a request without conditional headers (legacy compatibility).
    ///
    /// # Panics
    ///
    /// Panics if the response builder fails (should never happen).
    pub async fn handle_unconditional(&self) -> hyper::Response<Full<Bytes>> {
        match self.producer.produce().await {
            Ok(card) => match serde_json::to_vec(&card) {
                Ok(json) => {
                    let etag = make_etag(&json);
                    let last_modified = format_http_date(std::time::SystemTime::now());
                    hyper::Response::builder()
                        .status(200)
                        .header("content-type", "application/json")
                        .header("access-control-allow-origin", CORS_ALLOW_ALL)
                        .header("etag", &etag)
                        .header("last-modified", &last_modified)
                        .header("cache-control", self.cache_config.header_value())
                        .body(Full::new(Bytes::from(json)))
                        .expect("response builder should not fail with valid headers")
                }
                Err(e) => error_response(500, &format!("serialization error: {e}")),
            },
            Err(e) => error_response(500, &format!("card producer error: {e}")),
        }
    }
}

/// Checks whether the response should be 304 using pre-extracted header values.
fn is_not_modified(
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    current_etag: &str,
    current_last_modified: &str,
) -> bool {
    // If-None-Match takes precedence per RFC 7232 §6.
    if let Some(inm) = if_none_match {
        return etag_matches(inm, current_etag);
    }
    if let Some(ims) = if_modified_since {
        return ims == current_last_modified;
    }
    false
}

/// Weak `ETag` comparison for `If-None-Match` header values.
fn etag_matches(header_value: &str, current: &str) -> bool {
    let header_value = header_value.trim();
    if header_value == "*" {
        return true;
    }
    let current_bare = current.strip_prefix("W/").unwrap_or(current);
    for candidate in header_value.split(',') {
        let candidate = candidate.trim();
        let candidate_bare = candidate.strip_prefix("W/").unwrap_or(candidate);
        if candidate_bare == current_bare {
            return true;
        }
    }
    false
}

/// Builds a simple JSON error response.
fn error_response(status: u16, message: &str) -> hyper::Response<Full<Bytes>> {
    let body = serde_json::json!({ "error": message });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    hyper::Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(bytes)))
        .expect("response builder should not fail with valid headers")
}
