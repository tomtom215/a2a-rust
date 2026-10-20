// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`ServerSpan`](super::ServerSpan) and its helpers.

use super::*;
use crate::error::ServerError;
use a2a_protocol_types::error::A2aError;

#[test]
fn every_served_method_is_qualified_and_anything_else_is_other() {
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
        assert_eq!(a2a_method(m), format!("lf.a2a.v1.A2AService/{m}"));
    }
    assert_eq!(
        a2a_method("message/stream"),
        "lf.a2a.v1.A2AService/SendStreamingMessage"
    );
    assert_eq!(a2a_method("message/send"), "_OTHER");
    assert_eq!(a2a_method("DropTables; --"), "_OTHER");
}

#[test]
fn rpc_system_names_are_the_conventions_values() {
    assert_eq!(RpcSystem::JsonRpc.name(), "jsonrpc");
    assert_eq!(RpcSystem::Grpc.name(), "grpc");
    assert_eq!(RpcSystem::HttpJson.name(), "a2a_http_json");
}

#[test]
fn each_binding_reports_the_status_it_sends() {
    let not_found = ServerError::TaskNotFound("t".into());
    assert_eq!(not_found.wire_status(RpcSystem::JsonRpc), "-32001");
    assert_eq!(not_found.wire_status(RpcSystem::HttpJson), "404");
    let busy = ServerError::Overloaded("busy".into());
    assert_eq!(busy.wire_status(RpcSystem::HttpJson), "503");
    assert_eq!(
        A2aError::invalid_params("x").wire_status(RpcSystem::JsonRpc),
        "-32602"
    );
}

#[cfg(feature = "grpc")]
#[test]
fn grpc_reports_the_status_name_it_sends() {
    assert_eq!(
        ServerError::TaskNotFound("t".into()).wire_status(RpcSystem::Grpc),
        "NOT_FOUND"
    );
    assert_eq!(
        ServerError::Overloaded("busy".into()).wire_status(RpcSystem::Grpc),
        "RESOURCE_EXHAUSTED"
    );
}

type Seen = Arc<std::sync::Mutex<Vec<(Option<String>, Option<String>)>>>;

struct Recording(Seen);

impl Metrics for Recording {
    fn on_rpc_call(&self, call: &RpcCall<'_>) {
        self.0.lock().unwrap().push((
            call.method.map(str::to_owned),
            call.error_type.map(str::to_owned),
        ));
    }
}

fn handler(seen: &Seen) -> RequestHandler {
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    crate::RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_metrics(Recording(Arc::clone(seen)))
        .build()
        .unwrap()
}

/// A call whose future is dropped before it finishes — the peer went
/// away, or its client timed out behind a hung executor — is recorded as
/// failed, once, rather than not at all (audit O11).
#[tokio::test]
async fn a_call_dropped_in_flight_is_recorded_as_cancelled() {
    let seen = Seen::default();
    let handler = handler(&seen);
    let call = ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", None)
        .run(std::future::pending::<Result<(), ServerError>>());
    // Polled, so the call is in flight, then abandoned.
    assert!(
        tokio::time::timeout(Duration::from_millis(10), call)
            .await
            .is_err()
    );
    assert_eq!(
        *seen.lock().unwrap(),
        [(
            Some("lf.a2a.v1.A2AService/GetTask".to_owned()),
            Some("cancelled".to_owned())
        )]
    );
}

/// A call that finishes is recorded once, and not again as cancelled.
#[tokio::test]
async fn a_finished_call_is_recorded_once() {
    let seen = Seen::default();
    let handler = handler(&seen);
    let _ = ServerSpan::open(&handler, RpcSystem::HttpJson, "ListTasks", None)
        .run(async { Ok::<_, ServerError>(()) })
        .await;
    assert_eq!(
        *seen.lock().unwrap(),
        [(Some("lf.a2a.v1.A2AService/ListTasks".to_owned()), None)]
    );
}

/// Which statuses are server failures is each binding's conventions' rule:
/// every JSON-RPC code; a 5xx but not a 4xx; gRPC's six server-side codes.
#[test]
fn only_server_failures_are_errors() {
    for code in ["-32001", "-32602", "-32603"] {
        assert!(RpcSystem::JsonRpc.is_server_error(code), "{code}");
    }
    for (code, error) in [("400", false), ("404", false), ("429", false)] {
        assert_eq!(RpcSystem::HttpJson.is_server_error(code), error, "{code}");
    }
    for code in ["500", "503", "not a status"] {
        assert!(RpcSystem::HttpJson.is_server_error(code), "{code}");
    }
    let grpc_errors = [
        "UNKNOWN",
        "DEADLINE_EXCEEDED",
        "UNIMPLEMENTED",
        "INTERNAL",
        "UNAVAILABLE",
        "DATA_LOSS",
    ];
    let grpc_answers = [
        "CANCELLED",
        "INVALID_ARGUMENT",
        "NOT_FOUND",
        "ALREADY_EXISTS",
        "PERMISSION_DENIED",
        "RESOURCE_EXHAUSTED",
        "FAILED_PRECONDITION",
        "ABORTED",
        "OUT_OF_RANGE",
        "UNAUTHENTICATED",
    ];
    let custom = CustomBinding::new("custom", |_| "x".into());
    for code in grpc_errors {
        assert!(RpcSystem::Grpc.is_server_error(code), "{code}");
        let grpc_rule = RpcSystem::Custom(custom.with_grpc_server_errors());
        assert!(grpc_rule.is_server_error(code), "{code}");
    }
    for code in grpc_answers {
        assert!(!RpcSystem::Grpc.is_server_error(code), "{code}");
        let grpc_rule = RpcSystem::Custom(custom.with_grpc_server_errors());
        assert!(!grpc_rule.is_server_error(code), "{code}");
        // A custom binding that did not ask for gRPC's rule keeps counting
        // every failure.
        assert!(RpcSystem::Custom(custom).is_server_error(code), "{code}");
    }
}

