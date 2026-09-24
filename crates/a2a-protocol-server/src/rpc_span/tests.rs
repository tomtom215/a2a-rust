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
                        tokio::spawn(in_executor_span(
                            "t",
                            "c",
                            in_child_span("a2a.process_events", async {}),
                        ))
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
