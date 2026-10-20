// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A call related to a trace it is not a child of.
//!
//! A WebSocket connection's calls arrive as frames, which carry no trace
//! context: the A2A WebSocket binding defines none, and this SDK's client
//! sends none (`a2a-protocol-client`, `transport/websocket.rs`). The only
//! `traceparent` such a call has is the upgrade request's, and that names
//! the operation that opened the connection, not the one that sent this
//! frame — W3C Trace Context §3.4 has every request name its own parent, so
//! taking it as the parent of every call on the connection reported calls
//! from many operations as children of one. A span link is the OpenTelemetry
//! relation for a span caused by, but not part of, another trace.

use std::collections::HashMap;

use super::ServerSpan;
use crate::handler::RequestHandler;

impl ServerSpan {
    /// Links the span to the remote span `headers` name in a `traceparent`
    /// under `parent` and a `tracestate` under `state`, as related to the
    /// call rather than its parent. Only under
    /// [`InboundTracePolicy::Continue`](crate::InboundTracePolicy::Continue),
    /// the one policy that trusts a peer's trace; without the `otel` feature,
    /// or with no OpenTelemetry layer recording, nothing is linked.
    #[must_use]
    // Without `otel` the body is only `self`, which clippy would make
    // `const`; with it, the link is added, which cannot be.
    #[cfg_attr(
        not(feature = "otel"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    #[cfg_attr(not(feature = "websocket"), allow(dead_code))]
    pub fn with_link(
        self,
        handler: &RequestHandler,
        headers: &HashMap<String, String>,
        parent: &str,
        state: &str,
    ) -> Self {
        #[cfg(feature = "otel")]
        if handler.inbound_trace_policy == crate::handler::InboundTracePolicy::Continue
            && let Some(linked) = super::otel::remote_span(headers, parent, state)
        {
            use tracing_opentelemetry::OpenTelemetrySpanExt as _;
            self.span.add_link(linked);
        }
        self
    }
}

#[cfg(test)]
#[cfg(feature = "otel")]
mod tests {
    use std::sync::{Arc, Mutex};

    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanProcessor};
    use tracing_subscriber::layer::SubscriberExt as _;

    use super::*;
    use crate::handler::InboundTracePolicy;
    use crate::rpc_span::RpcSystem;

    /// Every span the provider ends.
    #[derive(Debug, Default, Clone)]
    struct Ended(Arc<Mutex<Vec<SpanData>>>);

    impl SpanProcessor for Ended {
        fn on_start(&self, _: &mut opentelemetry_sdk::trace::Span, _: &opentelemetry::Context) {}
        fn on_end(&self, span: SpanData) {
            self.0.lock().unwrap().push(span);
        }
        fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
            Ok(())
        }
        fn shutdown_with_timeout(
            &self,
            _: std::time::Duration,
        ) -> opentelemetry_sdk::error::OTelSdkResult {
            Ok(())
        }
    }

    const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
    const SPAN: &str = "00f067aa0ba902b7";

    /// The span one call on a connection records, under `policy`, when the
    /// upgrade carried a `traceparent`.
    fn call_span(policy: InboundTracePolicy) -> SpanData {
        struct Idle;
        crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
        let ended = Ended::default();
        let provider = SdkTracerProvider::builder()
            .with_span_processor(ended.clone())
            .build();
        let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("link-test"));
        let handler = crate::RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_inbound_trace_policy(policy)
            .build()
            .unwrap();
        let headers = HashMap::from([
            (":up-parent".to_owned(), format!("00-{TRACE}-{SPAN}-01")),
            (":up-state".to_owned(), "vendor=1".to_owned()),
        ]);
        tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
            let call = ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", Some(&headers))
                .with_link(&handler, &headers, ":up-parent", ":up-state");
            drop(call);
        });
        let spans = ended.0.lock().unwrap();
        assert_eq!(spans.len(), 1, "{spans:?}");
        spans[0].clone()
    }

    /// Linked, not parented: a root span of its own with the upgrade's span,
    /// its flags and its state as a link.
    #[test]
    fn a_call_links_to_the_upgrade_trace_without_being_its_child() {
        let span = call_span(InboundTracePolicy::Continue);
        assert_ne!(span.span_context.trace_id().to_string(), TRACE);
        assert_eq!(span.parent_span_id, opentelemetry::trace::SpanId::INVALID);
        let links: Vec<_> = span.links.iter().map(|l| &l.span_context).collect();
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].trace_id().to_string(), TRACE);
        assert_eq!(links[0].span_id().to_string(), SPAN);
        assert!(links[0].is_sampled() && links[0].is_remote());
        assert_eq!(links[0].trace_state().header(), "vendor=1");
    }

    /// `Restart` trusts no peer's trace, so it links to none either.
    #[test]
    fn a_restarted_trace_links_to_nothing() {
        let span = call_span(InboundTracePolicy::Restart);
        assert!(span.links.is_empty(), "{:?}", span.links);
        assert_ne!(span.span_context.trace_id().to_string(), TRACE);
    }
}