/// A call answered with a status that is no server failure records the
/// status and no `error.type`; one that is, both — and a call that never
/// finished is a failure on every binding, `cancelled` included in gRPC's.
#[tokio::test]
async fn a_client_class_status_is_recorded_without_an_error_type() {
    let seen = Seen::default();
    let handler = handler(&seen);
    let get_task = || Some("lf.a2a.v1.A2AService/GetTask".to_owned());
    let mut want = Vec::new();
    #[cfg(feature = "grpc")]
    {
        for err in [
            ServerError::TaskNotFound("t".into()),
            ServerError::Internal("broke".into()),
        ] {
            let _ = ServerSpan::open(&handler, RpcSystem::Grpc, "GetTask", None)
                .run(async { Err::<(), _>(err) })
                .await;
        }
        want.extend([
            (get_task(), None),
            (get_task(), Some("INTERNAL".to_owned())),
        ]);
    }
    for err in [
        ServerError::TaskNotFound("t".into()),
        ServerError::Internal("broke".into()),
    ] {
        let _ = ServerSpan::open(&handler, RpcSystem::HttpJson, "GetTask", None)
            .run(async { Err::<(), _>(err) })
            .await;
    }
    want.extend([(get_task(), None), (get_task(), Some("500".to_owned()))]);
    let _ = ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", None)
        .run(async { Err::<(), _>(ServerError::TaskNotFound("t".into())) })
        .await;
    want.push((get_task(), Some("-32001".to_owned())));
    let dropped = ServerSpan::open(&handler, RpcSystem::Grpc, "GetTask", None)
        .run(std::future::pending::<Result<(), ServerError>>());
    assert!(
        tokio::time::timeout(Duration::from_millis(10), dropped)
            .await
            .is_err()
    );
    want.push((get_task(), Some("cancelled".to_owned())));
    assert_eq!(*seen.lock().unwrap(), want);
}

/// On the span: a `NOT_FOUND` keeps `rpc.status_code` and sets neither
/// `error.type` nor the `Error` status.
#[cfg(all(feature = "tracing", feature = "grpc"))]
#[test]
fn a_not_found_span_has_its_status_code_and_no_error() {
    use tracing_subscriber::layer::SubscriberExt as _;
    let fields = Fields::default();
    let handler = handler(&Seen::default());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(fields.clone()), || {
        let _ = runtime.block_on(
            ServerSpan::open(&handler, RpcSystem::Grpc, "GetTask", None)
                .run(async { Err::<(), _>(ServerError::TaskNotFound("t".into())) }),
        );
    });
    let fields = fields.0.lock().unwrap().clone();
    assert_eq!(
        fields.get("rpc.status_code").map(String::as_str),
        Some("NOT_FOUND")
    );
    assert!(!fields.contains_key("error.type"), "{fields:?}");
    assert!(!fields.contains_key("otel.status_code"), "{fields:?}");
}

/// Counts the spans `tracing` is asked to create, by name.
#[cfg(feature = "tracing")]
#[derive(Clone, Default)]
struct Created(Arc<std::sync::Mutex<Vec<String>>>);

#[cfg(feature = "tracing")]
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Created {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        self.0
            .lock()
            .unwrap()
            .push(attrs.metadata().name().to_owned());
    }
}

/// The spans one call, and the task it spawns, create under `policy`.
#[cfg(feature = "tracing")]
fn spans_created_under(policy: InboundTracePolicy) -> Vec<String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    let created = Created::default();
    let subscriber = tracing_subscriber::registry().with(created.clone());
    let seen = Seen::default();
    let handler = crate::RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_metrics(Recording(Arc::clone(&seen)))
        .with_inbound_trace_policy(policy)
        .build()
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tracing::subscriber::with_default(subscriber, || {
        runtime.block_on(async {
            ServerSpan::open(&handler, RpcSystem::JsonRpc, "SendMessage", None)
                .run_with(
                    async {
                        // Spawned from inside the call, as the executor
                        // is; and spawning again from inside that task.
                        tokio::spawn(
                            ExecutorSpan::open(
                                "t",
                                "c",
                                None,
                                SpanSettings::default(),
                                &a2a_protocol_types::message::Message::user("m", vec![]),
                            )
                            .instrument(in_child_span("a2a.process_events", async {})),
                        )
                        .await
                        .map_err(|e| ServerError::Internal(e.to_string()))
                    },
                    // Spawned while the response is built, as the SSE
                    // writer is.
                    |_| tokio::spawn(in_child_span("a2a.sse", async {})),
                )
                .await
                .await
                .unwrap();
        });
    });
    created.0.lock().unwrap().clone()
}

