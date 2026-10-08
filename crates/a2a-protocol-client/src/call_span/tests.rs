// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the call's span: what it records, and the context it sends.
//!
//! These live in this crate, not only in the SDK's end-to-end tests, because
//! the incremental mutation gate runs the mutated crate's own tests: with
//! only the SDK's, 30 mutants in this file survived.

use super::*;

/// Every method the client sends has a qualified name; none falls to
/// `_OTHER`.
#[cfg(feature = "tracing")]
#[test]
fn every_client_method_has_a_qualified_name() {
    for m in [
        "SendMessage",
        "SendStreamingMessage",
        "GetTask",
        "ListTasks",
        "CancelTask",
        "SubscribeToTask",
        "CreateTaskPushNotificationConfig",
        "GetTaskPushNotificationConfig",
        "ListTaskPushNotificationConfigs",
        "DeleteTaskPushNotificationConfig",
        "GetExtendedAgentCard",
    ] {
        assert_eq!(qualified(m), format!("lf.a2a.v1.A2AService/{m}"));
    }
    assert_eq!(qualified("message/send"), "_OTHER");
}

#[test]
fn peer_reads_system_address_and_default_port() {
    let p = Peer::new("JSONRPC", "https://agents.example.com/a2a", None);
    assert_eq!(p.system, Some("jsonrpc"));
    assert_eq!(p.address.as_deref(), Some("agents.example.com"));
    assert_eq!(p.port, Some(443));
    assert!(p.carries_headers);
    let p = Peer::new("HTTP+JSON", "http://[::1]:8080", None);
    assert_eq!(p.system, Some("a2a_http_json"));
    assert_eq!(p.address.as_deref(), Some("::1"));
    assert_eq!(p.port, Some(8080));
    let p = Peer::new("SOMETHING", "not a url", None);
    assert_eq!(
        (p.system, p.address, p.port, p.carries_headers),
        (None, None, None, false)
    );
}
/// The identity a client's spans carry is the card's, field for field.
#[test]
fn agent_identity_is_the_cards_name_description_and_version() {
    let card = AgentCard::new(
        "worker",
        "2.1.0",
        a2a_protocol_types::agent_card::AgentInterface::jsonrpc("http://agent.example"),
    )
    .with_description("does work");
    assert_eq!(
        *agent_identity(&card),
        (
            "worker".to_owned(),
            "does work".to_owned(),
            "2.1.0".to_owned()
        )
    );
}

/// Every field recorded on any span, including those recorded after it
/// opened.
#[cfg(feature = "tracing")]
#[derive(Clone, Default)]
struct Fields(Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>);

#[cfg(feature = "tracing")]
struct Collect<'a>(&'a mut std::collections::BTreeMap<String, String>);

#[cfg(feature = "tracing")]
impl tracing::field::Visit for Collect<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[cfg(feature = "tracing")]
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Fields {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        attrs.record(&mut Collect(&mut self.0.lock().unwrap()));
    }
    fn on_record(
        &self,
        _id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        values.record(&mut Collect(&mut self.0.lock().unwrap()));
    }
}

/// The fields `f` records, with nothing but [`Fields`] subscribed.
#[cfg(feature = "tracing")]
fn recorded(f: impl FnOnce()) -> std::collections::BTreeMap<String, String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    let fields = Fields::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(fields.clone()), f);
    let recorded = fields.0.lock().unwrap();
    recorded.clone()
}

#[cfg(feature = "tracing")]
fn peer_with(description: &str, version: &str) -> Peer {
    Peer::new(
        "JSONRPC",
        "http://agent.example:9000",
        Some(Arc::new((
            "worker".to_owned(),
            description.to_owned(),
            version.to_owned(),
        ))),
    )
}

/// A send answered with a task records the task's id, context and state
/// from the response, and the agent from its card.
#[cfg(feature = "tracing")]
#[test]
fn a_successful_call_records_the_task_and_the_agent() {
    let fields = recorded(|| {
        let params = serde_json::json!({"message": {"messageId": "m1", "contextId": "ctx-req"}});
        let call = CallSpan::open("SendMessage", &peer_with("does work", "2.1.0"), &params);
        call.succeeded(Some(&serde_json::json!({"task": {
            "id": "t1",
            "contextId": "ctx-1",
            "status": {"state": "TASK_STATE_COMPLETED"}
        }})));
    });
    let field = |k: &str| fields.get(k).map(String::as_str);
    assert_eq!(field("a2a.message.id"), Some("m1"));
    assert_eq!(field("a2a.task.state"), Some("TASK_STATE_COMPLETED"));
    assert_eq!(field("a2a.task.id"), Some("t1"));
    assert_eq!(field("gen_ai.conversation.id"), Some("ctx-1"));
    assert_eq!(field("gen_ai.agent.name"), Some("worker"));
    assert_eq!(field("gen_ai.agent.description"), Some("does work"));
    assert_eq!(field("gen_ai.agent.version"), Some("2.1.0"));
    assert_eq!(field("error.type"), None);
}

/// An empty description or version is not available from the card, and is
/// not recorded.
#[cfg(feature = "tracing")]
#[test]
fn an_empty_description_or_version_is_not_recorded() {
    let fields = recorded(|| {
        let call = CallSpan::open(
            "GetTask",
            &peer_with("", ""),
            &serde_json::json!({"id": "t1"}),
        );
        call.succeeded(None);
    });
    assert_eq!(
        fields.get("gen_ai.agent.name").map(String::as_str),
        Some("worker")
    );
    assert!(
        !fields.contains_key("gen_ai.agent.description"),
        "{fields:?}"
    );
    assert!(!fields.contains_key("gen_ai.agent.version"), "{fields:?}");
}

