// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The observability gate: what an operator sees from one real server.
//!
//! ADR 0013 builds the gate before the work: a test that drives a JSON-RPC,
//! an HTTP+JSON and a gRPC call through a real server, each carrying a W3C
//! `traceparent`, with an in-memory span exporter and a manual metric reader
//! installed the way an adopter installs them — as the OpenTelemetry globals,
//! bridged from `tracing` by `tracing-opentelemetry`. It then asserts what the
//! ADR commits to, and reports every gap at once rather than the first:
//!
//! * **The span tree (O1, O4).** Each call has a `SERVER` span named
//!   `lf.a2a.v1.A2AService/SendMessage`, in the caller's trace, whose parent
//!   is the caller's span, marked remote, with `rpc.system.name` set for the
//!   binding (`jsonrpc`, `a2a_http_json`, `grpc`); and the executor's work
//!   runs in a span that is that span's child.
//! * **The downstream span id is a recorded span (O2).** The `traceparent`
//!   the executor would send onward names a span the exporter received —
//!   otherwise every downstream agent's trace points at a parent no backend
//!   has.
//! * **Every catalogued instrument is emitted (O5, O10, O11).** The catalogue
//!   is read from `book/src/deployment/observability.md`, so the page adopters
//!   build dashboards from is the contract; `rpc.server.call.duration` is
//!   added to it with the unit and advisory buckets the ADR takes from the
//!   semantic conventions.
//!
//! It fails on `main` as of 2026-09-23, which is its job: phase 2 of the
//! adopter audit turns it green one assertion at a time.
//!
//! One test, not several: the OpenTelemetry globals and the `tracing` default
//! are process-wide, so everything shares one installation.

#![cfg(all(feature = "otel", feature = "grpc", feature = "tracing"))]

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_sdk::client::{
    A2aClient, ClientBuilder, CurrentTrace, TracePropagationInterceptor,
};
use a2a_protocol_sdk::server::otel::OtelMetricsBuilder;
use a2a_protocol_sdk::server::store::InMemoryTaskStore;
use a2a_protocol_sdk::server::{
    GrpcConfig, GrpcDispatcher, HttpPushSender, JsonRpcDispatcher, RequestHandlerBuilder,
    RestDispatcher, serve_with_addr,
};
use a2a_protocol_sdk::types::params::{SendMessageConfiguration, TaskQueryParams};
use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
use a2a_protocol_sdk::types::trace_context::TraceContext;
use a2a_protocol_sdk::types::{AgentCapabilities, AgentCard, AgentInterface};
use opentelemetry::trace::{SpanKind, TracerProvider as _};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tracing_subscriber::layer::SubscriberExt;

mod fixtures;
use fixtures::*;

/// The advisory buckets for `rpc.server.call.duration`, from the semantic
/// conventions at `838e414` as ADR 0013 records them.
const RPC_DURATION_BUCKETS: [f64; 14] = [
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];

/// The fully-qualified method the ADR names every binding's server span for.
const SEND_SPAN: &str = "lf.a2a.v1.A2AService/SendMessage";

/// The method, and the JSON-RPC error code, of the call that fails.
const GET_TASK: &str = "lf.a2a.v1.A2AService/GetTask";
const TASK_NOT_FOUND: &str = "-32001";

struct Call {
    binding: &'static str,
    system: &'static str,
    trace_id: &'static str,
    parent_id: &'static str,
}

