// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The `console` exporter (`OTEL_TRACES_EXPORTER=console` and its siblings):
//! one line per span, per metric data point and per log record, on standard
//! output, so a developer can see what an agent records with no collector.
//!
//! Until now `console` was refused, which left a first-time user nothing to
//! look at without running a collector first. The format is one line a
//! record, not the multi-line dump `opentelemetry-stdout` writes (which also
//! pulls `chrono` in for it): a trace is something to read, and a span's
//! name, kind, duration, ids, status and attributes fit a line.

use std::fmt::{Display, Write as _};
use std::io::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opentelemetry::logs::AnyValue;
use opentelemetry::trace::{SpanKind, Status};
use opentelemetry::{KeyValue, Value};
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{LogBatch, LogExporter};
use opentelemetry_sdk::metrics::Temporality;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};

/// Where console lines go: standard output, or a buffer a test reads.
#[derive(Clone, Debug)]
pub(super) enum Sink {
    Stdout,
    #[cfg(test)]
    Buffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>),
}

impl Sink {
    /// One line, written and flushed whole, so lines from two threads do not
    /// interleave and a pipe sees each as it happens.
    fn line(&self, line: &str) {
        match self {
            Self::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{line}");
                let _ = out.flush();
            }
            #[cfg(test)]
            Self::Buffer(buf) => {
                let _ = writeln!(buf.lock().expect("lock"), "{line}");
            }
        }
    }
}

/// The exporter for all three signals.
#[derive(Clone, Debug)]
pub(super) struct Console {
    sink: Sink,
}

impl Console {
    pub(super) const fn stdout() -> Self {
        Self { sink: Sink::Stdout }
    }

    #[cfg(test)]
    pub(super) const fn with_sink(sink: Sink) -> Self {
        Self { sink }
    }
}

impl SpanExporter for Console {
    async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
        for span in &batch {
            self.sink.line(&span_line(span));
        }
        Ok(())
    }
}

impl LogExporter for Console {
    async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
        for (record, _scope) in batch.iter() {
            self.sink.line(&log_line(record));
        }
        Ok(())
    }
}

impl PushMetricExporter for Console {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        for line in metric_lines(metrics) {
            self.sink.line(&line);
        }
        Ok(())
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }

    fn shutdown_with_timeout(&self, _timeout: Duration) -> OTelSdkResult {
        Ok(())
    }

    fn temporality(&self) -> Temporality {
        Temporality::Cumulative
    }
}

/// `2026-10-26T07:28:22.271Z`: UTC, to the millisecond. A time before the
/// epoch prints as the epoch. (Howard Hinnant's `civil_from_days`.)
pub(super) fn rfc3339(t: SystemTime) -> String {
    let since = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// `85µs`, `4.2ms`, `1.25s`: three significant figures, the unit that keeps
/// the number readable.
fn human(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 1e-6 {
        format!("{}ns", d.as_nanos())
    } else if secs < 1e-3 {
        format!("{:.0}µs", secs * 1e6)
    } else if secs < 1.0 {
        format!("{:.1}ms", secs * 1e3)
    } else {
        format!("{secs:.2}s")
    }
}

/// A value as it reads in a line: bare when it is a word, quoted when it has
/// spaces or quotes or is empty.
fn text(s: &str) -> String {
    if s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '"' || c == '=') {
        format!("{s:?}")
    } else {
        s.to_owned()
    }
}

fn value(v: &Value) -> String {
    match v {
        Value::String(s) => text(s.as_str()),
        other => other.to_string(),
    }
}

fn any_value(v: &AnyValue) -> String {
    match v {
        AnyValue::String(s) => text(s.as_str()),
        AnyValue::Int(i) => i.to_string(),
        AnyValue::Double(d) => d.to_string(),
        AnyValue::Boolean(b) => b.to_string(),
        AnyValue::Bytes(b) => format!("<{} bytes>", b.len()),
        AnyValue::ListAny(items) => {
            let items: Vec<_> = items.iter().map(any_value).collect();
            format!("[{}]", items.join(","))
        }
        AnyValue::Map(map) => {
            let mut items: Vec<_> = map
                .iter()
                .map(|(k, v)| format!("{k}={}", any_value(v)))
                .collect();
            items.sort();
            format!("{{{}}}", items.join(","))
        }
        other => format!("{other:?}"),
    }
}

/// What `tracing-opentelemetry` adds to every span about the span itself — the
/// source line, the thread, the busy and idle times. Exported as they are, and
/// left out of a console line, where they bury the attributes a reader came
/// to see.
fn is_bridge_bookkeeping(key: &str) -> bool {
    key.starts_with("code.") || key.starts_with("thread.") || key == "busy_ns" || key == "idle_ns"
}

/// ` key=value key=value`, or nothing.
fn attributes<'a>(attrs: impl IntoIterator<Item = &'a KeyValue>) -> String {
    attrs
        .into_iter()
        .filter(|kv| !is_bridge_bookkeeping(kv.key.as_str()))
        .fold(String::new(), |mut out, kv| {
            let _ = write!(out, " {}={}", kv.key, value(&kv.value));
            out
        })
}

