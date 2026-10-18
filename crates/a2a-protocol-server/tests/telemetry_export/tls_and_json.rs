// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Mutual TLS with the collector, on either transport, and OTLP/HTTP with
//! JSON bodies. Each test that expects export has a counterpart showing the
//! collector refuses what it should, so neither can pass with the check off.

use a2a_protocol_server::otel::OtlpProtocol;

use super::certs::test_certs;
use super::collector::Collector;
use super::{assert_all_three_arrived, builder, record_one_of_each};

/// A collector requiring a client certificate is reached over OTLP/HTTP once
/// the certificate is given …
#[test]
fn https_with_a_client_certificate_exports() {
    let certs = test_certs();
    let collector = Collector::https_requiring_client_cert(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::HttpProtobuf)
        .with_ca_certificate_pem(&certs.ca_pem)
        .with_client_certificate_pem(&certs.client_cert_pem, &certs.client_key_pem)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
}

/// … and refuses the handshake without it, and the loss is reported.
#[test]
fn https_without_a_client_certificate_exports_nothing() {
    let certs = test_certs();
    let collector = Collector::https_requiring_client_cert(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::HttpProtobuf)
        .with_ca_certificate_pem(&certs.ca_pem)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    let err = telemetry.shutdown().expect_err("every export failed");
    assert!(err.to_string().contains("traces: "), "{err}");
    assert!(collector.received().is_empty(), "{:?}", collector.paths());
}

/// The same over OTLP/gRPC …
#[test]
fn grpc_over_tls_with_a_client_certificate_exports() {
    let certs = test_certs();
    let collector = Collector::grpcs_requiring_client_cert(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::Grpc)
        .with_ca_certificate_pem(&certs.ca_pem)
        .with_client_certificate_pem(&certs.client_cert_pem, &certs.client_key_pem)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&collector);
}

/// … where, without the certificate, nothing arrives.
#[test]
fn grpc_over_tls_without_a_client_certificate_exports_nothing() {
    let certs = test_certs();
    let collector = Collector::grpcs_requiring_client_cert(&certs);
    let telemetry = builder(&collector.https_base(), OtlpProtocol::Grpc)
        .with_ca_certificate_pem(&certs.ca_pem)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    let _ = telemetry.shutdown();
    assert!(collector.received().is_empty(), "{:?}", collector.paths());
}

/// A client key that is not the certificate's is refused when the
/// OTLP/HTTP exporters are built, not at every export.
#[test]
fn a_client_key_that_is_not_the_certificates_fails_the_build() {
    let certs = test_certs();
    let other = test_certs();
    let err = builder("https://localhost:1", OtlpProtocol::HttpProtobuf)
        .with_client_certificate_pem(&certs.client_cert_pem, &other.client_key_pem)
        .build()
        .expect_err("mismatched pair");
    assert!(err.to_string().contains("client certificate"), "{err}");
}

/// OTLP/HTTP with JSON bodies: every request says so, and the collector
/// decodes all three signals from them.
#[cfg(feature = "otlp-json")]
#[test]
fn http_json_exports_traces_metrics_and_logs() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpJson)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    let received = collector.received();
    assert!(!received.is_empty());
    for r in &received {
        assert_eq!(
            r.headers.get("content-type").map(String::as_str),
            Some("application/json"),
            "{}",
            r.path
        );
    }
    assert_all_three_arrived(&collector);
}
