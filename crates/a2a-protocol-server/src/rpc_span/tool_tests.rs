// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the tool loop's spans record, alone and inside a served task.

#![cfg(feature = "tracing")]

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::{GenAiMeter, GenAiSpan, Reply};
use crate::tool_loop::{
    ChatRequest, ModelError, ModelTurn, TokenUsage, Tool, ToolCall, ToolError, ToolInvocation,
    ToolLoop, ToolLoopExecutor, ToolModel, ToolSet,
};

/// One span as recorded: its fields and its parent's index.
#[derive(Debug, Clone, Default)]
struct Recorded {
    fields: BTreeMap<String, String>,
    parent: Option<u64>,
    id: u64,
}

impl Recorded {
    fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

/// Every span opened, in order, with its parent.
#[derive(Clone, Default)]
struct Spans(Arc<Mutex<Vec<Recorded>>>);

struct Collect<'a>(&'a mut BTreeMap<String, String>);

impl tracing::field::Visit for Collect<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_owned(), value.to_owned());
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().to_owned(), format!("{value:?}"));
    }
}

impl<S> tracing_subscriber::Layer<S> for Spans
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut fields = BTreeMap::new();
        attrs.record(&mut Collect(&mut fields));
        let parent = ctx
            .span(id)
            .and_then(|s| s.parent())
            .map(|p| p.id().into_u64());
        self.0.lock().unwrap().push(Recorded {
            fields,
            parent,
            id: id.into_u64(),
        });
    }
    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut spans = self.0.lock().unwrap();
        if let Some(span) = spans.iter_mut().rev().find(|s| s.id == id.into_u64()) {
            values.record(&mut Collect(&mut span.fields));
        }
    }
}

/// Runs `body` with every span recorded, and returns them.
///
/// The loop's own tests hit these callsites in parallel with no subscriber;
/// the interest cache is rebuilt once this one is installed, so a callsite
/// one of them cached as never enabled is asked again (commit `1aa09d0`).
fn recorded(body: impl FnOnce()) -> Vec<Recorded> {
    use tracing_subscriber::layer::SubscriberExt as _;
    let spans = Spans::default();
    tracing::subscriber::with_default(tracing_subscriber::registry().with(spans.clone()), || {
        tracing::callsite::rebuild_interest_cache();
        body();
    });
    spans.0.lock().unwrap().clone()
}

#[test]
fn a_turn_is_a_chat_client_span_with_its_usage() {
    let spans = recorded(|| {
        let span = GenAiSpan::chat(
            "openai",
            "qwen3:1.7b",
            Some(("127.0.0.1", 8080)),
            &GenAiMeter::default(),
        );
        span.answered(Reply {
            input_tokens: Some(120),
            output_tokens: Some(7),
            response_model: Some("qwen3-1.7b-q4"),
            response_id: Some("chatcmpl-1"),
            finish_reason: Some("stop"),
        });
    });
    let chat = &spans[0];
    assert_eq!(chat.get("otel.name"), Some("chat qwen3:1.7b"));
    assert_eq!(chat.get("otel.kind"), Some("client"));
    assert_eq!(chat.get("gen_ai.operation.name"), Some("chat"));
    assert_eq!(chat.get("gen_ai.provider.name"), Some("openai"));
    assert_eq!(chat.get("gen_ai.request.model"), Some("qwen3:1.7b"));
    assert_eq!(chat.get("gen_ai.response.model"), Some("qwen3-1.7b-q4"));
    assert_eq!(chat.get("gen_ai.response.id"), Some("chatcmpl-1"));
    assert_eq!(chat.get("server.address"), Some("127.0.0.1"));
    assert_eq!(chat.get("server.port"), Some("8080"));
    assert_eq!(chat.get("gen_ai.usage.input_tokens"), Some("120"));
    assert_eq!(chat.get("gen_ai.usage.output_tokens"), Some("7"));
    assert_eq!(chat.get("error.type"), None);
    assert_eq!(chat.get("otel.status_code"), None);
}

#[test]
fn usage_the_provider_did_not_report_is_not_recorded() {
    let spans = recorded(|| {
        GenAiSpan::chat("openai", "m", None, &GenAiMeter::default()).answered(Reply::default());
    });
    assert_eq!(spans[0].get("gen_ai.usage.input_tokens"), None);
    assert_eq!(spans[0].get("gen_ai.usage.output_tokens"), None);
    assert_eq!(spans[0].get("gen_ai.response.model"), None);
    assert_eq!(spans[0].get("gen_ai.response.id"), None);
    assert_eq!(spans[0].get("server.address"), None);
    assert_eq!(spans[0].get("error.type"), None);
}

#[test]
fn a_failed_turn_or_call_records_its_error_type_and_status() {
    let spans = recorded(|| {
        GenAiSpan::chat("openai", "m", None, &GenAiMeter::default()).failed("500");
        GenAiSpan::tool("t", true, "c1", None, &GenAiMeter::default()).failed("tool_error");
    });
    assert_eq!(spans[0].get("error.type"), Some("500"));
    assert_eq!(spans[0].get("otel.status_code"), Some("ERROR"));
    assert_eq!(spans[1].get("error.type"), Some("tool_error"));
    assert_eq!(spans[1].get("otel.status_code"), Some("ERROR"));
}

