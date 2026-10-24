// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `SERVER` span and `http.server.request.duration` of a request for the
//! agent card at `/.well-known/agent-card.json` (spec §8.2), on every
//! listener that answers one: JSON-RPC, HTTP+JSON, the axum router,
//! WebSocket and gRPC.
//!
//! A card request is HTTP, not an A2A call, so it follows the HTTP
//! conventions (`docs/http/http-spans.md`, `http-metrics.md`,
//! `open-telemetry/semantic-conventions@55c98cf`):
//!
//! * named `{http.request.method} /.well-known/agent-card.json`, or `HTTP`
//!   and the route for a method the conventions do not know (`_OTHER`);
//! * `http.request.method`, `url.path`, `url.scheme` (Required);
//!   `http.response.status_code`, `http.route`, `http.request.method_original`
//!   for `_OTHER`, `url.query` when there is one (Conditionally Required);
//!   `server.address` and `server.port` from `Host`, `network.peer.*` from the
//!   connection, `network.protocol.version` and `user_agent.original`
//!   (Recommended);
//! * `error.type` and `Error` status for a 5xx only: a server span's status
//!   "MUST be left unset" for a 4xx.
//!
//! `url.query` is recorded as `REDACTED`: the card takes no query, so one is
//! the caller's, and the conventions ask a query's sensitive values to be
//! scrubbed. `url.scheme` is `https` when this server's TLS terminated the
//! connection, else the request URI's scheme, else a `X-Forwarded-Proto` of
//! `http` or `https` (the conventions' "if known" for the original
//! request), else `http`.
//!
//! The span follows the handler's [`InboundTracePolicy`] as a call's does:
//! the caller's `traceparent` is its parent under `Continue`, a new trace
//! starts under `Restart`, and under `Drop` there is no span. The duration
//! goes to the global meter provider as it is at the measurement, under the
//! `a2a.server` scope, with the method, scheme, route, status,
//! `network.protocol.version` and, for a 5xx, `error.type`: nothing the
//! caller chooses freely, so the series stay bounded.

use crate::handler::InboundTracePolicy;

/// The route the card is served at.
#[cfg_attr(not(feature = "tracing"), allow(dead_code))]
pub const ROUTE: &str = "/.well-known/agent-card.json";

/// The HTTP methods the conventions know; any other is `_OTHER`.
const KNOWN: [&str; 9] = [
    "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT", "TRACE",
];

/// What the listener knows about the connection a card request came on.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardConnection {
    /// The peer's address, when the listener knows it.
    #[cfg_attr(not(feature = "tracing"), allow(dead_code))]
    pub peer: Option<std::net::SocketAddr>,
    /// Whether this server's TLS terminated the connection.
    pub tls: bool,
}

/// One card request, measured from [`start`](Self::start) to
/// [`finish`](Self::finish).
#[derive(Debug)]
#[must_use = "finish it with the response, or nothing is recorded"]
pub struct CardCall {
    #[cfg(feature = "tracing")]
    span: tracing::Span,
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    started: std::time::Instant,
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    method: &'static str,
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    scheme: &'static str,
    #[cfg_attr(not(feature = "otel"), allow(dead_code))]
    version: &'static str,
}

impl CardCall {
    /// Opens the span for `req`, under the handler's trace `policy`.
    #[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
    pub fn start<B>(
        policy: InboundTracePolicy,
        req: &hyper::Request<B>,
        connection: CardConnection,
    ) -> Self {
        let method = KNOWN
            .iter()
            .copied()
            .find(|m| *m == req.method().as_str())
            .unwrap_or("_OTHER");
        let scheme = scheme(req, connection.tls);
        let version = version(req.version());
        #[cfg(feature = "tracing")]
        let span = if policy == InboundTracePolicy::Drop {
            tracing::Span::none()
        } else {
            let span = open(req, method, scheme, version, connection);
            #[cfg(feature = "otel")]
            if policy == InboundTracePolicy::Continue
                && let Some(parent) = super::otel::remote_parent(&trace_headers(req))
            {
                use tracing_opentelemetry::OpenTelemetrySpanExt as _;
                let _ = span.set_parent(parent);
            }
            span
        };
        Self {
            #[cfg(feature = "tracing")]
            span,
            started: std::time::Instant::now(),
            method,
            scheme,
            version,
        }
    }

    /// Records how the request was answered: its status on the span, and
    /// its duration.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::unused_self, clippy::needless_pass_by_value)
    )]
    pub fn finish<B>(self, resp: &hyper::Response<B>) {
        let status = resp.status().as_u16();
        let failed = status >= 500;
        #[cfg(feature = "tracing")]
        {
            self.span
                .record("http.response.status_code", i64::from(status));
            if failed {
                self.span.record("error.type", status.to_string().as_str());
                self.span.record("otel.status_code", "ERROR");
            }
        }
        #[cfg(feature = "otel")]
        record_duration(&self, status, failed);
    }
}

/// `url.scheme`, as the module docs describe.
fn scheme<B>(req: &hyper::Request<B>, tls: bool) -> &'static str {
    if tls {
        return "https";
    }
    match req.uri().scheme_str() {
        Some("https") => return "https",
        Some("http") => return "http",
        _ => {}
    }
    match req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
    {
        Some(p) if p.trim().eq_ignore_ascii_case("https") => "https",
        _ => "http",
    }
}

/// `network.protocol.version`.
const fn version(v: hyper::Version) -> &'static str {
    match v {
        hyper::Version::HTTP_09 => "0.9",
        hyper::Version::HTTP_10 => "1.0",
        hyper::Version::HTTP_2 => "2",
        hyper::Version::HTTP_3 => "3",
        _ => "1.1",
    }
}

