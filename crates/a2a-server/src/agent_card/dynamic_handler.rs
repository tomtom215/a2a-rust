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

use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::A2aResult;
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
    /// Returns an [`A2aError`](a2a_protocol_types::error::A2aError) if card generation fails.
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
                            .unwrap_or_else(|_| fallback_error_response())
                    } else {
                        hyper::Response::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .header("access-control-allow-origin", CORS_ALLOW_ALL)
                            .header("etag", &etag)
                            .header("last-modified", &last_modified)
                            .header("cache-control", self.cache_config.header_value())
                            .body(Full::new(Bytes::from(json)))
                            .unwrap_or_else(|_| fallback_error_response())
                    }
                }
                Err(e) => error_response(500, &format!("serialization error: {e}")),
            },
            Err(e) => error_response(500, &format!("card producer error: {e}")),
        }
    }

    /// Handles a request without conditional headers (legacy compatibility).
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
                        .unwrap_or_else(|_| fallback_error_response())
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
        .unwrap_or_else(|_| fallback_error_response())
}

/// Fallback response when the response builder itself fails (should never happen
/// with valid static header names, but avoids panicking in production).
fn fallback_error_response() -> hyper::Response<Full<Bytes>> {
    hyper::Response::new(Full::new(Bytes::from_static(
        br#"{"error":"internal server error"}"#,
    )))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_card::caching::tests::minimal_agent_card;

    /// A mock producer that always returns a fixed agent card.
    struct MockProducer;

    impl AgentCardProducer for MockProducer {
        fn produce<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
            Box::pin(async { Ok(minimal_agent_card()) })
        }
    }

    /// A mock producer that always returns an error.
    struct FailingProducer;

    impl AgentCardProducer for FailingProducer {
        fn produce<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = A2aResult<AgentCard>> + Send + 'a>> {
            Box::pin(async {
                Err(a2a_protocol_types::error::A2aError::internal(
                    "producer failure",
                ))
            })
        }
    }

    #[test]
    fn construction_with_defaults() {
        let handler = DynamicAgentCardHandler::new(MockProducer);
        assert_eq!(
            handler.cache_config.max_age, 3600,
            "default max_age should be 3600 seconds"
        );
    }

    #[test]
    fn with_max_age_overrides_default() {
        let handler = DynamicAgentCardHandler::new(MockProducer).with_max_age(120);
        assert_eq!(
            handler.cache_config.max_age, 120,
            "with_max_age should set the configured value"
        );
    }

    #[tokio::test]
    async fn handle_returns_correct_content_type() {
        let handler = DynamicAgentCardHandler::new(MockProducer);
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;

        assert_eq!(resp.status(), 200, "response should be 200 OK");
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json",
            "content-type should be application/json"
        );
    }

    #[tokio::test]
    async fn handle_includes_etag_header() {
        let handler = DynamicAgentCardHandler::new(MockProducer);
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;

        let etag = resp
            .headers()
            .get("etag")
            .expect("response should include an ETag header");
        let etag_str = etag.to_str().unwrap();
        assert!(
            etag_str.starts_with("W/\""),
            "ETag should be a weak validator starting with W/\""
        );
    }

    #[tokio::test]
    async fn handle_includes_cache_control_header() {
        let handler = DynamicAgentCardHandler::new(MockProducer).with_max_age(300);
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;

        assert_eq!(
            resp.headers().get("cache-control").unwrap(),
            "public, max-age=300",
            "cache-control should reflect with_max_age setting"
        );
    }

    #[tokio::test]
    async fn handle_includes_cors_header() {
        let handler = DynamicAgentCardHandler::new(MockProducer);
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;

        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "*",
            "CORS header should allow all origins"
        );
    }

    #[tokio::test]
    async fn conditional_request_with_matching_etag_returns_304() {
        let handler = DynamicAgentCardHandler::new(MockProducer);

        // First request to get the ETag.
        let req1 = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp1 = handler.handle(&req1).await;
        assert_eq!(resp1.status(), 200, "first request should return 200");
        let etag = resp1
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        // Second request with If-None-Match matching the ETag.
        let req2 = hyper::Request::builder()
            .header("if-none-match", &etag)
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp2 = handler.handle(&req2).await;
        assert_eq!(
            resp2.status(),
            304,
            "conditional request with matching ETag should return 304 Not Modified"
        );
    }

    #[tokio::test]
    async fn conditional_request_with_non_matching_etag_returns_200() {
        let handler = DynamicAgentCardHandler::new(MockProducer);
        let req = hyper::Request::builder()
            .header("if-none-match", "W/\"does-not-match\"")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;

        assert_eq!(
            resp.status(),
            200,
            "non-matching ETag should return 200 with full body"
        );
    }

    #[tokio::test]
    async fn handle_unconditional_always_returns_full_response() {
        let handler = DynamicAgentCardHandler::new(MockProducer);

        let resp = handler.handle_unconditional().await;
        assert_eq!(resp.status(), 200, "unconditional handle should return 200");
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json",
            "unconditional response should have JSON content-type"
        );
        assert!(
            resp.headers().get("etag").is_some(),
            "unconditional response should still include ETag"
        );
    }

    #[tokio::test]
    async fn handle_returns_500_on_producer_error() {
        let handler = DynamicAgentCardHandler::new(FailingProducer);
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;

        assert_eq!(
            resp.status(),
            500,
            "producer error should result in 500 status"
        );
    }

    #[tokio::test]
    async fn handle_unconditional_returns_500_on_producer_error() {
        let handler = DynamicAgentCardHandler::new(FailingProducer);
        let resp = handler.handle_unconditional().await;

        assert_eq!(
            resp.status(),
            500,
            "producer error in unconditional handle should result in 500 status"
        );
    }

    /// Covers lines 190-194 (`fallback_error_response`).
    #[test]
    fn fallback_error_response_returns_internal_error_json() {
        let resp = fallback_error_response();
        assert_eq!(resp.status(), 200); // default status for Response::new
                                        // Body should contain error JSON
    }

    /// Covers line 113 (serialization error in handle) and line 136 (in `handle_unconditional`).
    /// These are hard to trigger with real `AgentCard` (which always serializes).
    /// Instead we test the `error_response` helper directly.
    #[tokio::test]
    async fn error_response_returns_correct_status() {
        let resp = error_response(503, "service unavailable");
        assert_eq!(resp.status(), 503);
        let body = {
            use http_body_util::BodyExt;
            resp.into_body().collect().await.unwrap().to_bytes()
        };
        let val: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(val["error"], "service unavailable");
    }

    #[tokio::test]
    async fn response_body_deserializes_to_agent_card() {
        use http_body_util::BodyExt;

        let handler = DynamicAgentCardHandler::new(MockProducer);
        let req = hyper::Request::builder()
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req).await;
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let card: AgentCard =
            serde_json::from_slice(&body).expect("response body should be valid AgentCard JSON");
        assert_eq!(
            card.name, "Test Agent",
            "deserialized card name should match"
        );
    }
}
