// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a lost export says beyond the exporter's own words.
//!
//! `opentelemetry-otlp` 0.33.1 reports every OTLP/HTTP transport failure as
//! the fixed text `HTTP export failed: network error` and keeps the cause for
//! a debug log (`exporter/http/mod.rs`, `HttpClient.NetworkError`). A refused
//! connection, an unknown certificate authority, a collector that speaks only
//! gRPC and a proxy that is not there all read the same. This crate owns the
//! HTTP client, so it keeps the cause chain ([`Cause`]) and the report
//! prints it.
//!
//! [`hint`] then names what to change, but only where a fact in the failure
//! says so: the TLS error that was returned, a `401` or `404` the collector
//! answered with, or a port that is another transport's conventional one. A
//! guess is worse than silence, so nothing here fires on a bare "connection
//! refused" (the cause already says it), and a failure that came with an
//! HTTP status is never taken for a port mismatch (a server that answered
//! speaks HTTP).

use std::error::Error;
use std::sync::{Mutex, PoisonError};

use super::config::OtlpProtocol;

/// The most recent transport failure of one signal's HTTP client, as text.
///
/// Written by [`ExportHttpClient`](super::http::ExportHttpClient) when a
/// request fails, read by the loss report of the same signal. One slot per
/// signal, so a metrics failure is never reported under traces.
#[derive(Debug, Default)]
pub(super) struct Cause(Mutex<Option<String>>);

impl Cause {
    pub(super) fn record(&self, error: &(dyn Error + 'static)) {
        self.record_text(chain(error));
    }

    pub(super) fn record_text(&self, text: String) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(text);
    }

