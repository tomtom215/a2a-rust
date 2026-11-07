// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The JSON console format, and the trace context it carries
//! (OpenTelemetry `specification/compatibility/logging_trace_context.md`:
//! `trace_id`, `span_id`, `trace_flags`, hex).

use std::io::Write as _;
use std::sync::{Arc, Mutex};

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt as _;

use tracing_opentelemetry::OpenTelemetrySpanExt as _;

use super::*;

/// Captures what the layer writes.
#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// The JSON lines `emit` printed, with spans recorded by an OpenTelemetry
/// tracer.
fn lines(emit: impl FnOnce()) -> Vec<Value> {
    let buffer = Buffer::default();
    let provider = SdkTracerProvider::builder().build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("t")))
        .with(
            tracing_subscriber::fmt::layer()
                .event_format(JsonLines)
                .with_writer(buffer.clone()),
        );
    tracing::subscriber::with_default(subscriber, emit);
    let mut sink = buffer.clone();
    sink.flush().unwrap();
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

#[test]
fn an_event_in_a_span_carries_the_trace_context_in_hex() {
    let mut want = None;
    let got = lines(|| {
        let span = tracing::info_span!("work");
        let _entered = span.enter();
        let cx = span.context();
        let sc = cx.span().span_context().clone();
        want = Some((sc.trace_id().to_string(), sc.span_id().to_string()));
        tracing::info!(task = "t-1", attempt = 2_u64, ok = true, "handled");
    });
    let (trace_id, span_id) = want.unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    let line = &got[0];
    assert_eq!(line["message"], "handled");
    assert_eq!(line["level"], "INFO");
    assert_eq!(
        line["target"],
        "a2a_protocol_server::otel::telemetry::console_format::tests"
    );
    assert_eq!(line["task"], "t-1");
    assert_eq!(line["attempt"], 2);
    assert_eq!(line["ok"], true);
    assert_eq!(line["trace_id"], trace_id.as_str());
    assert_eq!(line["span_id"], span_id.as_str());
    assert_eq!(trace_id.len(), 32);
    assert_eq!(span_id.len(), 16);
    // Sampled by the default sampler: flags 01.
    assert_eq!(line["trace_flags"], "01");
    assert!(line["timestamp"].as_str().unwrap().ends_with('Z'), "{line}");
}

#[test]
fn an_event_outside_a_span_has_no_trace_context() {
    let got = lines(|| tracing::warn!("alone"));
    assert_eq!(got.len(), 1);
    let line = got[0].as_object().unwrap();
    assert_eq!(line["message"], "alone");
    assert_eq!(line["level"], "WARN");
    for key in ["trace_id", "span_id", "trace_flags"] {
        assert!(!line.contains_key(key), "{line:?}");
    }
}

/// A field named like one of the line's own keys does not replace it.
#[test]
fn an_event_field_does_not_overwrite_the_lines_own_keys() {
    let got = lines(|| tracing::error!(level = "mine", target = "mine", "x"));
    assert_eq!(got[0]["level"], "ERROR");
    assert_ne!(got[0]["target"], "mine");
}

/// Every numeric field keeps its JSON type, a float among them.
#[test]
fn numeric_fields_are_json_numbers() {
    let got = lines(|| {
        tracing::info!(
            ratio = 0.25_f64,
            count = 3_u64,
            delta = -2_i64,
            ok = true,
            "x"
        );
    });
    assert_eq!(got[0]["ratio"], 0.25);
    assert_eq!(got[0]["count"], 3);
    assert_eq!(got[0]["delta"], -2);
    assert_eq!(got[0]["ok"], true);
}

#[test]
fn a_debug_field_is_its_debug_text() {
    let got = lines(|| tracing::info!(path = ?std::path::Path::new("/a"), "x"));
    assert_eq!(got[0]["path"], "\"/a\"");
}

#[test]
fn the_variable_chooses_json_or_text_and_notes_anything_else() {
    let notes = Notes::default();
    assert_eq!(from_env(None, &notes), ConsoleFormat::Text);
    assert_eq!(from_env(Some(""), &notes), ConsoleFormat::Text);
    assert_eq!(from_env(Some(" JSON "), &notes), ConsoleFormat::Json);
    assert_eq!(from_env(Some("Text"), &notes), ConsoleFormat::Text);
    assert_eq!(notes.take(), Vec::<String>::new());
    assert_eq!(from_env(Some("logfmt"), &notes), ConsoleFormat::Text);
    assert_eq!(
        notes.take(),
        ["A2A_LOG_FORMAT=\"logfmt\" is not `json` or `text`; the text format is used"]
    );
}
