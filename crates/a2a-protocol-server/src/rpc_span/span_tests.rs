// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the call's span and the executor's span record (ADR 0013,
//! 2026-10-06): the draft A2A attributes and the agent span's output.
//!
//! The SDK's observability gate covers these end to end, but the incremental
//! mutation gate runs this crate's tests only, and with only the gate's 16
//! mutants in `a2a.rs` and `agent.rs` survived.

#![cfg(feature = "tracing")]

use std::collections::BTreeMap;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use super::tests::Fields;
use super::*;
use crate::error::ServerError;

/// The fields the call's span holds after `record` runs inside it.
fn call_fields(record: impl FnOnce()) -> BTreeMap<String, String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    let handler = crate::RequestHandlerBuilder::new(Idle).build().unwrap();
    let fields = Fields::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(fields.clone()), || {
        runtime
            .block_on(
                ServerSpan::open(&handler, RpcSystem::JsonRpc, "SendMessage", None).run(async {
                    record();
                    Ok::<(), ServerError>(())
                }),
            )
            .unwrap();
    });
    let recorded = fields.0.lock().unwrap();
    recorded.clone()
}

#[test]
fn the_tenant_is_recorded_when_set_and_not_when_empty() {
    let set = call_fields(|| a2a::record_tenant(Some("acme")));
    assert_eq!(set.get("a2a.tenant").map(String::as_str), Some("acme"));
    let empty = call_fields(|| a2a::record_tenant(Some("")));
    assert!(!empty.contains_key("a2a.tenant"), "{empty:?}");
}

#[test]
fn the_message_task_context_and_state_are_recorded() {
    let fields = call_fields(|| {
        a2a::record_message(&Message::user("m1", vec![Part::text("hi")]));
        a2a::record_task("t1", Some("c1"));
        a2a::record_task_state(TaskState::Working);
    });
    let field = |k: &str| fields.get(k).map(String::as_str);
    assert_eq!(field("a2a.message.id"), Some("m1"));
    assert_eq!(field("a2a.task.id"), Some("t1"));
    assert_eq!(field("gen_ai.conversation.id"), Some("c1"));
    assert_eq!(field("a2a.task.state"), Some("TASK_STATE_WORKING"));
}

#[test]
fn a_task_response_records_its_id_context_and_state() {
    let task: a2a_protocol_types::task::Task = serde_json::from_value(serde_json::json!({
        "id": "t2",
        "contextId": "c2",
        "status": {"state": "TASK_STATE_FAILED"}
    }))
    .unwrap();
    let fields = call_fields(|| a2a::record_task_response(&task));
    let field = |k: &str| fields.get(k).map(String::as_str);
    assert_eq!(field("a2a.task.id"), Some("t2"));
    assert_eq!(field("gen_ai.conversation.id"), Some("c2"));
    assert_eq!(field("a2a.task.state"), Some("TASK_STATE_FAILED"));
}

fn status(state: TaskState, message: Option<Message>) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t"),
        context_id: ContextId::new("c"),
        status: TaskStatus {
            state,
            message,
            timestamp: None,
        },
        metadata: None,
    })
}

/// The executor span's fields after `events`, with content capture on.
fn executor_fields(events: &[StreamResponse]) -> BTreeMap<String, String> {
    use tracing_subscriber::layer::SubscriberExt as _;
    let fields = Fields::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(fields.clone()), || {
        let span = ExecutorSpan::open(
            "t",
            "c",
            None,
            SpanSettings {
                agent: true,
                content: true,
            },
            &Message::user("m", vec![Part::text("question")]),
        );
        let recorder = span.recorder().expect("a span that records");
        for event in events {
            recorder.observe(event);
        }
    });
    let recorded = fields.0.lock().unwrap();
    recorded.clone()
}

