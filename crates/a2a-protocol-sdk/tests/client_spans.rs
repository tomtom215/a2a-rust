// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The client's spans across a delegation chain: a caller, an orchestrator
//! agent that delegates, and a worker — with no `TracePropagationInterceptor`
//! and no `CurrentTrace` scope anywhere, so the one trace the chain shares is
//! the client's own doing.
//!
//! Each test records through a thread-local subscriber on a `current_thread`
//! runtime, so every server and client task runs on the thread the
//! subscriber is set on, and tests share nothing.

#![cfg(feature = "otel")]

use std::sync::Arc;
use std::time::SystemTime;

use a2a_protocol_sdk::prelude::*;
use opentelemetry::trace::{SpanKind, TracerProvider as _};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use tracing_subscriber::layer::SubscriberExt as _;

const SEND: &str = "lf.a2a.v1.A2AService/SendMessage";
const STREAM: &str = "lf.a2a.v1.A2AService/SendStreamingMessage";

struct Worker;

agent_executor!(Worker, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    emit.artifact("out", vec![Part::text("worked")], None, Some(true))
        .await?;
    emit.status(TaskState::Completed).await
});

/// Delegates every message to the worker whose card it was built with.
struct Orchestrator {
    worker: AgentCard,
    propagate: bool,
}

impl AgentExecutor for Orchestrator {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;
            let client = ClientBuilder::from_card(&self.worker)
                .map_err(|e| A2aError::internal(e.to_string()))?
                .with_trace_propagation(self.propagate)
                .build()
                .map_err(|e| A2aError::internal(e.to_string()))?;
            client
                .send_message(MessageSendParams::new(Message::user(
                    "delegated",
                    vec![Part::text("go")],
                )))
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?;
            emit.artifact("out", vec![Part::text("delegated")], None, Some(true))
                .await?;
            emit.status(TaskState::Completed).await
        })
    }
}

async fn serve(executor: impl AgentExecutor, name: &str) -> AgentCard {
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = probe.local_addr().expect("addr");
    drop(probe);
    let card = AgentCard::new(
        name,
        "3.1.4",
        AgentInterface::jsonrpc(format!("http://{addr}")),
    )
    .with_description(format!("the {name}"))
    .with_capabilities(AgentCapabilities::none().with_streaming(true));
    let handler = Arc::new(
        RequestHandlerBuilder::new(executor)
            .with_agent_card(card.clone())
            .build()
            .expect("handler"),
    );
    serve_with_addr(addr, JsonRpcDispatcher::new(handler))
        .await
        .expect("serve");
    card
}

/// Runs the chain once, the caller's send inside a root span, and returns
/// every span recorded.
fn chain(propagate: bool) -> Vec<SpanData> {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("client_spans")));
    tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let worker = serve(Worker, "worker").await;
            let orchestrator = serve(Orchestrator { worker, propagate }, "orchestrator").await;
            let client = ClientBuilder::from_card(&orchestrator)
                .expect("card")
                .build()
                .expect("client");
            let root = tracing::info_span!("caller-root");
            tracing::Instrument::instrument(
                client.send_message(MessageSendParams::new(Message::user(
                    "m",
                    vec![Part::text("hi")],
                ))),
                root,
            )
            .await
            .expect("send");
            // Spans close when their tasks finish; let the servers' finish.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });
    });
    let _ = provider.force_flush();
    exporter.get_finished_spans().expect("spans")
}

