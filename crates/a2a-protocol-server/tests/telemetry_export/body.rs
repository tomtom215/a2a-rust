// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! How the stand-in collectors read a request body: inflated if gzip, and
//! OTLP/JSON re-encoded as protobuf so one decoder serves every encoding.

use std::collections::HashMap;

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use prost::Message as _;

/// A gzip body inflated, as a collector does for `Content-Encoding: gzip`.
/// A body that is not gzip panics the collector's connection, and the export
/// fails, so a header that lies is caught.
pub fn gunzip(body: &[u8]) -> Vec<u8> {
    use std::io::Read as _;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(body)
        .read_to_end(&mut out)
        .expect("a gzip body");
    out
}

pub fn says_json(headers: &HashMap<String, String>) -> bool {
    headers
        .get("content-type")
        .is_some_and(|t| t.starts_with("application/json"))
}

/// An OTLP/JSON request body, decoded as a collector does and re-encoded as
/// protobuf, so one decoder serves both encodings. A body that does not
/// decode panics the collector's connection, and the export fails.
pub fn json_to_protobuf(path: &str, body: &[u8]) -> Vec<u8> {
    match path {
        "/v1/traces" => serde_json::from_slice::<ExportTraceServiceRequest>(body)
            .expect("an OTLP/JSON trace request")
            .encode_to_vec(),
        "/v1/metrics" => serde_json::from_slice::<ExportMetricsServiceRequest>(body)
            .expect("an OTLP/JSON metrics request")
            .encode_to_vec(),
        "/v1/logs" => serde_json::from_slice::<ExportLogsServiceRequest>(body)
            .expect("an OTLP/JSON logs request")
            .encode_to_vec(),
        other => panic!("no OTLP signal at {other}"),
    }
}
