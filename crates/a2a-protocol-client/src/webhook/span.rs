// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The span each webhook request is checked in.
//!
//! [`WebhookReceiver::receive`](super::WebhookReceiver::receive) is handed a
//! request the integrator's HTTP server already accepted, so its span is
//! `INTERNAL`, named `a2a.webhook.receive`, and a child of whatever span is
//! current — the server's span, when the integrator's framework records one.
//! When none is recorded and the request carries a `traceparent` (the
//! agent's push sender sends its POST's own context, server
//! `rpc_span/webhook.rs`), the span continues that trace instead, so the
//! notification is found under the task that caused it.
//!
//! It records the outcome as `a2a.webhook.outcome` (`accepted`, `duplicate`
//! or `rejected`), the task once the request authenticated, and, for a
//! rejection, `error.type` (the rejection's kind, below) and `Error` status.
//! Never the token, the `Authorization` header or the body.

use super::outcome::{WebhookOutcome, WebhookRejection};

/// The span of one `receive`.
#[derive(Debug)]
pub struct ReceiveSpan {
    #[cfg(feature = "tracing")]
    pub(super) span: tracing::Span,
}

impl ReceiveSpan {
    /// Opens it for a request with `headers`.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn open(headers: &hyper::HeaderMap) -> Self {
        #[cfg(feature = "tracing")]
        {
            let span = tracing::info_span!(
                target: "a2a_protocol_client::webhook",
                "a2a.webhook.receive",
                otel.status_code = tracing::field::Empty,
                error.type = tracing::field::Empty,
                a2a.webhook.outcome = tracing::field::Empty,
                a2a.task.id = tracing::field::Empty,
            );
            #[cfg(feature = "otel")]
            continue_sender_trace(&span, headers);
            Self { span }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {}
        }
    }

    /// Records how the request was answered.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::unused_self, clippy::missing_const_for_fn)
    )]
    pub fn ended(&self, outcome: &WebhookOutcome) {
        #[cfg(feature = "tracing")]
        match outcome {
            WebhookOutcome::Accepted { task_id, .. } => {
                self.span.record("a2a.webhook.outcome", "accepted");
                self.span.record("a2a.task.id", task_id.0.as_str());
            }
            WebhookOutcome::Duplicate { task_id, .. } => {
                self.span.record("a2a.webhook.outcome", "duplicate");
                self.span.record("a2a.task.id", task_id.0.as_str());
            }
            WebhookOutcome::Rejected(rejection) => {
                self.span.record("a2a.webhook.outcome", "rejected");
                self.span.record("error.type", kind(*rejection));
                self.span.record("otel.status_code", "ERROR");
            }
        }
    }
}

/// A rejection as a low-cardinality `error.type`.
#[cfg_attr(not(feature = "tracing"), allow(dead_code))]
const fn kind(rejection: WebhookRejection) -> &'static str {
    match rejection {
        WebhookRejection::MissingToken => "missing_token",
        WebhookRejection::UnknownToken => "unknown_token",
        WebhookRejection::BadAuthorization => "bad_authorization",
        WebhookRejection::MalformedBody => "malformed_body",
        WebhookRejection::TaskMismatch => "task_mismatch",
        WebhookRejection::RateLimited => "rate_limited",
    }
}

/// Makes `span` a child of the request's `traceparent` when no recorded
/// span is current. A malformed `traceparent` is ignored, as W3C Trace
/// Context asks of a receiver.
#[cfg(feature = "otel")]
fn continue_sender_trace(span: &tracing::Span, headers: &hyper::HeaderMap) {
    use a2a_protocol_types::trace_context::{TRACEPARENT_HEADER, TRACESTATE_HEADER, TraceContext};
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;

    if span.is_disabled()
        || tracing::Span::current()
            .context()
            .span()
            .span_context()
            .is_valid()
    {
        return;
    }
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let Some(trace) = header(TRACEPARENT_HEADER).and_then(|v| TraceContext::parse(v).ok()) else {
        return;
    };
    let trace = match header(TRACESTATE_HEADER) {
        Some(state) => trace.clone().with_tracestate(state).unwrap_or(trace),
        None => trace,
    };
    if let Some(parent) = crate::call_span::remote_context(&trace) {
        let _ = span.set_parent(parent);
    }
}
