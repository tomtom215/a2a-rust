// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The OpenTelemetry half: the caller's trace as the span's parent, and the
//! recorded span as the trace sent downstream (audit O2).

use std::collections::HashMap;

/// The caller's `traceparent` as an OpenTelemetry remote parent.
pub fn remote_parent(headers: &HashMap<String, String>) -> Option<opentelemetry::Context> {
    use opentelemetry::trace::{
        SpanContext, SpanId, TraceContextExt as _, TraceFlags, TraceId, TraceState,
    };
    let inbound = a2a_protocol_types::trace_context::TraceContext::parse(
        headers.get(a2a_protocol_types::trace_context::TRACEPARENT_HEADER)?,
    )
    .ok()?;
    let state = headers
        .get(a2a_protocol_types::trace_context::TRACESTATE_HEADER)
        .and_then(|s| s.parse::<TraceState>().ok())
        .unwrap_or_default();
    let context = SpanContext::new(
        TraceId::from_hex(inbound.trace_id()).ok()?,
        SpanId::from_hex(inbound.span_id()).ok()?,
        TraceFlags::new(inbound.flags()),
        true,
        state,
    );
    Some(opentelemetry::Context::new().with_remote_span_context(context))
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