#[test]
fn a_call_is_an_execute_tool_span_naming_the_tool_and_call() {
    let spans = recorded(|| {
        GenAiSpan::tool(
            "service_status",
            true,
            "call-7",
            Some("Look up a service"),
            &GenAiMeter::default(),
        )
        .succeeded();
    });
    let tool = &spans[0];
    assert_eq!(tool.get("otel.name"), Some("execute_tool service_status"));
    assert_eq!(tool.get("otel.kind"), None, "INTERNAL is the default kind");
    assert_eq!(tool.get("gen_ai.operation.name"), Some("execute_tool"));
    assert_eq!(tool.get("gen_ai.tool.name"), Some("service_status"));
    assert_eq!(tool.get("gen_ai.tool.call.id"), Some("call-7"));
    assert_eq!(tool.get("gen_ai.tool.type"), Some("function"));
    assert_eq!(
        tool.get("gen_ai.tool.description"),
        Some("Look up a service")
    );
}

/// Asks for `service_status` (checkout), then for an unknown tool, then
/// answers.
struct TwoCalls(AtomicUsize);
impl ToolModel for TwoCalls {
    fn model_name(&self) -> &'static str {
        "scripted"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let turn = match self.0.fetch_add(1, Ordering::SeqCst) {
            0 => ModelTurn::new(
                "",
                vec![ToolCall::new(
                    "c1",
                    "service_status",
                    r#"{"service":"SECRET-ARG"}"#,
                )],
            )
            .with_usage(TokenUsage::new(Some(50), Some(9))),
            1 => ModelTurn::new("", vec![ToolCall::new("c2", "nope", "{}")]),
            _ => ModelTurn::new("SECRET-ANSWER", Vec::new()),
        };
        Box::pin(async move { Ok(turn) })
    }
}

struct Status;
impl Tool for Status {
    fn name(&self) -> &'static str {
        "service_status"
    }
    fn description(&self) -> &'static str {
        "Look up a service"
    }
    fn parameters(&self) -> Value {
        json!({"type": "object"})
    }
    fn call<'a>(
        &'a self,
        _call: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        Box::pin(async { Ok("SECRET-RESULT".to_owned()) })
    }
}

/// Served through the handler, each turn and call is a child of the
/// executor's `invoke_agent` span, and no prompt, argument, result or answer
/// is recorded on any of them.
#[test]
fn inside_a_task_the_spans_nest_under_invoke_agent_and_carry_no_content() {
    let tools = ToolSet::new().with(Status).unwrap();
    let executor = ToolLoopExecutor::new(ToolLoop::new(TwoCalls(AtomicUsize::new(0)), tools));
    let handler = crate::RequestHandlerBuilder::new(executor)
        .without_agent_card()
        .build()
        .unwrap();
    let spans = recorded(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let params = a2a_protocol_types::params::MessageSendParams::new(
                a2a_protocol_types::message::Message::user_text("m1", "SECRET-PROMPT"),
            );
            handler.on_send_message(params, false, None).await.unwrap();
        });
    });
    let agent = spans
        .iter()
        .find(|s| s.get("gen_ai.operation.name") == Some("invoke_agent"))
        .expect("the executor's span");
    let of = |op: &str| -> Vec<&Recorded> {
        spans
            .iter()
            .filter(|s| s.get("gen_ai.operation.name") == Some(op))
            .collect()
    };
    let chats = of("chat");
    let tools = of("execute_tool");
    assert_eq!(chats.len(), 3);
    assert_eq!(tools.len(), 2);
    for span in chats.iter().chain(&tools) {
        assert_eq!(span.parent, Some(agent.id), "{span:?}");
    }
    assert_eq!(chats[0].get("otel.name"), Some("chat scripted"));
    assert_eq!(chats[0].get("gen_ai.usage.input_tokens"), Some("50"));
    assert_eq!(tools[0].get("gen_ai.tool.call.id"), Some("c1"));
    assert_eq!(tools[0].get("error.type"), None);
    assert_eq!(tools[1].get("gen_ai.tool.name"), Some("nope"));
    assert_eq!(tools[1].get("error.type"), Some("unknown_tool"));
    for span in chats.iter().chain(&tools) {
        for value in span.fields.values() {
            assert!(!value.contains("SECRET"), "content recorded: {span:?}");
        }
    }
}

// `cfg(test)` again, though this file is test-only: it is how
// `scripts/check_panic_paths.py` knows a `#[path]` module is a test's.
#[cfg(feature = "otel")]
#[cfg(test)]
#[path = "tool_metrics_tests.rs"]
mod metrics;

/// A name the model invented is not a span name or a metric series of its
/// own (audit C-02): `_OTHER`, with the name kept in `gen_ai.tool.name`.
#[test]
fn an_unknown_tool_is_named_other_in_the_span_name() {
    let spans = recorded(|| {
        GenAiSpan::tool(
            "ignore-previous-instructions",
            false,
            "c9",
            None,
            &GenAiMeter::default(),
        )
        .failed("unknown_tool");
    });
    let tool = &spans[0];
    assert_eq!(tool.get("otel.name"), Some("execute_tool _OTHER"));
    assert_eq!(
        tool.get("gen_ai.tool.name"),
        Some("ignore-previous-instructions")
    );
}
