// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the tool loop's metrics record, read back from a meter of the test's
//! own: per turn, per call, and per run.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;

use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use tokio_util::sync::CancellationToken;

use super::{Status, TwoCalls, recorded};
use crate::otel::test_reader::{collect, recording_meter};
use crate::rpc_span::{GenAiMeter, GenAiSpan, Reply};
use crate::tool_loop::{
    ChatRequest, LoopError, ModelError, ModelTurn, ToolCall, ToolLoop, ToolModel, ToolSet,
};

/// One data point: its attributes, and its count and sum (a counter's value
/// is its sum, with a count of one).
#[derive(Debug)]
struct Point {
    attributes: BTreeMap<String, String>,
    count: u64,
    sum: f64,
    bounds: Vec<f64>,
}

impl Point {
    fn get(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(String::as_str)
    }
}

/// Every point recorded under `name`.
#[allow(clippy::cast_precision_loss)] // Test values far below 2^53.
fn points(rm: &ResourceMetrics, name: &str) -> Vec<Point> {
    let attributes = |kvs: &mut dyn Iterator<Item = &opentelemetry::KeyValue>| {
        kvs.map(|kv| (kv.key.to_string(), kv.value.to_string()))
            .collect::<BTreeMap<_, _>>()
    };
    let mut found = Vec::new();
    for scope in rm.scope_metrics() {
        for metric in scope.metrics().filter(|m| m.name() == name) {
            match metric.data() {
                AggregatedMetrics::F64(MetricData::Histogram(h)) => {
                    found.extend(h.data_points().map(|p| Point {
                        attributes: attributes(&mut p.attributes()),
                        count: p.count(),
                        sum: p.sum(),
                        bounds: p.bounds().collect(),
                    }));
                }
                AggregatedMetrics::U64(MetricData::Histogram(h)) => {
                    found.extend(h.data_points().map(|p| Point {
                        attributes: attributes(&mut p.attributes()),
                        count: p.count(),
                        sum: p.sum() as f64,
                        bounds: p.bounds().collect(),
                    }));
                }
                AggregatedMetrics::U64(MetricData::Sum(s)) => {
                    found.extend(s.data_points().map(|p| Point {
                        attributes: attributes(&mut p.attributes()),
                        count: 1,
                        sum: p.value() as f64,
                        bounds: Vec::new(),
                    }));
                }
                other => panic!("{name} has an unexpected shape: {other:?}"),
            }
        }
    }
    found
}

/// The one point recorded under `name`.
fn only(rm: &ResourceMetrics, name: &str) -> Point {
    let mut found = points(rm, name);
    assert_eq!(found.len(), 1, "{name}: {found:?}");
    found.remove(0)
}

const DURATION_BUCKETS: [f64; 14] = [
    0.01, 0.02, 0.04, 0.08, 0.16, 0.32, 0.64, 1.28, 2.56, 5.12, 10.24, 20.48, 40.96, 81.92,
];

#[test]
fn a_turn_records_its_duration_and_its_tokens_in_both_families() {
    let (meter, reader, _provider) = recording_meter();
    let span = GenAiSpan::chat(
        "openai",
        "qwen3:1.7b",
        Some(("127.0.0.1", 8080)),
        &GenAiMeter::new(meter),
    );
    span.answered(Reply {
        input_tokens: Some(120),
        output_tokens: Some(7),
        response_model: Some("qwen3-1.7b-q4"),
        ..Reply::default()
    });
    drop(span);
    let rm = collect(&reader);

    let duration = only(&rm, "gen_ai.client.inference.duration");
    assert_eq!(duration.count, 1);
    assert_eq!(duration.bounds, DURATION_BUCKETS);
    for (key, value) in [
        ("gen_ai.operation.name", "chat"),
        ("gen_ai.provider.name", "openai"),
        ("gen_ai.request.model", "qwen3:1.7b"),
        ("gen_ai.response.model", "qwen3-1.7b-q4"),
        ("server.address", "127.0.0.1"),
        ("server.port", "8080"),
    ] {
        assert_eq!(duration.get(key), Some(value), "{key}: {duration:?}");
    }
    assert_eq!(duration.get("error.type"), None);

    let input = only(&rm, "gen_ai.client.inference.usage.input_tokens");
    assert_eq!(
        (input.sum, input.get("gen_ai.token.modality")),
        (120.0, Some("text"))
    );
    assert_eq!(input.get("gen_ai.request.model"), Some("qwen3:1.7b"));
    let output = only(&rm, "gen_ai.client.inference.usage.output_tokens");
    assert_eq!(
        (output.sum, output.get("gen_ai.token.modality")),
        (7.0, Some("text"))
    );

    let per_turn = only(&rm, "gen_ai.client.inference.operation.input_tokens");
    assert_eq!((per_turn.count, per_turn.sum), (1, 120.0));
    assert_eq!(
        per_turn.get("gen_ai.token.modality"),
        None,
        "the per-operation histograms are deliberately not broken down by modality"
    );
    assert_eq!(per_turn.bounds.first(), Some(&1.0));
    assert_eq!(per_turn.bounds.last(), Some(&67_108_864.0));
    assert_eq!(
        only(&rm, "gen_ai.client.inference.operation.output_tokens").sum,
        7.0
    );
    assert!(
        points(&rm, "gen_ai.client.operation.duration").is_empty(),
        "SHOULD NOT be reported for inference operations"
    );
}