pub(super) fn span_line(span: &SpanData) -> String {
    let kind = match span.span_kind {
        SpanKind::Client => "client",
        SpanKind::Server => "server",
        SpanKind::Producer => "producer",
        SpanKind::Consumer => "consumer",
        SpanKind::Internal => "internal",
    };
    let took = span
        .end_time
        .duration_since(span.start_time)
        .unwrap_or_default();
    let mut line = format!(
        "{} SPAN {} {kind} {} trace={} span={}",
        rfc3339(span.start_time),
        text(&span.name),
        human(took),
        span.span_context.trace_id(),
        span.span_context.span_id(),
    );
    if span.parent_span_id != opentelemetry::trace::SpanId::INVALID {
        let _ = write!(line, " parent={}", span.parent_span_id);
    }
    match &span.status {
        Status::Unset => {}
        Status::Ok => line.push_str(" status=ok"),
        Status::Error { description } if description.is_empty() => line.push_str(" status=error"),
        Status::Error { description } => {
            let _ = write!(line, " status=error:{}", text(description));
        }
    }
    // A count: a span's events are mostly the `tracing` events that ran
    // inside it, which are `LOG` lines of their own when logs are printed too.
    let events = span.events.iter().count();
    if events > 0 {
        let _ = write!(line, " events={events}");
    }
    line.push_str(&attributes(&span.attributes));
    line
}

pub(super) fn log_line(record: &opentelemetry_sdk::logs::SdkLogRecord) -> String {
    let at = record
        .timestamp()
        .or_else(|| record.observed_timestamp())
        .unwrap_or_else(SystemTime::now);
    let severity = record.severity_text().unwrap_or("LOG");
    let mut line = format!("{} LOG {severity}", rfc3339(at));
    // The body is the message itself, so a string is printed as it is.
    match record.body() {
        Some(AnyValue::String(body)) => {
            let _ = write!(line, " {}", body.as_str());
        }
        Some(body) => {
            let _ = write!(line, " {}", any_value(body));
        }
        None => {}
    }
    if let Some(ctx) = record.trace_context() {
        let _ = write!(line, " trace={} span={}", ctx.trace_id, ctx.span_id);
    }
    for (key, v) in record.attributes_iter() {
        let _ = write!(line, " {key}={}", any_value(v));
    }
    line
}

pub(super) fn metric_lines(metrics: &ResourceMetrics) -> Vec<String> {
    let mut lines = Vec::new();
    for scope in metrics.scope_metrics() {
        for metric in scope.metrics() {
            let (name, unit) = (metric.name(), metric.unit());
            match metric.data() {
                AggregatedMetrics::F64(data) => data_lines(&mut lines, name, unit, data),
                AggregatedMetrics::U64(data) => data_lines(&mut lines, name, unit, data),
                AggregatedMetrics::I64(data) => data_lines(&mut lines, name, unit, data),
            }
        }
    }
    lines
}

fn series<'a>(name: &str, attrs: impl Iterator<Item = &'a KeyValue>) -> String {
    let attrs: Vec<_> = attrs
        .map(|kv| format!("{}={}", kv.key, value(&kv.value)))
        .collect();
    if attrs.is_empty() {
        name.to_owned()
    } else {
        format!("{name}{{{}}}", attrs.join(","))
    }
}

const fn temporality(t: Temporality) -> &'static str {
    match t {
        Temporality::Delta => "delta",
        _ => "cumulative",
    }
}

fn data_lines<T: Display + Copy>(
    lines: &mut Vec<String>,
    name: &str,
    unit: &str,
    data: &MetricData<T>,
) {
    let unit = if unit.is_empty() {
        String::new()
    } else {
        format!(" {unit}")
    };
    match data {
        MetricData::Gauge(g) => {
            for p in g.data_points() {
                lines.push(format!(
                    "{} METRIC {} gauge {}{unit}",
                    rfc3339(g.time()),
                    series(name, p.attributes()),
                    p.value()
                ));
            }
        }
        MetricData::Sum(s) => {
            for p in s.data_points() {
                lines.push(format!(
                    "{} METRIC {} sum {}{unit} {}",
                    rfc3339(s.time()),
                    series(name, p.attributes()),
                    p.value(),
                    temporality(s.temporality())
                ));
            }
        }
        MetricData::Histogram(h) => {
            for p in h.data_points() {
                let mut line = format!(
                    "{} METRIC {} histogram count={} sum={}{unit} {}",
                    rfc3339(h.time()),
                    series(name, p.attributes()),
                    p.count(),
                    p.sum(),
                    temporality(h.temporality())
                );
                if let (Some(min), Some(max)) = (p.min(), p.max()) {
                    let _ = write!(line, " min={min} max={max}");
                }
                lines.push(line);
            }
        }
        MetricData::ExponentialHistogram(h) => {
            for p in h.data_points() {
                lines.push(format!(
                    "{} METRIC {} exponential-histogram count={} sum={}{unit} {}",
                    rfc3339(h.time()),
                    series(name, p.attributes()),
                    p.count(),
                    p.sum(),
                    temporality(h.temporality())
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests;
