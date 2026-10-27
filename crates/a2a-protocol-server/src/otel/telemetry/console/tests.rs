// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The console exporter's lines, produced by real providers recording into a
//! buffer. The path through `OTEL_*_EXPORTER=console` and standard output is
//! tested end to end in `tests/telemetry_export/console.rs`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use opentelemetry::KeyValue;
use opentelemetry::logs::{LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::trace::{Span as _, SpanKind, Status, Tracer as _, TracerProvider as _};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;

use super::*;

fn buffer() -> (Console, Arc<Mutex<Vec<u8>>>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    (Console::with_sink(Sink::Buffer(Arc::clone(&buf))), buf)
}

fn lines(buf: &Arc<Mutex<Vec<u8>>>) -> Vec<String> {
    String::from_utf8(buf.lock().expect("lock").clone())
        .expect("utf-8")
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Expected values come from Python's `datetime`, not from this module.
#[test]
fn timestamps_are_rfc3339_utc_to_the_millisecond() {
    for (secs, millis, expected) in [
        (0, 0, "1970-01-01T00:00:00.000Z"),
        (1_700_000_000, 500, "2023-11-14T22:13:20.500Z"),
        (951_782_400, 0, "2000-02-29T00:00:00.000Z"),
        (1_709_251_199, 999, "2024-02-29T23:59:59.999Z"),
        (4_102_444_800, 0, "2100-01-01T00:00:00.000Z"),
        (1_798_761_599, 1, "2026-12-31T23:59:59.001Z"),
        // From 2100-03-01 the day-of-era passes 36,524, the term the
        // century correction adds; every date before it is blind to that term.
        (4_107_542_399, 0, "2100-02-28T23:59:59.000Z"),
        (4_107_542_400, 0, "2100-03-01T00:00:00.000Z"),
        (7_289_654_399, 0, "2200-12-31T23:59:59.000Z"),
        (13_574_606_400, 0, "2400-02-29T12:00:00.000Z"),
        (13_574_649_600, 0, "2400-03-01T00:00:00.000Z"),
        (253_402_300_799, 0, "9999-12-31T23:59:59.000Z"),
    ] {
        let t = UNIX_EPOCH + Duration::new(secs, millis * 1_000_000);
        assert_eq!(rfc3339(t), expected, "{secs}.{millis}");
    }
    assert_eq!(
        rfc3339(UNIX_EPOCH - Duration::from_secs(5)),
        "1970-01-01T00:00:00.000Z"
    );
}

#[test]
fn durations_use_the_unit_that_keeps_the_number_short() {
    assert_eq!(human(Duration::from_nanos(900)), "900ns");
    assert_eq!(human(Duration::from_micros(85)), "85µs");
    assert_eq!(human(Duration::from_micros(4_200)), "4.2ms");
    assert_eq!(human(Duration::from_millis(1_250)), "1.25s");
}

/// The console exporter keeps running totals, and the SDK's own default is
/// the same, which is what lets `Console::temporality` carry
/// `#[mutants::skip]`: `Default::default()` could not be told from it.
#[test]
fn the_console_exporter_reports_cumulative_temporality() {
    use opentelemetry_sdk::metrics::exporter::PushMetricExporter as _;
    assert_eq!(Console::stdout().temporality(), Temporality::Cumulative);
    assert_eq!(Temporality::default(), Temporality::Cumulative);
}

/// The exporter only ever asks the SDK for running totals, so `temporality`
/// is handed `Cumulative` by every metric line it prints; `Delta` is named
/// here because no exporter-driven test can reach that arm.
#[test]
fn a_temporality_is_named_in_words() {
    assert_eq!(temporality(Temporality::Delta), "delta");
    assert_eq!(temporality(Temporality::Cumulative), "cumulative");
}

/// A duration exactly on a boundary is shown in the larger unit.
#[test]
fn a_duration_on_a_unit_boundary_is_shown_in_the_larger_unit() {
    assert_eq!(human(Duration::from_nanos(999)), "999ns");
    assert_eq!(human(Duration::from_micros(1)), "1µs");
    assert_eq!(human(Duration::from_millis(1)), "1.0ms");
    assert_eq!(human(Duration::from_secs(1)), "1.00s");
}

#[test]
fn a_span_is_one_line_with_ids_status_and_attributes() {
    let (console, buf) = buffer();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(console)
        .build();
    let tracer = provider.tracer("t");
    let mut span = tracer
        .span_builder("work")
        .with_kind(SpanKind::Server)
        .with_attributes(vec![
            KeyValue::new("n", 42_i64),
            KeyValue::new("msg", "hello world"),
        ])
        .start(&tracer);
    span.set_status(Status::error("boom bang"));
    span.end();
    let out = lines(&buf);
    assert_eq!(out.len(), 1, "{out:?}");
    let line = &out[0];
    assert!(line.contains(" SPAN work server "), "{line}");
    assert!(line.contains(" status=error:\"boom bang\""), "{line}");
    assert!(line.contains(" n=42 msg=\"hello world\""), "{line}");
    let trace = line
        .split_once(" trace=")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .expect("a trace id");
    assert_eq!(trace.len(), 32, "{line}");
    assert!(
        !line.contains(" parent="),
        "a root span has no parent: {line}"
    );
    assert!(!line.contains(" events="), "no events, no count: {line}");
}

/// An error status with no description is `status=error` and nothing after
/// it; the text of an empty description would otherwise print as `error:""`.
#[test]
fn an_error_status_with_no_description_has_no_colon_after_it() {
    let (console, buf) = buffer();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(console)
        .build();
    let tracer = provider.tracer("t");
    let mut span = tracer.start("failed");
    span.set_status(Status::error(""));
    span.end();
    let line = &lines(&buf)[0];
    assert!(line.contains(" status=error"), "{line}");
    assert!(!line.contains("status=error:"), "{line}");
}

#[test]
fn a_span_counts_its_events_without_listing_them() {
    let (console, buf) = buffer();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(console)
        .build();
    let tracer = provider.tracer("t");
    let mut span = tracer.start("busy");
    span.add_event("first thing that happened", vec![]);
    span.add_event("second thing", vec![]);
    span.end();
    let line = &lines(&buf)[0];
    assert!(line.contains(" events=2"), "{line}");
    assert!(!line.contains("first thing"), "{line}");
}

#[test]
fn a_child_span_names_its_parent_and_an_ok_span_says_so() {
    use opentelemetry::trace::TraceContextExt as _;
    let (console, buf) = buffer();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(console)
        .build();
    let tracer = provider.tracer("t");
    let outer_id = tracer.in_span("outer", |cx| {
        let mut child = tracer.start_with_context("inner", &cx);
        child.set_status(Status::Ok);
        child.end();
        cx.span().span_context().span_id().to_string()
    });
    let out = lines(&buf);
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].contains(" SPAN inner internal "), "{}", out[0]);
    assert!(
        out[0].contains(&format!(" parent={outer_id}")),
        "{}",
        out[0]
    );
    assert!(out[0].contains(" status=ok"), "{}", out[0]);
    assert!(out[1].contains(" SPAN outer internal "), "{}", out[1]);
}