/// Under `Drop`, nothing is recorded — not the call's span, and not the
/// spans of the work it spawns, however deep (the policy's promise). The
/// same call under `Continue` records all four, so the count is not zero
/// for want of a subscriber.
#[cfg(feature = "tracing")]
#[test]
fn a_dropped_trace_records_no_span_for_the_call_or_its_tasks() {
    assert_eq!(
        spans_created_under(InboundTracePolicy::Continue).len(),
        4,
        "the control: a traced call and its three tasks"
    );
    assert_eq!(
        spans_created_under(InboundTracePolicy::Drop),
        Vec::<String>::new()
    );
}

#[cfg(feature = "tracing")]
#[test]
fn a_long_method_original_is_cut_on_a_character_boundary() {
    assert_eq!(truncated("abc", 8), "abc");
    let s = "é".repeat(100); // 200 bytes
    let t = truncated(&s, 5);
    assert_eq!(t, "éé");
}

/// The fields `tracing` is given for the spans it creates, by field name,
/// including those recorded after creation.
#[cfg(feature = "tracing")]
#[derive(Clone, Default)]
pub(super) struct Fields(
    pub(super) Arc<std::sync::Mutex<std::collections::BTreeMap<String, String>>>,
);

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

/// The fields of the span a JSON-RPC call to `method` opens.
#[cfg(feature = "tracing")]
fn span_fields(method: &str) -> std::collections::BTreeMap<String, String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    let fields = Fields::default();
    let subscriber = tracing_subscriber::registry().with(fields.clone());
    let handler = handler(&Seen::default());
    tracing::subscriber::with_default(subscriber, || {
        drop(ServerSpan::open(&handler, RpcSystem::JsonRpc, method, None));
    });
    let recorded = fields.0.lock().unwrap();
    recorded.clone()
}

/// A served method names the span and leaves `rpc.method_original` unset; an
/// unknown one names it by the system, as the conventions ask for `_OTHER`,
/// and keeps what the peer sent. The incremental mutation gate found both
/// comparisons unguarded: flipping either survived.
#[cfg(feature = "tracing")]
#[test]
fn the_span_is_named_by_its_method_or_by_its_system_for_other() {
    let served = span_fields("GetTask");
    assert_eq!(
        served.get("otel.name").map(String::as_str),
        Some("lf.a2a.v1.A2AService/GetTask")
    );
    assert_eq!(served.get("rpc.method_original"), None);

    let unknown = span_fields("NoSuchMethod");
    assert_eq!(
        unknown.get("otel.name").map(String::as_str),
        Some(RpcSystem::JsonRpc.name())
    );
    assert_eq!(unknown.get("rpc.method").map(String::as_str), Some(OTHER));
    assert_eq!(
        unknown.get("rpc.method_original").map(String::as_str),
        Some("NoSuchMethod")
    );
}

/// Under `Continue` the caller's `traceparent` is the span's parent, so the
/// call joins the caller's trace; under `Restart` it does not. The mutation
/// gate found the policy comparison unguarded in the server crate's own
/// tests: flipping it survived.
#[cfg(feature = "otel")]
#[test]
fn only_continue_makes_the_callers_traceparent_the_spans_parent() {
    use opentelemetry::trace::TracerProvider as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    const TRACE: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("rpc_span")));
    let mut headers = HashMap::new();
    headers.insert(
        "traceparent".to_owned(),
        format!("00-{TRACE}-00f067aa0ba902b7-01"),
    );
    let trace_under = |policy: InboundTracePolicy| -> String {
        struct Idle;
        crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
        let handler = crate::RequestHandlerBuilder::new(Idle)
            .without_agent_card()
            .with_inbound_trace_policy(policy)
            .build()
            .unwrap();
        let call = ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", Some(&headers));
        let (trace, _, _) = call
            .span
            .in_scope(current_recorded_span)
            .expect("the span is recorded");
        format!("{:032x}", u128::from_be_bytes(trace))
    };
    tracing::subscriber::with_default(subscriber, || {
        assert_eq!(trace_under(InboundTracePolicy::Continue), TRACE);
        assert_ne!(trace_under(InboundTracePolicy::Restart), TRACE);
    });
}

/// `Debug` names the call, and nothing a log line would regret.
#[test]
fn a_server_span_debugs_as_its_system_and_method() {
    let handler = handler(&Seen::default());
    let call = ServerSpan::open(&handler, RpcSystem::JsonRpc, "GetTask", None);
    assert_eq!(
        format!("{call:?}"),
        r#"ServerSpan { system: JsonRpc, method: "lf.a2a.v1.A2AService/GetTask", .. }"#
    );
}