    /// The cause recorded since the last call, if any.
    pub(super) fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// `error` and each of its sources, joined by `: `, leaving out a link whose
/// words the one before it already contains (hyper's `client error (Connect)`
/// is followed by its own source, and a wrapper often repeats it).
pub(super) fn chain(error: &(dyn Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        let part = next.to_string();
        if !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        source = next.source();
    }
    text
}

/// What the report needs to know about where an export goes.
#[derive(Clone, Debug)]
pub(super) struct Route {
    pub(super) protocol: OtlpProtocol,
    pub(super) https: bool,
    /// The destination's port, when its URL names one.
    pub(super) port: Option<u16>,
}

impl Route {
    /// The route `destination` (a URL, as shown to a person) describes.
    pub(super) fn of(protocol: OtlpProtocol, destination: &str) -> Self {
        let uri = destination.parse::<http::Uri>().ok();
        Self {
            protocol,
            https: uri.as_ref().and_then(http::Uri::scheme_str) == Some("https"),
            port: uri.as_ref().and_then(http::Uri::port_u16),
        }
    }
}

/// OTLP/gRPC's and OTLP/HTTP's conventional ports (the OpenTelemetry
/// specification's defaults).
const GRPC_PORT: u16 = 4317;
const HTTP_PORT: u16 = 4318;

/// What to change, when `error` (the exporter's message) and `cause` (the
/// transport's, if it was kept) point at one thing. `None` when they point at
/// nothing in particular.
pub(super) fn hint(route: &Route, error: &str, cause: Option<&str>) -> Option<String> {
    let evidence =
        |needle: &str| error.contains(needle) || cause.is_some_and(|c| c.contains(needle));
    if route.https {
        if evidence("UnknownIssuer") {
            return Some(
                "the collector's certificate was not issued by a CA this exporter trusts \
                 (it trusts Mozilla's roots, not the operating system's): for a private CA, \
                 set OTEL_EXPORTER_OTLP_CERTIFICATE=/path/to/ca.pem"
                    .to_owned(),
            );
        }
        if evidence("NotValidForName") {
            return Some(
                "the collector's certificate is not valid for the host in the endpoint: \
                 use a host name the certificate lists"
                    .to_owned(),
            );
        }
        if evidence("Expired") {
            return Some("the collector's certificate has expired".to_owned());
        }
    }
    if evidence("status code: 401") || evidence("status code: 403") {
        return Some(
            "the collector refused the credentials: check OTEL_EXPORTER_OTLP_HEADERS \
             (or the signal's own OTEL_EXPORTER_OTLP_*_HEADERS)"
                .to_owned(),
        );
    }
    if evidence("status code: 404") && route.protocol.is_http() {
        return Some(
            "the collector has no OTLP/HTTP receiver at that path: the exporter posts to \
             /v1/traces, /v1/metrics and /v1/logs under the endpoint"
                .to_owned(),
        );
    }
    // A reply with a status means the other end speaks HTTP, so a port that
    // looks like the other transport's is no longer a suspect.
    if !evidence("status code") {
        match (route.protocol.is_http(), route.port) {
            (true, Some(GRPC_PORT)) => {
                return Some(format!(
                    "port {GRPC_PORT} is OTLP/gRPC's usual port and this export speaks \
                     OTLP/HTTP (usually {HTTP_PORT}): if the collector's gRPC receiver is \
                     meant, set OTEL_EXPORTER_OTLP_PROTOCOL=grpc"
                ));
            }
            (false, Some(HTTP_PORT)) => {
                return Some(format!(
                    "port {HTTP_PORT} is OTLP/HTTP's usual port and this export speaks gRPC \
                     (usually {GRPC_PORT}): if the collector's HTTP receiver is meant, set \
                     OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf"
                ));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    fn route(protocol: OtlpProtocol, url: &str) -> Route {
        Route::of(protocol, url)
    }

    #[derive(Debug)]
    struct Layered(&'static str, Option<Box<Self>>);

    impl fmt::Display for Layered {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl Error for Layered {
        fn source(&self) -> Option<&(dyn Error + 'static)> {
            self.1.as_deref().map(|e| e as &(dyn Error + 'static))
        }
    }

    #[test]
    fn the_cause_chain_lists_each_source_once() {
        let error = Layered(
            "client error (Connect)",
            Some(Box::new(Layered(
                "invalid peer certificate: UnknownIssuer",
                Some(Box::new(Layered("UnknownIssuer", None))),
            ))),
        );
        assert_eq!(
            chain(&error),
            "client error (Connect): invalid peer certificate: UnknownIssuer"
        );
    }

    #[test]
    fn a_slot_hands_over_what_was_recorded_once() {
        let slot = Cause::default();
        assert_eq!(slot.take(), None);
        // An error is recorded as its whole chain, and read before anything
        // else is written.
        slot.record(&Layered("first", Some(Box::new(Layered("because", None)))));
        assert_eq!(slot.take().as_deref(), Some("first: because"));
        assert_eq!(slot.take(), None);
        // The latest write wins, whichever way it was made.
        slot.record(&Layered("first", None));
        slot.record_text("second".to_owned());
        assert_eq!(slot.take().as_deref(), Some("second"));
        assert_eq!(slot.take(), None);
    }

    #[test]
    fn a_route_reads_scheme_and_port_from_the_destination() {
        let r = route(
            OtlpProtocol::HttpProtobuf,
            "https://c.example:4317/v1/traces",
        );
        assert!(r.https);
        assert_eq!(r.port, Some(4317));
        let r = route(OtlpProtocol::Grpc, "http://c.example");
        assert!(!r.https);
        assert_eq!(r.port, None);
    }

    #[test]
    fn an_unknown_ca_names_the_variable_that_trusts_one() {
        let h = hint(
            &route(OtlpProtocol::HttpProtobuf, "https://c:4318/v1/traces"),
            "Operation failed: HTTP export failed: network error",
            Some("client error (Connect): invalid peer certificate: UnknownIssuer"),
        )
        .expect("a hint");
        assert!(h.contains("OTEL_EXPORTER_OTLP_CERTIFICATE"), "{h}");
        assert!(h.contains("not the operating system's"), "{h}");
    }

    #[test]
    fn a_certificate_for_another_name_or_past_its_date_is_said_as_that() {
        let https = route(OtlpProtocol::Grpc, "https://c:4317");
        let name = hint(
            &https,
            "x",
            Some("invalid peer certificate: NotValidForName"),
        )
        .unwrap();
        assert!(name.contains("not valid for the host"), "{name}");
        let old = hint(&https, "x", Some("invalid peer certificate: Expired")).unwrap();
        assert!(old.contains("expired"), "{old}");
    }

    #[test]
    fn a_certificate_hint_needs_an_https_destination() {
        // The same words on a plaintext destination are not about a certificate.
        let plain = route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces");
        assert_eq!(hint(&plain, "x", Some("UnknownIssuer")), None);
    }

    #[test]
    fn a_refused_credential_points_at_the_headers() {
        let r = route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces");
        for status in ["401", "403"] {
            let h = hint(
                &r,
                &format!("HTTP export failed with status code: {status}"),
                None,
            )
            .expect("a hint");
            assert!(h.contains("OTEL_EXPORTER_OTLP_HEADERS"), "{h}");
        }
    }

    #[test]
    fn a_404_names_the_paths_the_exporter_posts_to() {
        let r = route(
            OtlpProtocol::HttpProtobuf,
            "http://c:4318/v1/traces/v1/traces",
        );
        let h = hint(&r, "HTTP export failed with status code: 404", None).expect("a hint");
        assert!(h.contains("/v1/traces, /v1/metrics and /v1/logs"), "{h}");
        // gRPC has no path, so a 404 is not this advice.
        let g = route(OtlpProtocol::Grpc, "http://c:4317");
        assert_eq!(hint(&g, "status code: 404", None), None);
    }

    #[test]
    fn http_to_4317_and_grpc_to_4318_are_named_as_the_other_transports_port() {
        let http = hint(
            &route(OtlpProtocol::HttpProtobuf, "http://c:4317/v1/traces"),
            "HTTP export failed: network error",
            None,
        )
        .expect("a hint");
        assert!(http.contains("OTEL_EXPORTER_OTLP_PROTOCOL=grpc"), "{http}");
        let grpc = hint(
            &route(OtlpProtocol::Grpc, "http://c:4318"),
            "transport error: broken pipe",
            None,
        )
        .expect("a hint");
        assert!(
            grpc.contains("OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf"),
            "{grpc}"
        );
    }

    #[test]
    fn a_reply_with_a_status_is_never_a_port_mismatch() {
        let r = route(OtlpProtocol::HttpProtobuf, "http://c:4317/v1/traces");
        assert_eq!(
            hint(&r, "HTTP export failed with status code: 503", None),
            None
        );
    }

    #[test]
    fn the_matching_port_and_protocol_say_nothing() {
        assert_eq!(
            hint(
                &route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces"),
                "HTTP export failed: network error",
                Some(
                    "client error (Connect): tcp connect error: Connection refused (os error 111)"
                ),
            ),
            None
        );
        assert_eq!(
            hint(
                &route(OtlpProtocol::Grpc, "http://c:4317"),
                "transport error",
                None
            ),
            None
        );
    }
}
