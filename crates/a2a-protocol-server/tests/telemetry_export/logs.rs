// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the exported log records carry beyond their body.

use a2a_protocol_server::otel::OtlpProtocol;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use prost::Message as _;

use super::collector::Collector;
use super::{builder, record_one_of_each};

/// Every log record with a string body: the body, and the trace and span
/// ids it carries (empty when it carries none).
fn log_records(collector: &Collector) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let mut out = Vec::new();
    for r in collector
        .received()
        .into_iter()
        .filter(|r| r.path.ends_with("/v1/logs"))
    {
        let req = ExportLogsServiceRequest::decode(&*r.body).expect("an OTLP logs request");
        for rl in req.resource_logs {
            for sl in rl.scope_logs {
                for rec in sl.log_records {
                    if let Some(Value::StringValue(s)) = rec.body.clone().and_then(|b| b.value) {
                        out.push((s, rec.trace_id, rec.span_id));
                    }
                }
            }
        }
    }
    out
}

/// A log record emitted inside a span carries that span's trace and span
/// ids, so a backend joins the log to its trace.
#[test]
fn a_log_inside_a_span_carries_its_trace_and_span_ids() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .build()
        .expect("build");
    record_one_of_each(&telemetry);
    telemetry.shutdown().expect("every signal flushed");
    let spans = collector.spans();
    let (_, span) = spans
        .iter()
        .find(|(_, s)| s.name == "unit-of-work")
        .expect("the span");
    let records = log_records(&collector);
    let (_, trace_id, span_id) = records
        .iter()
        .find(|(body, _, _)| body == "hello from the test")
        .unwrap_or_else(|| panic!("no log record among {records:?}"));
    assert_eq!(trace_id.len(), 16, "{records:?}");
    assert_eq!(*trace_id, span.trace_id);
    assert_eq!(*span_id, span.span_id);
}