/// A count of zero is a per-operation observation, but not a usage
/// contribution: "Each measurement MUST be a known, strictly positive
/// modality contribution". A count not reported is neither.
#[test]
fn zero_tokens_are_observed_but_not_counted_and_unreported_ones_are_neither() {
    let (meter, reader, _provider) = recording_meter();
    GenAiSpan::chat("openai", "m", None, &GenAiMeter::new(meter)).answered(Reply {
        input_tokens: Some(0),
        output_tokens: None,
        ..Reply::default()
    });
    let rm = collect(&reader);
    assert_eq!(
        only(&rm, "gen_ai.client.inference.operation.input_tokens").count,
        1
    );
    assert!(points(&rm, "gen_ai.client.inference.usage.input_tokens").is_empty());
    assert!(points(&rm, "gen_ai.client.inference.operation.output_tokens").is_empty());
    assert!(points(&rm, "gen_ai.client.inference.usage.output_tokens").is_empty());
    assert_eq!(
        only(&rm, "gen_ai.client.inference.duration").get("server.address"),
        None
    );
}

#[test]
fn a_failed_turn_or_call_records_its_error_type() {
    let (meter, reader, _provider) = recording_meter();
    let meter = GenAiMeter::new(meter);
    GenAiSpan::chat("openai", "m", None, &meter).failed("503");
    GenAiSpan::tool("t", "c1", None, &meter).failed("tool_error");
    let rm = collect(&reader);
    assert_eq!(
        only(&rm, "gen_ai.client.inference.duration").get("error.type"),
        Some("503")
    );
    let tool = only(&rm, "gen_ai.execute_tool.duration");
    assert_eq!(tool.get("error.type"), Some("tool_error"));
    assert_eq!(tool.get("gen_ai.tool.name"), Some("t"));
    assert_eq!(tool.get("gen_ai.tool.type"), Some("function"));
    assert_eq!(tool.bounds, DURATION_BUCKETS);
}

/// A turn or call dropped before anything ended it — its token fired, or the
/// future running it was dropped — is recorded, as `cancelled`, on its span
/// and its metric; a turn also records the `error` finish reason the
/// conventions ask for when one "was expected but not received".
#[test]
fn an_abandoned_turn_or_call_is_recorded_as_cancelled() {
    let (meter, reader, _provider) = recording_meter();
    let meter = GenAiMeter::new(meter);
    let spans = recorded(|| {
        drop(GenAiSpan::chat("openai", "m", None, &meter));
        drop(GenAiSpan::tool("t", "c1", None, &meter));
    });
    for span in &spans {
        assert_eq!(span.get("error.type"), Some("cancelled"), "{span:?}");
        assert_eq!(span.get("otel.status_code"), Some("ERROR"), "{span:?}");
    }
    let rm = collect(&reader);
    for name in [
        "gen_ai.client.inference.duration",
        "gen_ai.execute_tool.duration",
    ] {
        assert_eq!(
            only(&rm, name).get("error.type"),
            Some("cancelled"),
            "{name}"
        );
    }
}

