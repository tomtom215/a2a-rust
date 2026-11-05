// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The OpenTelemetry half: the caller's trace as the span's parent, and the
//! recorded span as the trace sent downstream (audit O2).

use std::collections::HashMap;

/// The parent context of an inbound call under `Continue`: the caller's
/// `traceparent` as a remote parent, with the entries of its `baggage`
/// header that `baggage` lets in (audit C-10). `None` when there is
/// neither. The handler's propagators were applied when it was built: a
/// `traceparent` is read only when `tracecontext` is on, which is what
/// left the policy at `Continue`, and `baggage` is `Off` without `baggage`.
pub fn inbound_parent(
    headers: &HashMap<String, String>,
    baggage: &a2a_protocol_types::trace_context::baggage::BaggagePolicy,
) -> Option<opentelemetry::Context> {
    use a2a_protocol_types::trace_context::baggage::{BAGGAGE_HEADER, parse};
    use opentelemetry::baggage::{BaggageExt as _, KeyValueMetadata};
    let parent = remote_parent(headers);
    let members = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(BAGGAGE_HEADER))
        .map(|(_, v)| parse(v, baggage))
        .unwrap_or_default();
    if members.is_empty() {
        return parent;
    }
    let entries = members
        .into_iter()
        .map(|m| KeyValueMetadata::new(m.key, m.value, m.properties.unwrap_or_default()));
    Some(parent.unwrap_or_default().with_baggage(entries))
}

/// The caller's `traceparent` as an OpenTelemetry remote parent.
pub fn remote_parent(headers: &HashMap<String, String>) -> Option<opentelemetry::Context> {
    use opentelemetry::trace::TraceContextExt as _;
    let context = remote_span(
        headers,
        a2a_protocol_types::trace_context::TRACEPARENT_HEADER,
        a2a_protocol_types::trace_context::TRACESTATE_HEADER,
    )?;
    Some(opentelemetry::Context::new().with_remote_span_context(context))
}

/// The remote span `headers` name in a `traceparent` under `parent` and a
/// `tracestate` under `state`.
pub fn remote_span(
    headers: &HashMap<String, String>,
    parent: &str,
    state: &str,
) -> Option<opentelemetry::trace::SpanContext> {
    use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId, TraceState};
    let inbound =
        a2a_protocol_types::trace_context::TraceContext::parse(headers.get(parent)?).ok()?;
    let state = headers
        .get(state)
        .and_then(|s| s.parse::<TraceState>().ok())
        .unwrap_or_default();
    Some(SpanContext::new(
        TraceId::from_hex(inbound.trace_id()).ok()?,
        SpanId::from_hex(inbound.span_id()).ok()?,
        TraceFlags::new(inbound.flags()),
        true,
        state,
    ))
}

/// The recorded span this code is running in, as the trace context to send
/// downstream: its trace id, its own span id, its flags. `None` when no
/// OpenTelemetry layer records spans, and the caller mints one instead.
pub fn current_recorded_span() -> Option<([u8; 16], [u8; 8], u8)> {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;
    let context = tracing::Span::current().context();
    let span = context.span();
    let sc = span.span_context();
    sc.is_valid().then(|| {
        (
            sc.trace_id().to_bytes(),
            sc.span_id().to_bytes(),
            sc.trace_flags().to_u8(),
        )
    })
}

/// `span`'s own context as `traceparent` and `tracestate` header values,
/// the state empty when it has none; `None` when no OpenTelemetry layer
/// records the span.
pub fn trace_headers(span: &tracing::Span) -> Option<(String, String)> {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;
    let context = span.context();
    let span = context.span();
    let sc = span.span_context();
    if !sc.is_valid() {
        return None;
    }
    let parent = a2a_protocol_types::trace_context::TraceContext::from_bytes(
        sc.trace_id().to_bytes(),
        sc.span_id().to_bytes(),
        sc.trace_flags().to_u8(),
    )
    .ok()?
    .traceparent();
    Some((parent, sc.trace_state().header()))
}
