// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The example's claims, checked without Langfuse: spans go to an in-memory
//! exporter through the same `tracing-opentelemetry` bridge `Telemetry`
//! installs.

use super::*;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use tracing_subscriber::layer::SubscriberExt as _;

/// Runs one request through both agents with every span recorded, on a
/// `current_thread` runtime so every task sees the thread's subscriber.
fn traced_request(context: &str) -> (String, Vec<SpanData>) {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("langfuse-agent-test")));
    let report = tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let orchestrator = start_agents(None).await;
            let report = ask(&orchestrator, context, "what changed?")
                .await
                .expect("ask");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            report
        })
    });
    let _ = provider.force_flush();
    (report, exporter.get_finished_spans().expect("spans"))
}

fn attr(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

#[test]
fn the_orchestrator_reports_the_workers_answer() {
    let (report, _) = traced_request("ctx-answer");
    assert_eq!(
        report,
        "orchestrated: the worker's answer to: what changed?"
    );
}

/// What the README says Langfuse shows: one trace, the worker's run nested
/// under the orchestrator's, both in the request's context, with messages.
#[test]
fn both_runs_are_agents_in_one_trace_and_one_session() {
    let (_, spans) = traced_request("ctx-session");
    let find = |name: &str| {
        spans.iter().find(|s| s.name == name).unwrap_or_else(|| {
            panic!(
                "no `{name}` among {:?}",
                spans.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        })
    };
    let root = find("user request");
    let orchestrator = find("invoke_agent orchestrator");
    let worker = find("invoke_agent worker");
    for run in [orchestrator, worker] {
        assert_eq!(run.span_context.trace_id(), root.span_context.trace_id());
        assert_eq!(
            attr(run, "gen_ai.operation.name").as_deref(),
            Some("invoke_agent")
        );
        assert_eq!(
            attr(run, "gen_ai.conversation.id").as_deref(),
            Some("ctx-session")
        );
        assert!(
            attr(run, "gen_ai.input.messages").is_some(),
            "{} input",
            run.name
        );
        assert!(
            attr(run, "gen_ai.output.messages").is_some(),
            "{} output",
            run.name
        );
    }
    // Walk up from the worker's run: the orchestrator's run is an ancestor.
    let mut parent = worker.parent_span_id;
    let mut found = false;
    while let Some(span) = spans.iter().find(|s| s.span_context.span_id() == parent) {
        if span.span_context.span_id() == orchestrator.span_context.span_id() {
            found = true;
            break;
        }
        parent = span.parent_span_id;
    }
    assert!(found, "the worker's run is not under the orchestrator's");
}

/// The preset builds offline: traces on, metrics and logs off, nothing
/// installed globally.
#[test]
fn the_langfuse_preset_builds_without_a_network() {
    let telemetry = Telemetry::builder()
        .install_globally(false)
        .with_langfuse(Langfuse::new("http://127.0.0.1:9", "pk-lf-x", "sk-lf-x"))
        .build()
        .expect("build");
    assert!(telemetry.tracer_provider().is_some());
    assert!(telemetry.meter_provider().is_none());
    assert!(telemetry.logger_provider().is_none());
    telemetry
        .shutdown()
        .expect("nothing recorded, nothing to fail");
}