/// The span, every field declared up front.
#[cfg(feature = "tracing")]
fn open<B>(
    req: &hyper::Request<B>,
    method: &'static str,
    scheme: &'static str,
    version: &'static str,
    connection: CardConnection,
) -> tracing::Span {
    let name = if method == "_OTHER" {
        "HTTP /.well-known/agent-card.json".to_owned()
    } else {
        format!("{method} {ROUTE}")
    };
    let span = tracing::info_span!(
        target: "a2a_protocol_server::card",
        "a2a.card",
        otel.name = name.as_str(),
        otel.kind = "server",
        otel.status_code = tracing::field::Empty,
        http.request.method = method,
        http.request.method_original = tracing::field::Empty,
        http.route = ROUTE,
        url.path = req.uri().path(),
        url.scheme = scheme,
        url.query = tracing::field::Empty,
        http.response.status_code = tracing::field::Empty,
        error.type = tracing::field::Empty,
        network.protocol.version = version,
        network.peer.address = tracing::field::Empty,
        network.peer.port = tracing::field::Empty,
        server.address = tracing::field::Empty,
        server.port = tracing::field::Empty,
        user_agent.original = tracing::field::Empty,
    );
    record_request(&span, req, method, connection);
    span
}

/// The span's attributes that depend on what the request carries.
#[cfg(feature = "tracing")]
fn record_request<B>(
    span: &tracing::Span,
    req: &hyper::Request<B>,
    method: &str,
    connection: CardConnection,
) {
    if method == "_OTHER" {
        span.record(
            "http.request.method_original",
            super::truncated(req.method().as_str(), super::METHOD_ORIGINAL_MAX),
        );
    }
    if req.uri().query().is_some() {
        span.record("url.query", "REDACTED");
    }
    if let Some(peer) = connection.peer {
        span.record("network.peer.address", peer.ip().to_string().as_str());
        span.record("network.peer.port", i64::from(peer.port()));
    }
    if let Some((host, port)) = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .and_then(super::address::server)
    {
        span.record("server.address", host.as_str());
        if let Some(port) = port {
            span.record("server.port", i64::from(port));
        }
    }
    if let Some(agent) = req
        .headers()
        .get(hyper::header::USER_AGENT)
        .and_then(|h| h.to_str().ok())
    {
        span.record(
            "user_agent.original",
            super::truncated(agent, USER_AGENT_MAX),
        );
    }
}

/// The longest `user_agent.original` recorded, in bytes.
#[cfg(feature = "tracing")]
const USER_AGENT_MAX: usize = 256;

/// `traceparent` and `tracestate`, as the call spans read them.
#[cfg(feature = "otel")]
fn trace_headers<B>(req: &hyper::Request<B>) -> std::collections::HashMap<String, String> {
    use a2a_protocol_types::trace_context::{TRACEPARENT_HEADER, TRACESTATE_HEADER};
    [TRACEPARENT_HEADER, TRACESTATE_HEADER]
        .into_iter()
        .filter_map(|name| {
            let value = crate::dispatch::field_value(req.headers(), name)?;
            Some((name.to_owned(), value))
        })
        .collect()
}

/// The HTTP conventions' advised buckets for `http.server.request.duration`.
#[cfg(feature = "otel")]
const DURATION_BUCKETS: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];

/// Records `call` in `http.server.request.duration`.
#[cfg(feature = "otel")]
fn record_duration(call: &CardCall, status: u16, failed: bool) {
    use opentelemetry::KeyValue;
    let histogram = opentelemetry::global::meter("a2a.server")
        .f64_histogram("http.server.request.duration")
        .with_unit("s")
        .with_description("Duration of HTTP server requests.")
        .with_boundaries(DURATION_BUCKETS.to_vec())
        .build();
    let mut attributes = vec![
        KeyValue::new("http.request.method", call.method),
        KeyValue::new("url.scheme", call.scheme),
        KeyValue::new("http.route", ROUTE),
        KeyValue::new("http.response.status_code", i64::from(status)),
        KeyValue::new("network.protocol.version", call.version),
    ];
    if failed {
        attributes.push(KeyValue::new("error.type", status.to_string()));
    }
    histogram.record(call.started.elapsed().as_secs_f64(), &attributes);
}

#[cfg(test)]
mod tests {
    use super::{scheme, version};

    fn request(uri: &str, forwarded: Option<&str>) -> hyper::Request<()> {
        let mut builder = hyper::Request::get(uri);
        if let Some(proto) = forwarded {
            builder = builder.header("x-forwarded-proto", proto);
        }
        builder.body(()).unwrap()
    }

    /// The scheme of an absolute request URI is the request's; only an
    /// origin-form one falls back to `X-Forwarded-Proto`; this server's TLS
    /// is `https` whatever either says.
    #[test]
    fn the_scheme_is_the_uris_then_the_forwarded_header() {
        let card = "/.well-known/agent-card.json";
        assert_eq!(scheme(&request(card, None), true), "https");
        assert_eq!(
            scheme(&request("https://a.example/x", None), false),
            "https"
        );
        assert_eq!(
            scheme(&request("http://a.example/x", Some("https")), false),
            "http"
        );
        assert_eq!(scheme(&request(card, Some(" HTTPS ")), false), "https");
        assert_eq!(scheme(&request(card, Some("http")), false), "http");
        assert_eq!(scheme(&request(card, None), false), "http");
    }

    #[test]
    fn each_http_version_has_its_conventions_name() {
        assert_eq!(version(hyper::Version::HTTP_09), "0.9");
        assert_eq!(version(hyper::Version::HTTP_10), "1.0");
        assert_eq!(version(hyper::Version::HTTP_11), "1.1");
        assert_eq!(version(hyper::Version::HTTP_2), "2");
        assert_eq!(version(hyper::Version::HTTP_3), "3");
    }
}
