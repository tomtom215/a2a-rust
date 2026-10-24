// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Every operation on every binding leaves a span and a duration, on both
//! sides of the call.
//!
//! `observability_e2e` reads one operation (SendMessage) on three bindings
//! closely. This reads all of them: the eleven operations in `Method::ALL`,
//! on JSON-RPC, HTTP+JSON, gRPC and WebSocket, through this SDK's client,
//! against one handler. For each (binding, operation) it requires a `SERVER`
//! span and a `rpc.server.call.duration` point from the server, and a
//! `CLIENT` span and a `rpc.client.call.duration` point from the client, each
//! named for the operation and carrying the binding's `rpc.system.name`.
//!
//! Why: an operation whose handler is wired to the telemetry by hand on one
//! binding and forgotten on another compiles, passes every functional test,
//! and has no series on any dashboard. A wiring audit (2026-10-24) found by
//! reading that the emission is structural, one wrapper per route; this
//! measures it.
//!
//! WebSocket reports `rpc.system.name` `jsonrpc`, as JSON-RPC over HTTP does,
//! so the two cannot be told apart in telemetry. The bindings therefore run
//! one after another, and the spans and the (delta) metrics are read and
//! cleared between them: what one binding's sweep emitted cannot be credited
//! to another's.

#![cfg(all(
    feature = "otel",
    feature = "tracing",
    feature = "grpc",
    feature = "websocket"
))]

// The harness is shared with `binding_equivalence`, which uses the parts this
// test does not (the recording interceptor, raw requests).
#[allow(dead_code)]
#[path = "../binding_equivalence/harness.rs"]
mod harness;
#[path = "../binding_equivalence/sweep.rs"]
mod sweep;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use a2a_protocol_types::method::Method;
use harness::{Binding, serve};
use opentelemetry::trace::{SpanKind, TracerProvider as _};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider, Temporality};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use tracing_subscriber::layer::SubscriberExt;

/// `ManualReader` is not `Clone`, and the provider takes its reader by value.
#[derive(Clone, Debug)]
struct SharedReader(Arc<ManualReader>);

impl MetricReader for SharedReader {
    fn register_pipeline(&self, pipeline: std::sync::Weak<opentelemetry_sdk::metrics::Pipeline>) {
        self.0.register_pipeline(pipeline);
    }
    fn collect(&self, rm: &mut ResourceMetrics) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.collect(rm)
    }
    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.force_flush()
    }
    fn shutdown_with_timeout(&self, timeout: Duration) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }
    fn temporality(
        &self,
        kind: opentelemetry_sdk::metrics::InstrumentKind,
    ) -> opentelemetry_sdk::metrics::Temporality {
        self.0.temporality(kind)
    }
}

/// The `rpc.system.name` each binding reports.
const fn system(binding: Binding) -> &'static str {
    match binding {
        Binding::JsonRpc | Binding::WebSocket => "jsonrpc",
        Binding::HttpJson => "a2a_http_json",
        Binding::Grpc => "grpc",
    }
}

fn qualified(method: Method) -> String {
    format!("lf.a2a.v1.A2AService/{}", method.wire_name())
}

/// What one side of the calls left: `(method, "span" | "metric")`.
type Seen = BTreeSet<(String, &'static str)>;

/// Reads the spans finished so far, and the metric points since the last
/// read, into `server` and `client`.
fn read(
    system: &str,
    exporter: &InMemorySpanExporter,
    tracer: &SdkTracerProvider,
    reader: &SharedReader,
    server: &mut Seen,
    client: &mut Seen,
) {
    let _ = tracer.force_flush();
    for span in exporter.get_finished_spans().expect("spans") {
        let on_this_binding = span
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "rpc.system.name" && kv.value.as_str() == system);
        let side = match span.span_kind {
            SpanKind::Server => &mut *server,
            SpanKind::Client => &mut *client,
            _ => continue,
        };
        if on_this_binding {
            side.insert((span.name.to_string(), "span"));
        }
    }
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    for metric in collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
    {
        let side = match metric.name() {
            "rpc.server.call.duration" => &mut *server,
            "rpc.client.call.duration" => &mut *client,
            _ => continue,
        };
        let AggregatedMetrics::F64(MetricData::Histogram(histogram)) = metric.data() else {
            continue;
        };
        for point in histogram.data_points() {
            let attr = |key: &str| {
                point
                    .attributes()
                    .find(|kv| kv.key.as_str() == key)
                    .map(|kv| kv.value.to_string())
            };
            if attr("rpc.system.name").as_deref() == Some(system)
                && let Some(method) = attr("rpc.method")
            {
                side.insert((method, "metric"));
            }
        }
    }
}

/// What a side should have left for every operation.
fn wanted() -> Seen {
    Method::ALL
        .iter()
        .flat_map(|m| [(qualified(*m), "span"), (qualified(*m), "metric")])
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_operation_on_every_binding_is_spanned_and_measured_on_both_sides() {
    let exporter = InMemorySpanExporter::default();
    let tracer = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    opentelemetry::global::set_tracer_provider(tracer.clone());
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(tracing_opentelemetry::layer().with_tracer(tracer.tracer("operation_matrix"))),
    )
    .expect("the only subscriber in this binary");
    // Delta, so each read holds only what the binding just swept emitted.
    let reader = SharedReader(Arc::new(
        ManualReader::builder()
            .with_temporality(Temporality::Delta)
            .build(),
    ));
    let meter = SdkMeterProvider::builder()
        .with_reader(reader.clone())
        .build();
    opentelemetry::global::set_meter_provider(meter.clone());

    let agent = serve(false, true).await;
    let mut gaps = Vec::new();
    for binding in Binding::ALL {
        exporter.reset();
        // Drain the previous binding's last points.
        let _ = reader.collect(&mut ResourceMetrics::default());
        let client = agent.client(binding, None).await;
        sweep::sweep(&client)
            .await
            .unwrap_or_else(|e| panic!("{binding:?}: {e}"));

        let (mut server, mut client_side) = (Seen::new(), Seen::new());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            read(
                system(binding),
                &exporter,
                &tracer,
                &reader,
                &mut server,
                &mut client_side,
            );
            let complete = wanted().is_subset(&server) && wanted().is_subset(&client_side);
            if complete || Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        for (side, seen) in [("server", &server), ("client", &client_side)] {
            for missing in wanted().difference(seen) {
                gaps.push(format!(
                    "{binding:?} {side}: no {} for {}",
                    missing.1, missing.0
                ));
            }
        }
    }
    assert!(
        gaps.is_empty(),
        "{} gap(s):\n{}",
        gaps.len(),
        gaps.join("\n")
    );
}
