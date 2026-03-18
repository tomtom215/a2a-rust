// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Static agent card handler with HTTP caching.
//!
//! [`StaticAgentCardHandler`] serves a pre-serialized [`AgentCard`] as JSON.
//! The card is serialized once at construction time and served as raw bytes
//! on every request. Supports HTTP caching via `ETag`, `Last-Modified`,
//! `Cache-Control`, and conditional request headers (spec §8.3).

use a2a_protocol_types::agent_card::AgentCard;
use bytes::Bytes;
use http_body_util::Full;

use crate::agent_card::caching::{
    check_conditional, format_http_date, make_etag, CacheConfig, ConditionalResult,
};
use crate::agent_card::CORS_ALLOW_ALL;
use crate::error::ServerResult;

/// Serves a pre-serialized [`AgentCard`] as a JSON HTTP response with caching.
#[derive(Debug, Clone)]
pub struct StaticAgentCardHandler {
    card_json: Bytes,
    etag: String,
    last_modified: String,
    cache_config: CacheConfig,
}

impl StaticAgentCardHandler {
    /// Creates a new handler by serializing the given [`AgentCard`] to JSON.
    ///
    /// Computes an `ETag` from the serialized content and records the current
    /// time as `Last-Modified`.
    ///
    /// # Errors
    ///
    /// Returns a [`ServerError`](crate::error::ServerError) if serialization fails.
    pub fn new(card: &AgentCard) -> ServerResult<Self> {
        let json = serde_json::to_vec(card)?;
        let etag = make_etag(&json);
        Ok(Self {
            card_json: Bytes::from(json),
            etag,
            last_modified: format_http_date(std::time::SystemTime::now()),
            cache_config: CacheConfig::default(),
        })
    }

    /// Sets the `Cache-Control` max-age in seconds.
    #[must_use]
    pub const fn with_max_age(mut self, seconds: u32) -> Self {
        self.cache_config = CacheConfig::with_max_age(seconds);
        self
    }

    /// Handles an agent card request, returning a cached response.
    ///
    /// Supports conditional requests via `If-None-Match` and `If-Modified-Since`
    /// headers, returning `304 Not Modified` when appropriate.
    #[must_use]
    pub fn handle(
        &self,
        req: &hyper::Request<impl hyper::body::Body>,
    ) -> hyper::Response<Full<Bytes>> {
        let result = check_conditional(req, &self.etag, &self.last_modified);
        match result {
            ConditionalResult::NotModified => self.not_modified_response(),
            ConditionalResult::SendFull => self.full_response(),
        }
    }

    /// Handles a request without conditional headers (legacy compatibility).
    #[must_use]
    pub fn handle_unconditional(&self) -> hyper::Response<Full<Bytes>> {
        self.full_response()
    }

    fn full_response(&self) -> hyper::Response<Full<Bytes>> {
        hyper::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("access-control-allow-origin", CORS_ALLOW_ALL)
            .header("etag", &self.etag)
            .header("last-modified", &self.last_modified)
            .header("cache-control", self.cache_config.header_value())
            .body(Full::new(self.card_json.clone()))
            .unwrap_or_else(|_| hyper::Response::new(Full::new(Bytes::new())))
    }

    fn not_modified_response(&self) -> hyper::Response<Full<Bytes>> {
        hyper::Response::builder()
            .status(304)
            .header("etag", &self.etag)
            .header("last-modified", &self.last_modified)
            .header("cache-control", self.cache_config.header_value())
            .body(Full::new(Bytes::new()))
            .unwrap_or_else(|_| hyper::Response::new(Full::new(Bytes::new())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_card::caching::tests::minimal_agent_card;

    #[test]
    fn static_handler_returns_etag_and_cache_headers() {
        let card = minimal_agent_card();
        let handler = StaticAgentCardHandler::new(&card).unwrap();
        let req = hyper::Request::builder()
            .method("GET")
            .uri("/.well-known/agent.json")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req);
        assert_eq!(resp.status(), 200);
        assert!(resp.headers().contains_key("etag"));
        assert!(resp.headers().contains_key("last-modified"));
        assert!(resp.headers().contains_key("cache-control"));
    }

    #[test]
    fn static_handler_304_on_matching_etag() {
        let card = minimal_agent_card();
        let handler = StaticAgentCardHandler::new(&card).unwrap();
        // First request to get the etag.
        let req1 = hyper::Request::builder()
            .method("GET")
            .uri("/.well-known/agent.json")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp1 = handler.handle(&req1);
        let etag = resp1
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();

        // Second request with If-None-Match.
        let req2 = hyper::Request::builder()
            .method("GET")
            .uri("/.well-known/agent.json")
            .header("if-none-match", &etag)
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp2 = handler.handle(&req2);
        assert_eq!(resp2.status(), 304);
    }

    #[test]
    fn static_handler_200_on_mismatched_etag() {
        let card = minimal_agent_card();
        let handler = StaticAgentCardHandler::new(&card).unwrap();
        let req = hyper::Request::builder()
            .method("GET")
            .uri("/.well-known/agent.json")
            .header("if-none-match", "\"wrong-etag\"")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req);
        assert_eq!(resp.status(), 200);
    }

    /// Covers lines 75-77 (`handle_unconditional`).
    #[test]
    fn static_handler_unconditional_returns_200() {
        let card = minimal_agent_card();
        let handler = StaticAgentCardHandler::new(&card).unwrap();
        let resp = handler.handle_unconditional();
        assert_eq!(resp.status(), 200);
        assert!(resp.headers().contains_key("etag"));
        assert!(resp.headers().contains_key("content-type"));
        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );
    }

    #[test]
    fn static_handler_custom_max_age() {
        let card = minimal_agent_card();
        let handler = StaticAgentCardHandler::new(&card)
            .unwrap()
            .with_max_age(7200);
        let req = hyper::Request::builder()
            .method("GET")
            .uri("/.well-known/agent.json")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let resp = handler.handle(&req);
        let cc = resp
            .headers()
            .get("cache-control")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cc.contains("max-age=7200"));
    }
}