/// A failed call records `error.type`, an error status, and the task the
/// request named.
#[cfg(feature = "tracing")]
#[test]
fn a_failed_call_records_its_error_and_the_requested_task() {
    let fields = recorded(|| {
        let call = CallSpan::open(
            "GetTask",
            &Peer::default(),
            &serde_json::json!({"id": "t9"}),
        );
        call.failed(&crate::error::ClientError::Timeout("slow".into()));
    });
    let field = |k: &str| fields.get(k).map(String::as_str);
    assert_eq!(field("error.type"), Some("timeout"));
    assert_eq!(field("otel.status_code"), Some("ERROR"));
    assert_eq!(field("a2a.task.id"), Some("t9"));
}

/// Each class of failure has its own `error.type`; anything else is
/// `_OTHER`.
#[cfg(feature = "tracing")]
#[test]
fn every_error_class_has_its_error_type() {
    use crate::error::ClientError as E;
    let serialization = serde_json::from_str::<u8>("not json").unwrap_err();
    for (error, expected) in [
        (
            E::Protocol(a2a_protocol_types::error::A2aError::task_not_found("t")),
            "-32001",
        ),
        (
            E::UnexpectedStatus {
                status: 503,
                body: String::new(),
                retry_after: None,
            },
            "503",
        ),
        (E::Timeout("slow".into()), "timeout"),
        (E::HttpClient("refused".into()), "transport"),
        (E::Transport("reset".into()), "transport"),
        (E::Serialization(serialization), "serialization"),
        (E::InvalidEndpoint("no scheme".into()), "invalid_endpoint"),
        (
            E::AuthRequired {
                task_id: a2a_protocol_types::task::TaskId::new("t"),
            },
            "auth_required",
        ),
        (E::ProtocolBindingMismatch("x".into()), "_OTHER"),
    ] {
        assert_eq!(error_type(&error), expected, "{error:?}");
    }
}

/// `f` with a `tracing-opentelemetry` layer recording every span.
#[cfg(feature = "otel")]
fn with_otel<R>(f: impl FnOnce() -> R) -> R {
    use opentelemetry::trace::TracerProvider as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("call-span-test"));
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), f)
}

/// The `traceparent` [`propagate`] writes for a fresh call, and that call's
/// own trace and span ids.
#[cfg(feature = "otel")]
fn propagated(
    peer: &Peer,
    enabled: bool,
) -> (
    std::collections::HashMap<String, String>,
    opentelemetry::trace::SpanContext,
) {
    use opentelemetry::trace::TraceContextExt as _;
    use tracing_opentelemetry::OpenTelemetrySpanExt as _;
    let call = CallSpan::open("SendMessage", peer, &serde_json::json!({}));
    let mut headers = std::collections::HashMap::new();
    propagate(&call, peer, enabled, &mut headers);
    let sc = call.span.context().span().span_context().clone();
    (headers, sc)
}

#[cfg(feature = "otel")]
fn traceparent(
    headers: &std::collections::HashMap<String, String>,
) -> a2a_protocol_types::trace_context::TraceContext {
    use a2a_protocol_types::trace_context::{TRACEPARENT_HEADER, TraceContext};
    TraceContext::parse(&headers[TRACEPARENT_HEADER]).expect("a valid traceparent")
}

/// With a recording layer, the call's `traceparent` names the call's own
/// span.
#[cfg(feature = "otel")]
#[test]
fn the_traceparent_names_the_call_span() {
    let (headers, sc) = with_otel(|| propagated(&Peer::new("JSONRPC", "http://a:1", None), true));
    let sent = traceparent(&headers);
    assert!(sc.is_valid());
    assert_eq!(sent.trace_id(), sc.trace_id().to_string());
    assert_eq!(sent.span_id(), sc.span_id().to_string());
}

/// Propagation turned off, or a transport without per-request headers,
/// sends nothing.
#[cfg(feature = "otel")]
#[test]
fn nothing_is_sent_when_off_or_without_per_request_headers() {
    let jsonrpc = Peer::new("JSONRPC", "http://a:1", None);
    let (headers, _) = with_otel(|| propagated(&jsonrpc, false));
    assert!(headers.is_empty(), "{headers:?}");
    let custom = Peer::new("CUSTOM", "ws://a:1", None);
    let (headers, _) = with_otel(|| propagated(&custom, true));
    assert!(headers.is_empty(), "{headers:?}");
}

/// A `CurrentTrace` scope is the call's parent when no recorded span is
/// current — and only then: a recorded span, when there is one, wins.
#[cfg(feature = "otel")]
#[test]
fn a_current_trace_is_the_parent_only_without_a_recorded_span() {
    let ambient = crate::CurrentTrace::start_root();
    let peer = Peer::new("JSONRPC", "http://a:1", None);
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");

    let alone = with_otel(|| {
        rt.block_on(crate::CurrentTrace::scope(ambient.clone(), async {
            propagated(&peer, true).0
        }))
    });
    assert_eq!(traceparent(&alone).trace_id(), ambient.trace_id());

    let (under_span, outer) = with_otel(|| {
        use opentelemetry::trace::TraceContextExt as _;
        use tracing_opentelemetry::OpenTelemetrySpanExt as _;
        let outer = tracing::info_span!("caller");
        let _entered = outer.enter();
        let outer_trace = outer.context().span().span_context().trace_id();
        let headers = rt.block_on(crate::CurrentTrace::scope(ambient.clone(), async {
            propagated(&peer, true).0
        }));
        (headers, outer_trace)
    });
    let sent = traceparent(&under_span);
    assert_eq!(sent.trace_id(), outer.to_string());
    assert_ne!(sent.trace_id(), ambient.trace_id());
}