fn attr(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

fn agent_of(span: &SpanData) -> String {
    attr(span, "gen_ai.agent.name").unwrap_or_default()
}

/// One trace from the caller to the worker; each server span is the child
/// of the client span that called it; the orchestrator's client span is the
/// child of its `invoke_agent` span.
#[test]
fn a_delegation_chain_is_one_trace_with_each_hop_under_its_call() {
    let spans = chain(true);
    let root = spans
        .iter()
        .find(|s| s.name == "caller-root")
        .expect("root span");
    let trace = root.span_context.trace_id();
    let named = |kind: SpanKind, agent: &str| {
        spans
            .iter()
            .find(|s| s.name == SEND && s.span_kind == kind && agent_of(s) == agent)
            .unwrap_or_else(|| panic!("no {kind:?} `{SEND}` for {agent}: {:?}", names(&spans)))
    };
    let call_orchestrator = named(SpanKind::Client, "orchestrator");
    let served_orchestrator = named(SpanKind::Server, "orchestrator");
    let call_worker = named(SpanKind::Client, "worker");
    let served_worker = named(SpanKind::Server, "worker");
    let invoke = spans
        .iter()
        .find(|s| s.name == "invoke_agent orchestrator")
        .expect("the orchestrator's invoke_agent span");

    for s in [
        call_orchestrator,
        served_orchestrator,
        invoke,
        call_worker,
        served_worker,
    ] {
        assert_eq!(
            s.span_context.trace_id(),
            trace,
            "`{}` left the trace",
            s.name
        );
    }
    assert_eq!(
        call_orchestrator.parent_span_id,
        root.span_context.span_id()
    );
    assert_eq!(
        served_orchestrator.parent_span_id,
        call_orchestrator.span_context.span_id()
    );
    assert!(served_orchestrator.parent_span_is_remote);
    assert_eq!(call_worker.parent_span_id, invoke.span_context.span_id());
    assert_eq!(
        served_worker.parent_span_id,
        call_worker.span_context.span_id()
    );
    assert!(served_worker.parent_span_is_remote);
}

/// The client span's attributes: the A2A draft conventions', the agent from
/// its card, where it was reached, and the task the response named.
#[test]
fn the_client_span_describes_the_call_and_its_outcome() {
    let spans = chain(true);
    let call = spans
        .iter()
        .find(|s| s.name == SEND && s.span_kind == SpanKind::Client && agent_of(s) == "worker")
        .expect("client span");
    for (key, want) in [
        ("rpc.system.name", "jsonrpc"),
        ("rpc.method", SEND),
        ("a2a.method.name", "SendMessage"),
        ("a2a.protocol.version", "1.0"),
        ("a2a.message.id", "delegated"),
        ("a2a.task.state", "TASK_STATE_COMPLETED"),
        ("gen_ai.agent.name", "worker"),
        ("gen_ai.agent.description", "the worker"),
        ("gen_ai.agent.version", "3.1.4"),
        ("server.address", "127.0.0.1"),
    ] {
        assert_eq!(attr(call, key).as_deref(), Some(want), "{key}");
    }
    let served = spans
        .iter()
        .find(|s| s.name == SEND && s.span_kind == SpanKind::Server && agent_of(s) == "worker")
        .expect("server span");
    // Both ends name the same task and context.
    for key in ["a2a.task.id", "gen_ai.conversation.id"] {
        assert!(attr(call, key).is_some(), "{key}");
        assert_eq!(attr(call, key), attr(served, key), "{key}");
    }
    assert!(attr(call, "server.port").is_some());
    assert!(attr(call, "error.type").is_none());
    // Every attribute once.
    for s in &spans {
        let mut keys: Vec<&str> = s.attributes.iter().map(|kv| kv.key.as_str()).collect();
        keys.sort_unstable();
        assert!(
            keys.windows(2).all(|w| w[0] != w[1]),
            "`{}` repeats a key: {keys:?}",
            s.name
        );
    }
}

/// With propagation off, the worker learns nothing of the trace: its server
/// span starts a trace of its own. The counter-test that makes the first
/// test's parentage the client's doing rather than an accident.
#[test]
fn with_propagation_off_the_called_agent_starts_its_own_trace() {
    let spans = chain(false);
    let call_worker = spans
        .iter()
        .find(|s| s.name == SEND && s.span_kind == SpanKind::Client && agent_of(s) == "worker")
        .expect("the client span is still recorded");
    let served_worker = spans
        .iter()
        .find(|s| s.name == SEND && s.span_kind == SpanKind::Server && agent_of(s) == "worker")
        .expect("server span");
    assert_ne!(
        served_worker.span_context.trace_id(),
        call_worker.span_context.trace_id()
    );
    assert!(!served_worker.parent_span_is_remote);
}

/// A streaming call's client span covers consuming the stream — it ends no
/// earlier than the last event's arrival — and is exported only once the
/// stream is dropped, not when it opened.
#[test]
fn a_streaming_calls_span_ends_when_its_stream_is_dropped() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("client_spans")));
    let mut consumed_at = None;
    tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        rt.block_on(async {
            let worker = serve(Worker, "worker").await;
            let client = ClientBuilder::from_card(&worker)
                .expect("card")
                .build()
                .expect("client");
            let mut stream = client
                .stream_message(MessageSendParams::new(Message::user(
                    "s",
                    vec![Part::text("go")],
                )))
                .await
                .expect("stream");
            let mut events = 0;
            while let Some(event) = stream.next().await {
                event.expect("event");
                events += 1;
                consumed_at = Some(SystemTime::now());
            }
            assert!(events >= 2, "{events} events");
            // Nothing exported the client span while the stream was open.
            let open = exporter.get_finished_spans().expect("spans");
            assert!(
                !open
                    .iter()
                    .any(|s| s.name == STREAM && s.span_kind == SpanKind::Client),
                "the client span closed before its stream was dropped"
            );
            drop(stream);
        });
    });
    let spans = exporter.get_finished_spans().expect("spans");
    let call = spans
        .iter()
        .find(|s| s.name == STREAM && s.span_kind == SpanKind::Client)
        .unwrap_or_else(|| panic!("no client stream span: {:?}", names(&spans)));
    assert!(call.end_time >= consumed_at.expect("consumed"));
}

fn names(spans: &[SpanData]) -> Vec<String> {
    spans
        .iter()
        .map(|s| format!("{}:{:?}:{}", s.name, s.span_kind, agent_of(s)))
        .collect()
}