/// `TwoCalls`: three turns, one call that runs and one to an unknown tool.
#[tokio::test]
async fn a_run_records_its_duration_and_how_many_turns_and_calls_it_took() {
    let (meter, reader, _provider) = recording_meter();
    let tools = ToolSet::new().with(Status).unwrap();
    let tool_loop = ToolLoop::new(TwoCalls(AtomicUsize::new(0)), tools)
        .with_agent_name("status-agent")
        .with_meter(meter);
    tool_loop
        .run("hi", &CancellationToken::new())
        .await
        .expect("answers");
    let rm = collect(&reader);

    let run = only(&rm, "gen_ai.invoke_agent.duration");
    assert_eq!(run.get("gen_ai.agent.name"), Some("status-agent"));
    assert_eq!(run.get("gen_ai.request.model"), Some("scripted"));
    assert_eq!(run.get("error.type"), None);
    assert_eq!(run.bounds.first(), Some(&0.1));
    let turns = only(&rm, "gen_ai.invoke_agent.inference_calls");
    assert_eq!((turns.count, turns.sum), (1, 3.0));
    assert_eq!(turns.get("gen_ai.agent.name"), Some("status-agent"));
    assert_eq!(turns.get("gen_ai.request.model"), None);
    assert_eq!(turns.bounds, [1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0]);
    assert_eq!(only(&rm, "gen_ai.invoke_agent.tool_calls").sum, 2.0);

    assert_eq!(
        only(&rm, "gen_ai.client.inference.duration").count,
        3,
        "three turns, one series"
    );
    let calls = points(&rm, "gen_ai.execute_tool.duration");
    let by_tool = |name: &str| {
        calls
            .iter()
            .find(|p| p.get("gen_ai.tool.name") == Some(name))
    };
    assert_eq!(by_tool("service_status").unwrap().get("error.type"), None);
    assert_eq!(
        by_tool("nope").unwrap().get("error.type"),
        Some("unknown_tool")
    );
}

/// Always asks for a call; never answers.
struct Insists;
impl ToolModel for Insists {
    fn model_name(&self) -> &'static str {
        "insists"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async {
            Ok(ModelTurn::new(
                "",
                vec![ToolCall::new("c", "service_status", "{}")],
            ))
        })
    }
}

#[tokio::test]
async fn a_run_that_fails_records_why() {
    let (meter, reader, _provider) = recording_meter();
    let tools = ToolSet::new().with(Status).unwrap();
    let tool_loop = ToolLoop::new(Insists, tools)
        .with_max_turns(2)
        .with_meter(meter);
    let result = tool_loop.run("hi", &CancellationToken::new()).await;
    assert!(matches!(result, Err(LoopError::TurnLimit(2))), "{result:?}");
    let rm = collect(&reader);
    let run = only(&rm, "gen_ai.invoke_agent.duration");
    assert_eq!(run.get("error.type"), Some("turn_limit"));
    assert_eq!(run.get("gen_ai.agent.name"), None);
    assert_eq!(only(&rm, "gen_ai.invoke_agent.inference_calls").sum, 2.0);
}

/// Never answers.
struct Silent;
impl ToolModel for Silent {
    fn model_name(&self) -> &'static str {
        "silent"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(std::future::pending())
    }
}

/// A run dropped mid-turn — an executor timeout does this — is recorded,
/// and so is the turn it was in, both as `cancelled`.
#[tokio::test]
async fn a_run_dropped_mid_turn_is_recorded_as_cancelled() {
    let (meter, reader, _provider) = recording_meter();
    let tool_loop = ToolLoop::new(Silent, ToolSet::new()).with_meter(meter);
    let cancel = CancellationToken::new();
    let timed_out = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        tool_loop.run("hi", &cancel),
    )
    .await;
    assert!(timed_out.is_err());
    let rm = collect(&reader);
    assert_eq!(
        only(&rm, "gen_ai.invoke_agent.duration").get("error.type"),
        Some("cancelled")
    );
    assert_eq!(only(&rm, "gen_ai.invoke_agent.inference_calls").sum, 1.0);
    assert_eq!(
        only(&rm, "gen_ai.client.inference.duration").get("error.type"),
        Some("cancelled")
    );
}

/// Cancelled through its token rather than dropped: the same record.
#[tokio::test]
async fn a_run_cancelled_mid_turn_is_recorded_as_cancelled() {
    let (meter, reader, _provider) = recording_meter();
    let tool_loop = ToolLoop::new(Silent, ToolSet::new()).with_meter(meter);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        trigger.cancel();
    });
    let result = tool_loop.run("hi", &cancel).await;
    assert!(matches!(result, Err(LoopError::Cancelled)), "{result:?}");
    let rm = collect(&reader);
    assert_eq!(
        only(&rm, "gen_ai.invoke_agent.duration").get("error.type"),
        Some("cancelled")
    );
    assert_eq!(
        only(&rm, "gen_ai.client.inference.duration").get("error.type"),
        Some("cancelled")
    );
}