#[test]
fn each_metric_data_point_is_a_line_with_its_series_and_kind() {
    let (console, buf) = buffer();
    let provider = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(console).build())
        .build();
    let meter = provider.meter("t");
    meter
        .u64_counter("probe.count")
        .with_unit("{request}")
        .build()
        .add(3, &[KeyValue::new("route", "/x")]);
    let histogram = meter.f64_histogram("probe.duration").with_unit("s").build();
    histogram.record(0.5, &[]);
    histogram.record(1.5, &[]);
    meter.u64_gauge("probe.depth").build().record(7, &[]);
    provider.force_flush().expect("flush");
    let out = lines(&buf);
    let has = |needle: &str| out.iter().any(|l| l.contains(needle));
    assert!(
        has(" METRIC probe.count{route=/x} sum 3 {request} cumulative"),
        "{out:?}"
    );
    assert!(
        has(" METRIC probe.duration histogram count=2 sum=2 s cumulative min=0.5 max=1.5"),
        "{out:?}"
    );
    assert!(has(" METRIC probe.depth gauge 7"), "{out:?}");
}

#[test]
fn a_log_record_is_a_line_with_severity_body_and_attributes() {
    let (console, buf) = buffer();
    let provider = SdkLoggerProvider::builder()
        .with_simple_exporter(console)
        .build();
    let logger = provider.logger("t");
    let mut record = logger.create_log_record();
    record.set_severity_number(Severity::Warn);
    record.set_severity_text("WARN");
    record.set_body("disk nearly full".into());
    record.add_attribute("mount", "/data");
    logger.emit(record);
    let out = lines(&buf);
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].contains(" LOG WARN disk nearly full"), "{}", out[0]);
    assert!(out[0].ends_with(" mount=/data"), "{}", out[0]);
}

#[test]
fn the_bridges_bookkeeping_is_left_out_and_real_attributes_are_not() {
    let (console, buf) = buffer();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(console)
        .build();
    let tracer = provider.tracer("t");
    tracer
        .span_builder("s")
        .with_attributes(vec![
            KeyValue::new("code.file.path", "x.rs"),
            KeyValue::new("code.line.number", 3_i64),
            KeyValue::new("thread.id", 1_i64),
            KeyValue::new("thread.name", "main"),
            KeyValue::new("busy_ns", 10_i64),
            KeyValue::new("idle_ns", 20_i64),
            KeyValue::new("target", "my_agent"),
            KeyValue::new("code", "kept"),
            KeyValue::new("http.request.method", "GET"),
        ])
        .start(&tracer)
        .end();
    let line = &lines(&buf)[0];
    for gone in ["code.file", "code.line", "thread.", "busy_ns", "idle_ns"] {
        assert!(!line.contains(gone), "{gone} in {line}");
    }
    assert!(
        line.ends_with(" target=my_agent code=kept http.request.method=GET"),
        "{line}"
    );
}

#[test]
fn words_are_bare_and_everything_else_is_quoted() {
    assert_eq!(text("plain"), "plain");
    assert_eq!(text(""), "\"\"");
    assert_eq!(text("a b"), "\"a b\"");
    assert_eq!(text("a=b"), "\"a=b\"");
    assert_eq!(text("say \"hi\""), "\"say \\\"hi\\\"\"");
}
