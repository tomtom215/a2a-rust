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
#[derive(Clone, Copy, Debug)]
pub(super) struct Route {
    pub(super) protocol: OtlpProtocol,
    pub(super) https: bool,
    /// The destination's port, when its URL names one.
    pub(super) port: Option<u16>,
    /// Whether the export goes through an HTTP proxy.
    pub(super) proxied: bool,
}

impl Route {
    /// The route `destination` (a URL, as shown to a person) describes.
    pub(super) fn of(protocol: OtlpProtocol, destination: &str) -> Self {
        let uri = destination.parse::<http::Uri>().ok();
        Self {
            protocol,
            https: uri.as_ref().and_then(http::Uri::scheme_str) == Some("https"),
            port: uri.as_ref().and_then(http::Uri::port_u16),
            proxied: false,
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
pub(super) fn hint(route: Route, error: &str, cause: Option<&str>) -> Option<String> {
    let evidence =
        |needle: &str| error.contains(needle) || cause.is_some_and(|c| c.contains(needle));
    let first = |rules: &[Rule]| {
        rules
            .iter()
            .find(|r| (r.applies)(route) && r.evidence.iter().any(|e| evidence(e)))
            .map(|r| r.hint.to_owned())
    };
    first(BEFORE_PORTS)
        .or_else(|| {
            (!evidence("status code"))
                .then(|| port_hint(route))
                .flatten()
        })
        .or_else(|| first(AFTER_PORTS))
}

/// One hint: given when `applies` holds of the route and the failure
/// contains any of `evidence`.
struct Rule {
    applies: fn(Route) -> bool,
    evidence: &'static [&'static str],
    hint: &'static str,
}

/// The rules checked before the ports, in order. The words are those
/// `hyper-util` and rustls 0.23 print, as a live collector and proxy drew
/// them out (`deploy/observability/README.md`, TLS section): the error
/// variants' names (`NotValidForName`, `Expired`) never appear.
const BEFORE_PORTS: &[Rule] = &[
    // A CONNECT the proxy answered with other than 200 (407 has its own).
    Rule {
        applies: |r| r.proxied,
        evidence: &[
            "tunnel error: proxy authorization required",
            "status code: 407",
        ],
        hint: "the proxy requires credentials, or refused the ones given: put them in the \
               proxy URL (http://user:password@proxy:3128)",
    },
    Rule {
        applies: |r| r.proxied,
        evidence: &["tunnel error: unsuccessful"],
        hint: "the proxy refused to open a tunnel (CONNECT) to the collector: allow the \
               collector's host and port on the proxy, or name the collector in NO_PROXY to \
               reach it directly",
    },
    Rule {
        applies: |r| r.https,
        evidence: &["UnknownIssuer"],
        hint: "the collector's certificate was not issued by a CA this exporter trusts (it \
               trusts Mozilla's roots, not the operating system's): for a private CA, set \
               OTEL_EXPORTER_OTLP_CERTIFICATE=/path/to/ca.pem",
    },
    Rule {
        applies: |r| r.https,
        evidence: &["certificate not valid for name"],
        hint: "the collector's certificate is not valid for the host in the endpoint: use a \
               host name the certificate lists",
    },
    Rule {
        applies: |r| r.https,
        evidence: &["certificate expired"],
        hint: "the collector's certificate has expired",
    },
    Rule {
        applies: |r| r.https,
        evidence: &["CaUsedAsEndEntity"],
        hint: "the collector presents a CA certificate as its own, which this exporter \
               refuses; `openssl req -x509` makes one unless told otherwise. Make the \
               collector's self-signed certificate with \
               `-addext basicConstraints=critical,CA:FALSE` and its host in subjectAltName, \
               or issue it from a CA and set OTEL_EXPORTER_OTLP_CERTIFICATE to that CA",
    },
    // Alerts the collector sent, about this exporter's own certificate.
    Rule {
        applies: |r| r.https,
        evidence: &["received fatal alert: CertificateRequired"],
        hint: "the collector requires a client certificate (mutual TLS): set \
               OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE and OTEL_EXPORTER_OTLP_CLIENT_KEY",
    },
    Rule {
        applies: |r| r.https,
        evidence: &["received fatal alert: UnknownCA"],
        hint: "the collector does not trust this exporter's client certificate: it was not \
               issued by a CA in the collector's client_ca_file",
    },
    // A plaintext reply to a TLS client hello.
    Rule {
        applies: |r| r.https,
        evidence: &["received corrupt message of type InvalidContentType"],
        hint: "the endpoint is https:// but the collector answered without TLS: if its \
               receiver has no TLS configured, write the endpoint with http://",
    },
    // Plaintext through a proxy: the proxy answers for itself, in the same
    // words a collector would.
    Rule {
        applies: |r| r.proxied && !r.https,
        evidence: &["status code: 401", "status code: 403"],
        hint: "the proxy or the collector refused the request: if the proxy did, allow the \
               collector on it or name the collector in NO_PROXY; if the collector did, check \
               OTEL_EXPORTER_OTLP_HEADERS (or the signal's own OTEL_EXPORTER_OTLP_*_HEADERS)",
    },
    Rule {
        applies: |_| true,
        evidence: &["status code: 401", "status code: 403"],
        hint: "the collector refused the credentials: check OTEL_EXPORTER_OTLP_HEADERS (or \
               the signal's own OTEL_EXPORTER_OTLP_*_HEADERS)",
    },
    Rule {
        applies: |r| r.protocol.is_http(),
        evidence: &["status code: 404"],
        hint: "the collector has no OTLP/HTTP receiver at that path: the exporter posts to \
               /v1/traces, /v1/metrics and /v1/logs under the endpoint",
    },
];

/// The rules checked after the ports: plaintext to a receiver that serves
/// TLS. Go's TLS server answers an HTTP/1 request with 400, and closes a gRPC
/// connection unanswered.
const AFTER_PORTS: &[Rule] = &[
    Rule {
        applies: |r| !r.https && r.protocol.is_http(),
        evidence: &["status code: 400"],
        hint: "a collector whose receiver serves TLS answers a plaintext request with 400: if \
               it does, write the endpoint with https://",
    },
    Rule {
        applies: |r| !r.https && !r.protocol.is_http(),
        evidence: &["broken pipe"],
        hint: "the collector closed the connection unanswered: if its gRPC receiver serves \
               TLS, write the endpoint with https://; if this is its OTLP/HTTP port, set \
               OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf",
    },
];

/// A port that is the other transport's conventional one. Only asked when
/// the failure carries no HTTP status: a reply with a status means the other
/// end speaks HTTP, so the port is no longer a suspect.
fn port_hint(route: Route) -> Option<String> {
    match (route.protocol.is_http(), route.port) {
        (true, Some(GRPC_PORT)) => Some(format!(
            "port {GRPC_PORT} is OTLP/gRPC's usual port and this export speaks OTLP/HTTP \
             (usually {HTTP_PORT}): if the collector's gRPC receiver is meant, set \
             OTEL_EXPORTER_OTLP_PROTOCOL=grpc"
        )),
        (false, Some(HTTP_PORT)) => Some(format!(
            "port {HTTP_PORT} is OTLP/HTTP's usual port and this export speaks gRPC (usually \
             {GRPC_PORT}): if the collector's HTTP receiver is meant, set \
             OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
