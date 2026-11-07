// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What `install_subscriber` prints to standard error: the text `tracing`
//! prints by default, or one JSON object per line carrying the trace context
//! (audit C-06; ADR 0038 §6).
//!
//! The OpenTelemetry compatibility note "Trace Context in non-OTLP Log
//! Formats" (`specification/compatibility/logging_trace_context.md`) names
//! the fields of a log line outside OTLP: `trace_id` and `span_id` as
//! lower-case hex, and `trace_flags` as two hex digits. They are written
//! inside a span that has a valid OpenTelemetry context, and left out
//! elsewhere.

use std::fmt;
use std::time::SystemTime;

use opentelemetry::trace::TraceContextExt as _;
use serde_json::{Map, Value};
use tracing::field::{Field, Visit};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

use super::config::Notes;
use super::console::rfc3339;

/// The format of the lines `install_subscriber` prints.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsoleFormat {
    /// `tracing-subscriber`'s text, as before 0.16.0.
    #[default]
    Text,
    /// One JSON object per line: `timestamp`, `level`, `target`, `message`,
    /// the event's own fields and, inside a span with an OpenTelemetry
    /// context, `trace_id`, `span_id` and `trace_flags`.
    Json,
}

/// The variable that chooses the format when the code does not.
pub(super) const VARIABLE: &str = "A2A_LOG_FORMAT";

/// The format `A2A_LOG_FORMAT` names (`json` or `text`, in any case); a
/// value it does not know is noted and the text format used, as F3 has an
/// unusable value do.
pub(super) fn from_env(value: Option<&str>, notes: &Notes) -> ConsoleFormat {
    match value.map(str::trim) {
        None | Some("") => ConsoleFormat::Text,
        Some(v) if v.eq_ignore_ascii_case("json") => ConsoleFormat::Json,
        Some(v) if v.eq_ignore_ascii_case("text") => ConsoleFormat::Text,
        Some(v) => {
            notes.ignored(
                VARIABLE,
                v,
                "is not `json` or `text`",
                "the text format is used",
            );
            ConsoleFormat::Text
        }
    }
}

/// Formats each event as one JSON line.
pub(super) struct JsonLines;

impl<S, N> FormatEvent<S, N> for JsonLines
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let mut line = Map::new();
        line.insert("timestamp".into(), rfc3339(SystemTime::now()).into());
        line.insert("level".into(), meta.level().as_str().into());
        line.insert("target".into(), meta.target().into());
        let mut fields = Fields(Map::new());
        event.record(&mut fields);
        let mut fields = fields.0;
        line.insert(
            "message".into(),
            fields
                .remove("message")
                .unwrap_or(Value::String(String::new())),
        );
        for (key, value) in fields {
            line.entry(key).or_insert(value);
        }
        // `tracing::Span::current()` is the none span here: the dispatcher
        // is busy delivering this event, and `tracing` hands a re-entrant
        // lookup no subscriber. The OpenTelemetry context the tracing layer
        // activates when a span is entered (on by default in
        // tracing-opentelemetry 0.34) is the one the event happened in.
        let context = opentelemetry::Context::current();
        let span = context.span();
        let span_context = span.span_context();
        if span_context.is_valid() {
            line.insert(
                "trace_id".into(),
                span_context.trace_id().to_string().into(),
            );
            line.insert("span_id".into(), span_context.span_id().to_string().into());
            line.insert(
                "trace_flags".into(),
                format!("{:02x}", span_context.trace_flags().to_u8()).into(),
            );
        }
        writeln!(writer, "{}", Value::Object(line))
    }
}

/// An event's fields as JSON values.
struct Fields(Map<String, Value>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().to_owned(), value.into());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().to_owned(), format!("{value:?}").into());
    }
}

#[cfg(test)]
mod tests;
