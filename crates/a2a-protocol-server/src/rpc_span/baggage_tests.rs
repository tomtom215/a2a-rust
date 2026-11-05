// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Inbound W3C baggage (audit C-10; ADR 0038 §1): opt-in by key, bounded
//! (W3C Baggage §3.3.2), only under `Continue`, and `OTEL_PROPAGATORS`
//! honoured when the handler is built.

#![cfg(feature = "otel")]

use std::collections::HashMap;

use a2a_protocol_types::trace_context::baggage::{BaggagePolicy, Propagators};
use opentelemetry::baggage::BaggageExt as _;
use opentelemetry::trace::TraceContextExt as _;

use super::otel::inbound_parent;
use super::*;
use crate::handler::InboundTracePolicy;

const PARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

fn headers(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn entry(cx: &opentelemetry::Context, key: &str) -> Option<String> {
    cx.baggage().get(key).map(ToString::to_string)
}

#[test]
fn the_allowed_entries_join_the_callers_parent() {
    let h = headers(&[
        ("traceparent", PARENT),
        ("baggage", "tenant=acme,user.id=42"),
    ]);
    let cx = inbound_parent(&h, &BaggagePolicy::keys(["tenant"])).expect("a parent");
    assert_eq!(
        cx.span().span_context().trace_id().to_string(),
        "0af7651916cd43dd8448eb211c80319c"
    );
    assert_eq!(entry(&cx, "tenant").as_deref(), Some("acme"));
    assert_eq!(entry(&cx, "user.id"), None);
    assert_eq!(cx.baggage().len(), 1);
}

#[test]
fn nothing_is_taken_by_default() {
    let h = headers(&[("traceparent", PARENT), ("baggage", "tenant=acme")]);
    let cx = inbound_parent(&h, &BaggagePolicy::Off).expect("the trace parent still");
    assert_eq!(cx.baggage().len(), 0);
    assert!(cx.span().span_context().is_valid());
    let only_baggage = headers(&[("baggage", "tenant=acme")]);
    assert!(inbound_parent(&only_baggage, &BaggagePolicy::Off).is_none());
}

/// Baggage without a `traceparent` still reaches the call, as a context
/// with no remote span: the span is then a root.
#[test]
fn baggage_without_a_traceparent_is_a_context_of_its_own() {
    let h = headers(&[("Baggage", "tenant=acme")]);
    let cx = inbound_parent(&h, &BaggagePolicy::All).expect("a context");
    assert!(!cx.span().span_context().is_valid());
    assert_eq!(entry(&cx, "tenant").as_deref(), Some("acme"));
    assert!(inbound_parent(&headers(&[]), &BaggagePolicy::All).is_none());
}

#[test]
fn a_header_over_the_w3c_limit_is_dropped_whole() {
    let long = format!("tenant=acme,pad={}", "x".repeat(8192));
    let h = headers(&[("traceparent", PARENT), ("baggage", &long)]);
    let cx = inbound_parent(&h, &BaggagePolicy::All).expect("the trace parent");
    assert_eq!(cx.baggage().len(), 0);
}

fn built(propagators: Option<Propagators>, policy: InboundTracePolicy) -> RequestHandler {
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    let builder = crate::RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_inbound_trace_policy(policy)
        .with_baggage_propagation(BaggagePolicy::keys(["tenant"]));
    match propagators {
        Some(p) => builder.with_propagators(p),
        None => builder,
    }
    .build()
    .unwrap()
}

/// `OTEL_PROPAGATORS` without `tracecontext` stops the caller's trace being
/// continued (`Continue` acts as `Restart`; `Drop` stays `Drop`), and
/// without `baggage` no baggage is taken whatever the policy says.
#[test]
fn the_propagators_decide_what_is_read() {
    let all = built(Some(Propagators::default()), InboundTracePolicy::Continue);
    assert_eq!(all.inbound_trace_policy, InboundTracePolicy::Continue);
    assert_eq!(all.baggage, BaggagePolicy::keys(["tenant"]));
    let baggage_only = built(
        Some(Propagators::from_env_value(Some("baggage"))),
        InboundTracePolicy::Continue,
    );
    assert_eq!(
        baggage_only.inbound_trace_policy,
        InboundTracePolicy::Restart
    );
    assert_eq!(baggage_only.baggage, BaggagePolicy::keys(["tenant"]));
    let none = built(
        Some(Propagators::from_env_value(Some("none"))),
        InboundTracePolicy::Continue,
    );
    assert_eq!(none.inbound_trace_policy, InboundTracePolicy::Restart);
    assert_eq!(none.baggage, BaggagePolicy::Off);
    let dropped = built(
        Some(Propagators::from_env_value(Some("none"))),
        InboundTracePolicy::Drop,
    );
    assert_eq!(dropped.inbound_trace_policy, InboundTracePolicy::Drop);
}

/// The call's span, and a span opened inside it (an executor's outbound
/// call), carry the allowed baggage — which is what the client sends on —
/// under `Continue`, and none under `Restart`.
#[test]
fn the_calls_context_carries_the_allowed_baggage_to_its_children() {
    use opentelemetry::trace::TracerProvider as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    let seen = |policy| {
        let handler = built(Some(Propagators::default()), policy);
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
        let subscriber = tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("t")));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let h = headers(&[
            ("traceparent", PARENT),
            ("baggage", "tenant=acme,user.id=42"),
        ]);
        tracing::subscriber::with_default(subscriber, || {
            runtime.block_on(
                ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", Some(&h)).run(async {
                    let call = tracing::Span::current().context();
                    let child = tracing::info_span!("outbound").context();
                    Ok::<_, crate::error::ServerError>((
                        entry(&call, "tenant"),
                        entry(&child, "tenant"),
                        entry(&child, "user.id"),
                    ))
                }),
            )
        })
        .unwrap()
    };
    assert_eq!(
        seen(InboundTracePolicy::Continue),
        (Some("acme".to_owned()), Some("acme".to_owned()), None)
    );
    assert_eq!(seen(InboundTracePolicy::Restart), (None, None, None));
}
