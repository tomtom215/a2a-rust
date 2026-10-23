// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tool loop's spans and metrics as exported: a turn's `chat` span with
//! what the provider reported, and the `GenAI` metrics, decoded from what a
//! collector received.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_server::otel::{OtlpProtocol, Telemetry};
use a2a_protocol_server::tool_loop::{
    ChatRequest, ModelError, ModelTurn, TokenUsage, ToolLoop, ToolModel, ToolSet,
};
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::trace::v1::Span;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::collector::{Collector, attr};

/// Answers at once, reporting everything a provider can.
struct Reporting;

impl ToolModel for Reporting {
    fn model_name(&self) -> &'static str {
        "qwen3:1.7b"
    }
    fn provider_name(&self) -> &'static str {
        "openai"
    }
    fn server(&self) -> Option<(&str, u16)> {
        Some(("models.internal", 8443))
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async {
            Ok(ModelTurn::new("done", Vec::new())
                .with_usage(TokenUsage::new(Some(31), Some(4)))
                .with_response_model("qwen3-1.7b-q4_k_m")
                .with_response_id("chatcmpl-42")
                .with_finish_reason("stop"))
        })
    }
}

fn string_array(span: &Span, key: &str) -> Vec<String> {
    let value = span
        .attributes
        .iter()
        .find(|kv| kv.key == key)
        .and_then(|kv| kv.value.as_ref()?.value.as_ref());
    let Some(Value::ArrayValue(array)) = value else {
        panic!("{key} is not a string array: {value:?}");
    };
    array
        .values
        .iter()
        .filter_map(|v| match v.value.as_ref()? {
            Value::StringValue(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// One run, its spans through this telemetry's tracer and its metrics
/// through this telemetry's meter, exported over OTLP/HTTP.
#[test]
fn a_turn_exports_its_chat_span_and_the_genai_metrics() {
    let collector = Collector::http();
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_otlp_endpoint(collector.http_base())
        .with_otlp_protocol(OtlpProtocol::HttpProtobuf)
        .with_logs(false)
        .build()
        .expect("build");
    let meter = telemetry
        .meter_provider()
        .expect("metrics are on")
        .meter("a2a.server");
    let tool_loop = ToolLoop::new(Reporting, ToolSet::new())
        .with_agent_name("reporter")
        .with_meter(meter);
    let subscriber = tracing_subscriber::registry().with(telemetry.layer());
    tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let answer = rt
            .block_on(tool_loop.run("hi", &CancellationToken::new()))
            .expect("answers");
        assert_eq!(answer.text, "done");
    });
    telemetry.shutdown().expect("flushed");

    let spans = collector.spans();
    let chat = spans
        .iter()
        .map(|(_, span)| span)
        .find(|span| span.name == "chat qwen3:1.7b")
        .unwrap_or_else(|| panic!("no chat span in {spans:?}"));
    for (key, value) in [
        ("gen_ai.operation.name", "chat"),
        ("gen_ai.provider.name", "openai"),
        ("gen_ai.request.model", "qwen3:1.7b"),
        ("gen_ai.response.model", "qwen3-1.7b-q4_k_m"),
        ("gen_ai.response.id", "chatcmpl-42"),
        ("gen_ai.usage.input_tokens", "31"),
        ("gen_ai.usage.output_tokens", "4"),
        ("server.address", "models.internal"),
        ("server.port", "8443"),
    ] {
        assert_eq!(attr(&chat.attributes, key).as_deref(), Some(value), "{key}");
    }
    assert_eq!(
        string_array(chat, "gen_ai.response.finish_reasons"),
        ["stop"]
    );
    assert_eq!(attr(&chat.attributes, "error.type"), None);

    let names = collector.metric_names();
    for name in [
        "gen_ai.client.inference.duration",
        "gen_ai.client.inference.usage.input_tokens",
        "gen_ai.client.inference.usage.output_tokens",
        "gen_ai.client.inference.operation.input_tokens",
        "gen_ai.client.inference.operation.output_tokens",
        "gen_ai.invoke_agent.duration",
        "gen_ai.invoke_agent.inference_calls",
        "gen_ai.invoke_agent.tool_calls",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} not in {names:?}");
    }
}