/// Only the agent's words are its output: a status message from the user
/// role is not.
#[test]
fn only_agent_status_messages_are_output() {
    let fields = executor_fields(&[
        status(
            TaskState::Working,
            Some(Message::user("u", vec![Part::text("from the user")])),
        ),
        status(
            TaskState::Completed,
            Some(Message::agent("a", vec![Part::text("the answer")])),
        ),
    ]);
    let output = &fields["gen_ai.output.messages"];
    assert!(output.contains("the answer"), "{output}");
    assert!(!output.contains("from the user"), "{output}");
    assert_eq!(
        fields.get("a2a.task.state").map(String::as_str),
        Some("TASK_STATE_COMPLETED")
    );
}

/// A direct reply ends the turn and is the output; it has no task state.
#[test]
fn a_direct_reply_is_the_output() {
    let fields = executor_fields(&[StreamResponse::Message(Message::agent(
        "a",
        vec![Part::text("a direct reply")],
    ))]);
    assert!(
        fields["gen_ai.output.messages"].contains("a direct reply"),
        "{fields:?}"
    );
    assert!(!fields.contains_key("a2a.task.state"), "{fields:?}");
}

/// A turn that ends with nothing said records its state and no output.
#[test]
fn a_turn_with_nothing_said_records_no_output() {
    let fields = executor_fields(&[status(TaskState::Completed, None)]);
    assert!(!fields.contains_key("gen_ai.output.messages"), "{fields:?}");
    assert_eq!(
        fields.get("a2a.task.state").map(String::as_str),
        Some("TASK_STATE_COMPLETED")
    );
}

/// The output budget is 64 KiB: two thousand bytes fit whole.
#[test]
fn output_well_under_the_limit_is_kept_whole() {
    assert_eq!(agent::CONTENT_LIMIT, 65_536);
    let long = "x".repeat(2_000);
    let fields = executor_fields(&[status(
        TaskState::Completed,
        Some(Message::agent("a", vec![Part::text(long.clone())])),
    )]);
    let output = &fields["gen_ai.output.messages"];
    assert!(output.contains(&long), "cut: {} bytes", output.len());
    assert!(!output.contains("truncated"), "{output}");
}

/// `referenceTaskIds` is a string array, which a `tracing` field cannot
/// hold, so it goes to the OpenTelemetry span directly: seen only by an
/// exporter.
#[cfg(feature = "otel")]
#[test]
fn reference_task_ids_are_exported_as_a_string_array() {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
    use tracing_subscriber::layer::SubscriberExt as _;

    #[derive(Clone, Debug, Default)]
    struct Keep(Arc<std::sync::Mutex<Vec<SpanData>>>);
    impl SpanExporter for Keep {
        async fn export(&self, batch: Vec<SpanData>) -> opentelemetry_sdk::error::OTelSdkResult {
            self.0.lock().unwrap().extend(batch);
            Ok(())
        }
    }

    struct Idle;
    crate::agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });
    let handler = crate::RequestHandlerBuilder::new(Idle).build().unwrap();
    let keep = Keep::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(keep.clone())
        .build();
    let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("span-tests"));
    let mut message = Message::user("m1", vec![Part::text("hi")]);
    message.reference_task_ids = Some(vec![TaskId::new("r1"), TaskId::new("r2")]);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(layer), || {
        runtime
            .block_on(
                ServerSpan::open(&handler, RpcSystem::JsonRpc, "SendMessage", None).run(async {
                    a2a::record_message(&message);
                    Ok::<(), ServerError>(())
                }),
            )
            .unwrap();
    });
    let spans = std::mem::take(&mut *keep.0.lock().unwrap());
    let value = spans
        .iter()
        .flat_map(|s| s.attributes.iter())
        .find(|kv| kv.key.as_str() == "a2a.message.reference_task_ids")
        .map(|kv| kv.value.clone());
    let names: Vec<String> = spans.iter().map(|s| s.name.to_string()).collect();
    let expected: Vec<opentelemetry::StringValue> = vec!["r1".into(), "r2".into()];
    assert_eq!(
        value,
        Some(opentelemetry::Value::Array(expected.into())),
        "spans: {names:?}"
    );
}