const CALLS: [Call; 3] = [
    Call {
        binding: "JSONRPC",
        system: "jsonrpc",
        trace_id: "0af7651916cd43dd8448eb211c80319c",
        parent_id: "b7ad6b7169203331",
    },
    Call {
        binding: "HTTP+JSON",
        system: "a2a_http_json",
        trace_id: "1bf7651916cd43dd8448eb211c80319c",
        parent_id: "c7ad6b7169203331",
    },
    Call {
        binding: "GRPC",
        system: "grpc",
        trace_id: "2cf7651916cd43dd8448eb211c80319c",
        parent_id: "d7ad6b7169203331",
    },
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_server_three_bindings_what_an_operator_sees() {
    // ── Telemetry, installed as an adopter installs it ──────────────────────
    let exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());
    let subscriber = tracing_subscriber::registry().with(
        tracing_opentelemetry::layer().with_tracer(tracer_provider.tracer("observability_e2e")),
    );
    tracing::subscriber::set_global_default(subscriber)
        .expect("the only subscriber in this binary");

    let reader = SharedReader(Arc::new(ManualReader::default()));
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader.clone())
        .build();
    opentelemetry::global::set_meter_provider(meter_provider.clone());

    // ── One server, three bindings ──────────────────────────────────────────
    let card = AgentCard::new(
        "observed",
        "1.0.0",
        AgentInterface::jsonrpc("http://127.0.0.1"),
    )
    .with_capabilities(
        AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(true),
    );
    let handler = Arc::new(
        RequestHandlerBuilder::new(Recorder)
            .with_agent_card(card)
            .with_task_store(FailingStore(InMemoryTaskStore::new()))
            .with_push_sender(HttpPushSender::new().allow_private_urls())
            .with_metrics(OtelMetricsBuilder::new().build())
            .build()
            .expect("handler"),
    );
    let jsonrpc = serve_with_addr("127.0.0.1:0", JsonRpcDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("jsonrpc");
    let rest = serve_with_addr("127.0.0.1:0", RestDispatcher::new(Arc::clone(&handler)))
        .await
        .expect("rest");
    let grpc = GrpcDispatcher::new(Arc::clone(&handler), GrpcConfig::default())
        .serve_with_addr("127.0.0.1:0")
        .await
        .expect("grpc");

    let mut clients: HashMap<&str, A2aClient> = HashMap::new();
    for (binding, url) in [
        ("JSONRPC", format!("http://{jsonrpc}")),
        ("HTTP+JSON", format!("http://{rest}")),
    ] {
        let client = ClientBuilder::new(url)
            .with_protocol_binding(binding)
            .with_interceptor(TracePropagationInterceptor::new())
            .build()
            .expect("client");
        clients.insert(binding, client);
    }
    let grpc_client = ClientBuilder::new(format!("http://{grpc}"))
        .with_protocol_binding("GRPC")
        .with_interceptor(TracePropagationInterceptor::new())
        .build_grpc()
        .await
        .expect("grpc client");
    clients.insert("GRPC", grpc_client);

    // ── The workload ────────────────────────────────────────────────────────
    for call in &CALLS {
        let inbound = TraceContext::parse(&format!("00-{}-{}-01", call.trace_id, call.parent_id))
            .expect("traceparent");
        let client = &clients[call.binding];
        CurrentTrace::scope(inbound, client.send_message(message(call.binding, None)))
            .await
            .unwrap_or_else(|e| panic!("{} send: {e}", call.binding));
    }
    // An error, a stream that persists nothing, and a push delivery: the
    // failure and delivery instruments record only when those happen.
    let jsonrpc_client = &clients["JSONRPC"];
    let _ = jsonrpc_client
        .get_task(TaskQueryParams {
            tenant: None,
            id: "no-such-task".to_owned(),
            history_length: None,
        })
        .await
        .expect_err("an unknown task is an error");
    let mut stream = jsonrpc_client
        .stream_message(message("persist-fail", Some("persist-fail")))
        .await
        .expect("stream");
    while tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .ok()
        .flatten()
        .is_some()
    {}
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let hook = webhook(Arc::clone(&hits)).await;
    let mut push = message("push", None);
    push.configuration = Some(SendMessageConfiguration {
        task_push_notification_config: Some(TaskPushNotificationConfig::new("", hook)),
        ..SendMessageConfiguration::default()
    });
    let mut stream = jsonrpc_client
        .stream_message(push)
        .await
        .expect("push stream");
    while tokio::time::timeout(Duration::from_secs(10), stream.next())
        .await
        .ok()
        .flatten()
        .is_some()
    {}
    // Push delivery runs behind the stream: wait for the webhook to be hit,
    // with a deadline, rather than for a fixed time (CONTRIBUTING.md).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while hits.load(std::sync::atomic::Ordering::SeqCst) == 0
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }

    // ── What the exporter and the reader received ───────────────────────────
    // A span is exported when it closes, and `tracing` keeps a parent open
    // until its children close — the call's span waits for the background
    // processor and the SSE writer it spawned (its recorded end time is still
    // the call's own). So wait, with a deadline, for every span the SDK
    // spawns to have its parent exported, rather than for a fixed time.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let spans = loop {
        let _ = tracer_provider.force_flush();
        let spans = exporter.get_finished_spans().expect("spans");
        let ids: std::collections::HashSet<_> =
            spans.iter().map(|s| s.span_context.span_id()).collect();
        let settled = spans
            .iter()
            .filter(|s| s.name.starts_with("a2a."))
            .all(|s| ids.contains(&s.parent_span_id));
        if settled || tokio::time::Instant::now() >= deadline {
            break spans;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    let seen: BTreeMap<String, Option<String>> =
        SEEN.lock().expect("lock").iter().cloned().collect();
    let mut gaps = Vec::new();

    for call in &CALLS {
        let b = call.binding;
        let server = spans.iter().find(|s| {
            s.span_kind == SpanKind::Server
                && s.name == SEND_SPAN
                && s.span_context.trace_id().to_string() == call.trace_id
        });
        let Some(server) = server else {
            gaps.push(format!(
                "{b}: no SERVER span `{SEND_SPAN}` in trace {} (O1)",
                call.trace_id
            ));
            continue;
        };
        if server.parent_span_id.to_string() != call.parent_id || !server.parent_span_is_remote {
            gaps.push(format!(
                "{b}: the server span's parent is {} (remote: {}), not the caller's {} (O1)",
                server.parent_span_id, server.parent_span_is_remote, call.parent_id
            ));
        }
        if attr(server, "rpc.system.name").as_deref() != Some(call.system) {
            gaps.push(format!(
                "{b}: rpc.system.name is {:?}, not {:?}",
                attr(server, "rpc.system.name"),
                call.system
            ));
        }
        // ADR 0013, option 2: the HTTP+JSON span also carries the HTTP
        // method and the matched route template, for HTTP-oriented tooling.
        if call.system == "a2a_http_json" {
            for (key, want) in [
                ("http.request.method", "POST"),
                ("http.route", "/message:send"),
            ] {
                if attr(server, key).as_deref() != Some(want) {
                    gaps.push(format!(
                        "{b}: {key} is {:?}, not {want:?}",
                        attr(server, key)
                    ));
                }
            }
        }
        let own = server.span_context.span_id();
        if !spans.iter().any(|s| s.parent_span_id == own) {
            gaps.push(format!(
                "{b}: the executor's work has no span under the server span (O4)"
            ));
        }
        // The book: "Task and context identifiers are on the spans" (O1).
        let execute = spans.iter().find(|s| {
            s.name == "a2a.execute" && s.span_context.trace_id() == server.span_context.trace_id()
        });
        match execute {
            None => gaps.push(format!(
                "{b}: no `a2a.execute` span in the call's trace (O4)"
            )),
            Some(span) => {
                for key in ["a2a.task.id", "a2a.context.id"] {
                    if attr(span, key).is_none_or(|v| v.is_empty()) {
                        gaps.push(format!("{b}: the executor's span has no `{key}` (O1)"));
                    }
                }
            }
        }
        match seen.get(b) {
            None => gaps.push(format!("{b}: the executor never ran")),
            Some(None) => gaps.push(format!("{b}: the executor saw no trace context")),
            Some(Some(downstream)) => {
                if !spans
                    .iter()
                    .any(|s| s.span_context.span_id().to_string() == *downstream)
                {
                    gaps.push(format!(
                        "{b}: the span id sent downstream, {downstream}, is no recorded span (O2)"
                    ));
                }
            }
        }
    }

    // Every span the SDK opens for work it spawns is a child of a recorded
    // span. One whose parent is missing is the root of a trace of its own —
    // which is what `a2a.sse` was when the SSE writer was spawned after the
    // call's span had closed (O4).
    let ids: std::collections::HashSet<_> =
        spans.iter().map(|s| s.span_context.span_id()).collect();
    for span in spans.iter().filter(|s| s.name.starts_with("a2a.")) {
        if !ids.contains(&span.parent_span_id) {
            gaps.push(format!(
                "`{}` in trace {} has no recorded parent (O4)",
                span.name,
                span.span_context.trace_id()
            ));
        }
    }
    // A call's span ends when the call does, not when the stream it opened
    // does: `tracing` keeps it open for its children, and the exported end
    // time must still be the call's own.
    for sse in spans.iter().filter(|s| s.name == "a2a.sse") {
        if let Some(call) = spans
            .iter()
            .find(|s| s.span_context.span_id() == sse.parent_span_id)
            && call.end_time >= sse.end_time
        {
            gaps.push(format!(
                "`{}` ends with the stream it opened, not with the call (O1)",
                call.name
            ));
        }
    }
    for name in ["a2a.execute", "a2a.process_events", "a2a.sse"] {
        if !spans.iter().any(|s| s.name == name) {
            gaps.push(format!("no `{name}` span was recorded (O4)"));
        }
    }

    // The unknown task: a failed call, whose span says why with the code the
    // caller was answered with (semconv: `error.type` is that status code).
    let failed = spans
        .iter()
        .find(|s| s.span_kind == SpanKind::Server && s.name == GET_TASK);
    match failed {
        None => gaps.push(format!(
            "no SERVER span `{GET_TASK}` for the failed GetTask (O1)"
        )),
        Some(span) => {
            if attr(span, "error.type").as_deref() != Some(TASK_NOT_FOUND) {
                gaps.push(format!(
                    "the failed GetTask span's error.type is {:?}, not {TASK_NOT_FOUND:?}",
                    attr(span, "error.type")
                ));
            }
            if !matches!(span.status, opentelemetry::trace::Status::Error { .. }) {
                gaps.push(format!(
                    "the failed GetTask span's status is {:?}, not Error",
                    span.status
                ));
            }
        }
    }

    let mut rm = ResourceMetrics::default();
    reader.collect(&mut rm).expect("collect");
    let mut emitted = BTreeMap::new();
    for scope in rm.scope_metrics() {
        for metric in scope.metrics() {
            emitted.insert(metric.name().to_owned(), metric);
        }
    }
    let mut catalogue = book_catalogue();
    catalogue.insert("rpc.server.call.duration".to_owned());
    for name in &catalogue {
        if !emitted.contains_key(name.as_str()) {
            gaps.push(format!(
                "instrument `{name}` is catalogued and was never emitted"
            ));
        }
    }
    if let Some(metric) = emitted.get("rpc.server.call.duration") {
        if metric.unit() != "s" {
            gaps.push(format!(
                "rpc.server.call.duration has unit {:?}, not \"s\"",
                metric.unit()
            ));
        }
        if let AggregatedMetrics::F64(MetricData::Histogram(h)) = metric.data() {
            for point in h.data_points() {
                let bounds: Vec<f64> = point.bounds().collect();
                if bounds != RPC_DURATION_BUCKETS {
                    gaps.push(format!(
                        "rpc.server.call.duration buckets are {bounds:?} (O5)"
                    ));
                    break;
                }
            }
            // Each binding's send, recorded as a success under its own
            // `rpc.system.name`; the unknown task, as a failure carrying the
            // JSON-RPC code it was answered with.
            let points: Vec<BTreeMap<String, String>> = h
                .data_points()
                .map(|p| {
                    p.attributes()
                        .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                        .collect()
                })
                .collect();
            let has = |p: &BTreeMap<String, String>, k: &str, v: &str| {
                p.get(k).map(String::as_str) == Some(v)
            };
            for call in &CALLS {
                if !points.iter().any(|p| {
                    has(p, "rpc.system.name", call.system)
                        && has(p, "rpc.method", SEND_SPAN)
                        && !p.contains_key("error.type")
                }) {
                    gaps.push(format!(
                        "{}: no successful rpc.server.call.duration point for {SEND_SPAN} (O5); points: {points:?}",
                        call.binding
                    ));
                }
            }
            if !points.iter().any(|p| {
                has(p, "rpc.system.name", "jsonrpc")
                    && has(p, "rpc.method", GET_TASK)
                    && has(p, "error.type", TASK_NOT_FOUND)
                    && has(p, "rpc.status_code", TASK_NOT_FOUND)
            }) {
                gaps.push(format!(
                    "no rpc.server.call.duration point for the failed GetTask with error.type {TASK_NOT_FOUND:?} (O5, O11)"
                ));
            }
        } else {
            gaps.push("rpc.server.call.duration is not an f64 histogram".to_owned());
        }
    }

    assert!(
        gaps.is_empty(),
        "{} observability gap(s), from {} spans and {} instruments:\n  {}",
        gaps.len(),
        spans.len(),
        emitted.len(),
        gaps.join("\n  ")
    );
}
