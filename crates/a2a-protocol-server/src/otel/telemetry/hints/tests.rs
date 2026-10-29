// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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
        route(OtlpProtocol::HttpProtobuf, "https://c:4318/v1/traces"),
        "Operation failed: HTTP export failed: network error",
        Some("client error (Connect): invalid peer certificate: UnknownIssuer"),
    )
    .expect("a hint");
    assert!(h.contains("OTEL_EXPORTER_OTLP_CERTIFICATE"), "{h}");
    assert!(h.contains("not the operating system's"), "{h}");
}

/// A failure seen live and the words its hint must contain.
struct Live {
    protocol: OtlpProtocol,
    url: &'static str,
    error: &'static str,
    cause: Option<&'static str>,
    expect: &'static str,
}

/// Each failure below is the exporter's message as written against a live
/// collector (otelcol-contrib 0.163.2) with that misconfiguration, copied
/// verbatim. The hints for a wrong name and an expired certificate first
/// matched the error variants' names, which the messages never contain, and
/// so never fired.
const SEEN_LIVE: &[Live] = &[
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "https://c:4318/v1/traces",
        error: "Operation failed: HTTP export failed: network error",
        cause: Some(
            "client error (Connect): invalid peer certificate: certificate not valid \
             for name \"otel-wrongname\"; certificate is only valid for \
             DnsName(\"localhost\"), IpAddress(127.0.0.1) or IpAddress(0::1)",
        ),
        expect: "not valid for the host",
    },
    Live {
        protocol: OtlpProtocol::Grpc,
        url: "https://c:4317",
        error: "Operation failed: TonicTracesClient export failed with gRPC code: Unavailable: \
             transport error: invalid peer certificate: certificate not valid for name \
             \"otel-wrongname\"; certificate is only valid for DnsName(\"localhost\")",
        cause: None,
        expect: "not valid for the host",
    },
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "https://c:4318/v1/traces",
        error: "Operation failed: HTTP export failed: network error",
        cause: Some(
            "client error (Connect): invalid peer certificate: certificate expired: \
             verification time 1793186556 (UNIX), but certificate is not valid after \
             1577923200 (215263356 seconds ago)",
        ),
        expect: "has expired",
    },
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "https://c:4318/v1/traces",
        error: "Operation failed: HTTP export failed: network error",
        cause: Some(
            "client error (Connect): invalid peer certificate: \
             Other(OtherError(CaUsedAsEndEntity))",
        ),
        expect: "CA:FALSE",
    },
    Live {
        protocol: OtlpProtocol::Grpc,
        url: "https://c:4317",
        error: "Operation failed: TonicTracesClient export failed with gRPC code: Unavailable: \
             transport error: invalid peer certificate: Other(OtherError(CaUsedAsEndEntity))",
        cause: None,
        expect: "CA:FALSE",
    },
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "https://c:4318/v1/traces",
        error: "Operation failed: HTTP export failed: network error",
        cause: Some(
            "client error (SendRequest): connection error: received fatal alert: \
             CertificateRequired",
        ),
        expect: "OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE",
    },
    Live {
        protocol: OtlpProtocol::Grpc,
        url: "https://c:4317",
        error: "Operation failed: TonicTracesClient export failed with gRPC code: Unknown: \
             transport error: connection error: received fatal alert: CertificateRequired",
        cause: None,
        expect: "OTEL_EXPORTER_OTLP_CLIENT_CERTIFICATE",
    },
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "https://c:4318/v1/traces",
        error: "Operation failed: HTTP export failed: network error",
        cause: Some(
            "client error (SendRequest): connection error: received fatal alert: UnknownCA",
        ),
        expect: "client_ca_file",
    },
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "https://c:4318/v1/traces",
        error: "Operation failed: HTTP export failed: network error",
        cause: Some("client error (Connect): received corrupt message of type InvalidContentType"),
        expect: "write the endpoint with http://",
    },
    Live {
        protocol: OtlpProtocol::Grpc,
        url: "https://c:4317",
        error: "Operation failed: TonicTracesClient export failed with gRPC code: Unavailable: \
             transport error: received corrupt message of type InvalidContentType",
        cause: None,
        expect: "write the endpoint with http://",
    },
    Live {
        protocol: OtlpProtocol::HttpProtobuf,
        url: "http://c:4418/v1/traces",
        error: "Operation failed: HTTP export failed with status code: 400",
        cause: None,
        expect: "https://",
    },
    Live {
        protocol: OtlpProtocol::Grpc,
        url: "http://c:4417",
        error: "Operation failed: TonicTracesClient export failed with gRPC code: Unknown: \
             transport error: connection error: stream closed because of a broken pipe",
        cause: None,
        expect: "https://",
    },
];

#[test]
fn each_tls_failure_seen_live_gets_its_hint() {
    for live in SEEN_LIVE {
        let h = hint(route(live.protocol, live.url), live.error, live.cause)
            .unwrap_or_else(|| panic!("no hint for {:?} {:?}", live.error, live.cause));
        assert!(h.contains(live.expect), "{:?} not in {h}", live.expect);
    }
}

