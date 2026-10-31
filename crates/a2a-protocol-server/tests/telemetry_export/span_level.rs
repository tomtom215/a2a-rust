// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Events below the span level are not exported on the span (audit C-01).
//!
//! Until 2026-10-29 the span layer had no level filter: every `DEBUG` and
//! `TRACE` event inside an exported span, from any crate in the process,
//! became one of its events, and `RUST_LOG` could not turn them off.

use a2a_protocol_server::otel::OtlpProtocol;
use tracing_subscriber::layer::SubscriberExt as _;

use super::builder;
use super::collector::Collector;

fn event_names(collector: &Collector) -> Vec<String> {
    let spans = collector.spans();
    let (_, span) = spans
        .iter()
        .find(|(_, s)| s.name == "unit-of-work")
        .expect("the span was exported");
    span.events.iter().map(|e| e.name.clone()).collect()
}

fn record(telemetry: &a2a_protocol_server::otel::Telemetry) {
    let subscriber = tracing_subscriber::registry().with(telemetry.layer());
    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("unit-of-work");
        let _entered = span.enter();
        tracing::info!("an info event");
        tracing::debug!("a debug event");
        tracing::trace!("a trace event");
        // A dependency's debug span: not exported at all.
        let inner = tracing::debug_span!("h2-frame");
        let _inner = inner.enter();
        tracing::info!("inside a debug span");
    });
}

#[test]
fn events_below_the_span_level_are_not_exported() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_logs(false)
        .with_metrics(false)
        .build()
        .expect("build");
    record(&telemetry);
    telemetry.shutdown().expect("flushed");
    let names = event_names(&collector);
    assert!(names.iter().any(|n| n == "an info event"), "{names:?}");
    assert!(!names.iter().any(|n| n == "a debug event"), "{names:?}");
    assert!(!names.iter().any(|n| n == "a trace event"), "{names:?}");
    assert!(
        !collector.spans().iter().any(|(_, s)| s.name == "h2-frame"),
        "a DEBUG span is not exported"
    );
}

#[test]
fn the_span_level_can_be_lowered() {
    let collector = Collector::http();
    let telemetry = builder(&collector.http_base(), OtlpProtocol::HttpProtobuf)
        .with_logs(false)
        .with_metrics(false)
        .with_span_level(tracing::Level::DEBUG)
        .build()
        .expect("build");
    record(&telemetry);
    telemetry.shutdown().expect("flushed");
    let names = event_names(&collector);
    assert!(names.iter().any(|n| n == "a debug event"), "{names:?}");
    assert!(!names.iter().any(|n| n == "a trace event"), "{names:?}");
}
