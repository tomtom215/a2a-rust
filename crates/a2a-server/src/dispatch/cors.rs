// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! CORS (Cross-Origin Resource Sharing) configuration for A2A dispatchers.
//!
//! Browser-based A2A clients need CORS headers to interact with agents.
//! [`CorsConfig`] provides configurable CORS support that can be applied to
//! both [`RestDispatcher`](super::RestDispatcher) and
//! [`JsonRpcDispatcher`](super::JsonRpcDispatcher).

use std::convert::Infallible;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};

/// CORS configuration for A2A dispatchers.
///
/// # Examples
///
/// ```
/// use a2a_protocol_server::dispatch::cors::CorsConfig;
///
/// // Allow all origins (development/testing).
/// let cors = CorsConfig::permissive();
///
/// // Restrict to a specific origin.
/// let cors = CorsConfig::new("https://my-app.example.com");
/// ```
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// The `Access-Control-Allow-Origin` value.
    pub allow_origin: String,
    /// The `Access-Control-Allow-Methods` value.
    pub allow_methods: String,
    /// The `Access-Control-Allow-Headers` value.
    pub allow_headers: String,
    /// The `Access-Control-Max-Age` value in seconds.
    pub max_age_secs: u32,
}

impl CorsConfig {
    /// Creates a new [`CorsConfig`] with the given allowed origin.
    #[must_use]
    pub fn new(allow_origin: impl Into<String>) -> Self {
        Self {
            allow_origin: allow_origin.into(),
            allow_methods: "GET, POST, PUT, DELETE, OPTIONS".into(),
            allow_headers: "content-type, authorization, a2a-notification-token".into(),
            max_age_secs: 86400,
        }
    }

    /// Creates a permissive [`CorsConfig`] that allows all origins.
    ///
    /// Suitable for development or public APIs. For production use,
    /// prefer [`CorsConfig::new`] with a specific origin.
    #[must_use]
    pub fn permissive() -> Self {
        Self::new("*")
    }

    /// Applies CORS headers to an existing HTTP response.
    pub fn apply_headers<B>(&self, resp: &mut hyper::Response<B>) {
        let headers = resp.headers_mut();
        // These `parse()` calls only fail on invalid header values containing
        // control characters, which our constructors don't produce.
        if let Ok(v) = self.allow_origin.parse() {
            headers.insert("access-control-allow-origin", v);
        }
        if let Ok(v) = self.allow_methods.parse() {
            headers.insert("access-control-allow-methods", v);
        }
        if let Ok(v) = self.allow_headers.parse() {
            headers.insert("access-control-allow-headers", v);
        }
        if let Ok(v) = self.max_age_secs.to_string().parse() {
            headers.insert("access-control-max-age", v);
        }
    }

