// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! gzip on export bodies: asked for in code, it is on the wire on every
//! transport, and asked for nowhere it is not. The collector inflates what it
//! is sent and panics on a body that is not gzip, so a header with nothing
//! behind it fails the export, not only an assertion.

use a2a_protocol_server::otel::{OtlpCompression, OtlpProtocol};

use super::collector::Collector;
use super::{assert_all_three_arrived, builder, record_one_of_each};

/// The value of `header` on every request the collector received.
fn header_on_each(collector: &Collector, header: &str) -> Vec<(String, Option<String>)> {
    collector
        .received()
        .into_iter()
        .map(|r| (r.path, r.headers.get(header).cloned()))
        .collect()
}

fn export_gzipped(collector: &Collector, protocol: OtlpProtocol) {
    let telemetry = builder(&collector.http_base(), protocol)
        .with_compression(OtlpCompression::Gzip)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(collector);
}

fn assert_every_request_says(collector: &Collector, header: &str, value: &str) {
    let seen = header_on_each(collector, header);
    assert!(!seen.is_empty(), "nothing was received");
    for (path, got) in seen {
        assert_eq!(got.as_deref(), Some(value), "{header} on {path}");
    }
}

#[test]
fn http_protobuf_bodies_are_gzipped_when_asked() {
    let collector = Collector::http();
    export_gzipped(&collector, OtlpProtocol::HttpProtobuf);
    assert_every_request_says(&collector, "content-encoding", "gzip");
}

#[cfg(feature = "otlp-json")]
#[test]
fn http_json_bodies_are_gzipped_when_asked() {
    let collector = Collector::http();
    export_gzipped(&collector, OtlpProtocol::HttpJson);
    assert_every_request_says(&collector, "content-encoding", "gzip");
}

#[test]
fn grpc_messages_are_gzipped_when_asked() {
    let collector = Collector::grpc();
    export_gzipped(&collector, OtlpProtocol::Grpc);
    assert_every_request_says(&collector, "grpc-encoding", "gzip");
}

/// The control: nothing asked, nothing compressed, so the tests above are
/// not passing because every export is gzipped whatever it was told.
#[test]
fn nothing_is_compressed_unless_asked() {
    let http = Collector::http();
    let telemetry = builder(&http.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&http);
    for (path, got) in header_on_each(&http, "content-encoding") {
        assert_eq!(got, None, "content-encoding on {path}");
    }

    let grpc = Collector::grpc();
    let telemetry = builder(&grpc.http_base(), OtlpProtocol::Grpc)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    assert_all_three_arrived(&grpc);
    for (path, got) in header_on_each(&grpc, "grpc-encoding") {
        assert!(
            got.is_none() || got.as_deref() == Some("identity"),
            "grpc-encoding on {path}: {got:?}"
        );
    }
}