#[test]
fn a_certificate_hint_needs_an_https_destination() {
    // The same words on a plaintext destination are not about a certificate.
    let plain = route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces");
    assert_eq!(hint(plain, "x", Some("UnknownIssuer")), None);
}

/// A proxy's refusals, as a live Squid gave them (deny-all and
/// basic-auth configurations), copied verbatim.
#[test]
fn a_proxy_refusal_is_named_as_the_proxys() {
    let via = |protocol, url| Route {
        proxied: true,
        ..route(protocol, url)
    };
    let tunnel = hint(
        via(OtlpProtocol::HttpProtobuf, "https://c:4418/v1/traces"),
        "Operation failed: HTTP export failed: network error",
        Some("client error (Connect): tunnel error: unsuccessful"),
    )
    .expect("a hint");
    assert!(tunnel.contains("NO_PROXY"), "{tunnel}");
    let grpc = hint(
        via(OtlpProtocol::Grpc, "http://c:4317"),
        "Operation failed: TonicTracesClient export failed with gRPC code: Unavailable: \
         transport error: tunnel error: unsuccessful: tunnel error: unsuccessful",
        None,
    )
    .expect("a hint");
    assert!(grpc.contains("CONNECT"), "{grpc}");
    let plain = hint(
        via(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces"),
        "Operation failed: HTTP export failed with status code: 403",
        None,
    )
    .expect("a hint");
    assert!(plain.contains("the proxy or the collector"), "{plain}");
    // Without a proxy the same 403 is the collector's.
    let direct = hint(
        route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces"),
        "Operation failed: HTTP export failed with status code: 403",
        None,
    )
    .expect("a hint");
    assert!(direct.starts_with("the collector refused"), "{direct}");
    for (r, error, cause) in [
        (
            via(OtlpProtocol::HttpProtobuf, "https://c:4418/v1/traces"),
            "Operation failed: HTTP export failed: network error",
            Some("client error (Connect): tunnel error: proxy authorization required"),
        ),
        (
            via(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces"),
            "Operation failed: HTTP export failed with status code: 407",
            None,
        ),
    ] {
        let h = hint(r, error, cause).expect("a hint");
        assert!(h.contains("http://user:password@"), "{h}");
    }
    // And a tunnel error without a proxy is not one.
    assert_eq!(
        hint(
            route(OtlpProtocol::HttpProtobuf, "https://c:4418/v1/traces"),
            "x",
            Some("tunnel error: unsuccessful")
        ),
        None
    );
}

#[test]
fn a_refused_credential_points_at_the_headers() {
    let r = route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces");
    for status in ["401", "403"] {
        let h = hint(
            r,
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
    let h = hint(r, "HTTP export failed with status code: 404", None).expect("a hint");
    assert!(h.contains("/v1/traces, /v1/metrics and /v1/logs"), "{h}");
    // gRPC has no path, so a 404 is not this advice.
    let g = route(OtlpProtocol::Grpc, "http://c:4317");
    assert_eq!(hint(g, "status code: 404", None), None);
}

#[test]
fn http_to_4317_and_grpc_to_4318_are_named_as_the_other_transports_port() {
    let http = hint(
        route(OtlpProtocol::HttpProtobuf, "http://c:4317/v1/traces"),
        "HTTP export failed: network error",
        None,
    )
    .expect("a hint");
    assert!(
        http.contains("port 4317 is OTLP/gRPC's usual port")
            && http.contains("OTEL_EXPORTER_OTLP_PROTOCOL=grpc"),
        "{http}"
    );
    // Evidence no other rule takes: a broken pipe would also draw the
    // TLS rule's hint, which names the same variable, so the port's own
    // hint went untested.
    let grpc = hint(
        route(OtlpProtocol::Grpc, "http://c:4318"),
        "transport error",
        None,
    )
    .expect("a hint");
    assert!(
        grpc.contains("port 4318 is OTLP/HTTP's usual port")
            && grpc.contains("OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf"),
        "{grpc}"
    );
    // Before the TLS rule, too.
    let piped = hint(
        route(OtlpProtocol::Grpc, "http://c:4318"),
        "transport error: broken pipe",
        None,
    )
    .expect("a hint");
    assert!(
        piped.contains("port 4318 is OTLP/HTTP's usual port"),
        "{piped}"
    );
}

#[test]
fn a_reply_with_a_status_is_never_a_port_mismatch() {
    let r = route(OtlpProtocol::HttpProtobuf, "http://c:4317/v1/traces");
    assert_eq!(
        hint(r, "HTTP export failed with status code: 503", None),
        None
    );
}

#[test]
fn the_matching_port_and_protocol_say_nothing() {
    assert_eq!(
        hint(
            route(OtlpProtocol::HttpProtobuf, "http://c:4318/v1/traces"),
            "HTTP export failed: network error",
            Some("client error (Connect): tcp connect error: Connection refused (os error 111)"),
        ),
        None
    );
    assert_eq!(
        hint(
            route(OtlpProtocol::Grpc, "http://c:4317"),
            "transport error",
            None
        ),
        None
    );
}
