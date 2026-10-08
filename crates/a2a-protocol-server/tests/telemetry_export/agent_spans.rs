// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The executor's span as exported: the GenAI `invoke_agent` conventions, and
//! the opt-in message content, decoded from what a collector received.

use std::sync::Arc;

use a2a_protocol_server::otel::{OtlpProtocol, Telemetry};
use a2a_protocol_server::{EventEmitter, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::agent_card::{AgentCard, AgentInterface};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::TaskState;
use opentelemetry_proto::tonic::trace::v1::Span;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::collector::{Collector, attr};

struct Echo;

agent_executor!(Echo, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    let text = ctx.message.text().unwrap_or_default().to_owned();
    emit.artifact(
        "echo",
        vec![Part::text(format!("echo: {text}"))],
        None,
        Some(true),
    )
    .await?;
    emit.status(TaskState::Completed).await
});

/// Sends one message carrying `text` through a handler built by `build`, and
/// returns the executor's exported span.
fn run_once(
    text: &str,
    build: impl FnOnce(RequestHandlerBuilder) -> RequestHandlerBuilder,
) -> Span {
    let collector = Collector::http();
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_otlp_endpoint(collector.http_base())
        .with_otlp_protocol(OtlpProtocol::HttpProtobuf)
        .with_metrics(false)
        .with_logs(false)
        .build()
        .expect("build");
    let card = AgentCard::new(
        "echo-agent",
        "2.0.1",
        AgentInterface::jsonrpc("http://127.0.0.1"),
    )
    .with_description("Echoes what it is sent");
    let handler = Arc::new(
        build(RequestHandlerBuilder::new(Echo).with_agent_card(card))
            .build()
            .expect("handler"),
    );
    let params = MessageSendParams::new(
        Message::user("m-1", vec![Part::text(text)]).with_context_id("ctx-agent-spans"),
    );
    let subscriber = tracing_subscriber::registry().with(telemetry.layer());
    tracing::subscriber::with_default(subscriber, || {
        // `current_thread`: the executor is spawned onto this thread, so the
        // thread-local subscriber records it too.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let _ = handler
                .on_send_message(params, false, None)
                .await
                .expect("send");
            // Let the executor's task finish and its span close.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        });
    });
    telemetry.shutdown().expect("flushed");
    collector
        .spans()
        .into_iter()
        .map(|(_, s)| s)
        .find(|s| attr(&s.attributes, "a2a.task.id").is_some() && s.kind == 1)
        .unwrap_or_else(|| {
            panic!(
                "no executor span among {:?}",
                collector
                    .spans()
                    .iter()
                    .map(|(_, s)| s.name.clone())
                    .collect::<Vec<_>>()
            )
        })
}

fn json_attr(span: &Span, key: &str) -> serde_json::Value {
    let raw =
        attr(&span.attributes, key).unwrap_or_else(|| panic!("no `{key}` on `{}`", span.name));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("`{key}` is not JSON ({e}): {raw}"))
}

/// By default: an `invoke_agent` span named for the card, with the agent and
/// the A2A context as the conversation — and no message content.
#[test]
fn the_executor_span_is_invoke_agent_without_content_by_default() {
    let span = run_once("Ada", |b| b);
    assert_eq!(span.name, "invoke_agent echo-agent");
    for (key, want) in [
        ("gen_ai.operation.name", "invoke_agent"),
        ("gen_ai.agent.name", "echo-agent"),
        ("gen_ai.agent.description", "Echoes what it is sent"),
        ("gen_ai.agent.version", "2.0.1"),
        ("gen_ai.conversation.id", "ctx-agent-spans"),
        ("a2a.context.id", "ctx-agent-spans"),
        ("a2a.task.state", "TASK_STATE_COMPLETED"),
    ] {
        assert_eq!(attr(&span.attributes, key).as_deref(), Some(want), "{key}");
    }
    assert!(attr(&span.attributes, "gen_ai.input.messages").is_none());
    assert!(attr(&span.attributes, "gen_ai.output.messages").is_none());
}

/// Opted in: the request's message and the agent's artifact, in the
/// conventions' message format, with a finish reason for the completed task.
#[test]
fn content_capture_records_input_and_output_messages() {
    let span = run_once("Ada", |b| b.with_span_content_capture(true));
    assert_eq!(
        json_attr(&span, "gen_ai.input.messages"),
        serde_json::json!([{ "role": "user", "parts": [{ "type": "text", "content": "Ada" }] }])
    );
    assert_eq!(
        json_attr(&span, "gen_ai.output.messages"),
        serde_json::json!([{
            "role": "assistant",
            "parts": [{ "type": "text", "content": "echo: Ada" }],
            "finish_reason": "stop",
        }])
    );
}

/// A message past the limit is cut, marked, and the attribute stays near
/// the limit rather than the message's size.
#[test]
fn captured_content_is_bounded() {
    let long = "x".repeat(200 * 1024);
    let span = run_once(&long, |b| b.with_span_content_capture(true));
    let input = attr(&span.attributes, "gen_ai.input.messages").expect("input");
    assert!(input.len() < 66 * 1024, "{} bytes", input.len());
    assert!(input.contains("truncated"), "the cut is marked");
    let parsed: serde_json::Value = serde_json::from_str(&input).expect("still valid JSON");
    assert_eq!(parsed[0]["role"], "user");
}

/// Conventions off: the span is `a2a.execute`, as before, and claims no
/// GenAI operation.
#[test]
fn agent_conventions_can_be_turned_off() {
    let span = run_once("Ada", |b| b.with_agent_span_conventions(false));
    assert_eq!(span.name, "a2a.execute");
    assert!(attr(&span.attributes, "gen_ai.operation.name").is_none());
    assert!(attr(&span.attributes, "gen_ai.agent.name").is_none());
    assert_eq!(
        attr(&span.attributes, "a2a.task.state").as_deref(),
        Some("TASK_STATE_COMPLETED")
    );
}
