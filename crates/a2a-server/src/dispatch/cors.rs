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
/// use a2a_server::dispatch::cors::CorsConfig;
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