    /// Builds a preflight (OPTIONS) response with CORS headers.
    #[must_use]
    pub fn preflight_response(&self) -> hyper::Response<BoxBody<Bytes, Infallible>> {
        let mut resp = hyper::Response::builder()
            .status(204)
            .body(Full::new(Bytes::new()).boxed())
            .unwrap_or_else(|_| hyper::Response::new(Full::new(Bytes::new()).boxed()));
        self.apply_headers(&mut resp);
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sets_origin_and_defaults() {
        let cors = CorsConfig::new("https://example.com");

        assert_eq!(cors.allow_origin, "https://example.com");
        assert_eq!(
            cors.allow_methods, "GET, POST, PUT, DELETE, OPTIONS",
            "default methods should include common HTTP verbs"
        );
        assert_eq!(
            cors.allow_headers,
            "content-type, authorization, a2a-notification-token",
            "default headers should include content-type, authorization, and a2a-notification-token"
        );
        assert_eq!(
            cors.max_age_secs, 86400,
            "default max-age should be 24 hours"
        );
    }

    #[test]
    fn new_accepts_string_and_str() {
        let from_str = CorsConfig::new("https://a.com");
        let from_string = CorsConfig::new(String::from("https://b.com"));

        assert_eq!(from_str.allow_origin, "https://a.com");
        assert_eq!(from_string.allow_origin, "https://b.com");
    }

    #[test]
    fn permissive_allows_all_origins() {
        let cors = CorsConfig::permissive();
        assert_eq!(
            cors.allow_origin, "*",
            "permissive config should use wildcard origin"
        );
    }

    #[test]
    fn apply_headers_sets_all_cors_headers() {
        let cors = CorsConfig::new("https://app.example.com");
        let mut resp = hyper::Response::new(Full::new(Bytes::new()).boxed());
        cors.apply_headers(&mut resp);

        let headers = resp.headers();
        assert_eq!(
            headers.get("access-control-allow-origin").unwrap(),
            "https://app.example.com"
        );
        assert_eq!(
            headers.get("access-control-allow-methods").unwrap(),
            "GET, POST, PUT, DELETE, OPTIONS"
        );
        assert_eq!(
            headers.get("access-control-allow-headers").unwrap(),
            "content-type, authorization, a2a-notification-token"
        );
        assert_eq!(headers.get("access-control-max-age").unwrap(), "86400");
    }

    #[test]
    fn apply_headers_with_custom_config() {
        let mut cors = CorsConfig::new("https://custom.dev");
        cors.allow_methods = "POST, OPTIONS".into();
        cors.allow_headers = "content-type".into();
        cors.max_age_secs = 3600;

        let mut resp = hyper::Response::new(Full::new(Bytes::new()).boxed());
        cors.apply_headers(&mut resp);

        let headers = resp.headers();
        assert_eq!(
            headers.get("access-control-allow-origin").unwrap(),
            "https://custom.dev"
        );
        assert_eq!(
            headers.get("access-control-allow-methods").unwrap(),
            "POST, OPTIONS",
            "custom methods should be applied"
        );
        assert_eq!(
            headers.get("access-control-allow-headers").unwrap(),
            "content-type",
            "custom headers should be applied"
        );
        assert_eq!(
            headers.get("access-control-max-age").unwrap(),
            "3600",
            "custom max-age should be applied"
        );
    }

    #[test]
    fn apply_headers_overwrites_existing_cors_headers() {
        let cors = CorsConfig::new("https://second.com");
        let mut resp = hyper::Response::builder()
            .header("access-control-allow-origin", "https://first.com")
            .body(Full::new(Bytes::new()).boxed())
            .unwrap();

        cors.apply_headers(&mut resp);

        assert_eq!(
            resp.headers().get("access-control-allow-origin").unwrap(),
            "https://second.com",
            "apply_headers should overwrite pre-existing CORS headers"
        );
    }

    #[test]
    fn preflight_response_returns_204_no_content() {
        let cors = CorsConfig::permissive();
        let resp = cors.preflight_response();

        assert_eq!(
            resp.status().as_u16(),
            204,
            "preflight response should have 204 No Content status"
        );
    }

    #[test]
    fn preflight_response_includes_cors_headers() {
        let cors = CorsConfig::new("https://preflight.test");
        let resp = cors.preflight_response();

        let headers = resp.headers();
        assert_eq!(
            headers.get("access-control-allow-origin").unwrap(),
            "https://preflight.test"
        );
        assert!(
            headers.get("access-control-allow-methods").is_some(),
            "preflight response must include allow-methods header"
        );
        assert!(
            headers.get("access-control-allow-headers").is_some(),
            "preflight response must include allow-headers header"
        );
        assert!(
            headers.get("access-control-max-age").is_some(),
            "preflight response must include max-age header"
        );
    }

    #[test]
    fn config_is_cloneable() {
        let cors = CorsConfig::new("https://clone.test");
        let cloned = cors.clone();
        assert_eq!(cors.allow_origin, cloned.allow_origin);
        assert_eq!(cors.allow_methods, cloned.allow_methods);
        assert_eq!(cors.allow_headers, cloned.allow_headers);
        assert_eq!(cors.max_age_secs, cloned.max_age_secs);
    }

    #[test]
    fn max_age_zero_is_valid() {
        let mut cors = CorsConfig::permissive();
        cors.max_age_secs = 0;

        let mut resp = hyper::Response::new(Full::new(Bytes::new()).boxed());
        cors.apply_headers(&mut resp);

        assert_eq!(
            resp.headers().get("access-control-max-age").unwrap(),
            "0",
            "max-age of 0 should be set correctly"
        );
    }
}
