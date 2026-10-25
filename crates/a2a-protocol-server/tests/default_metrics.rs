// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A handler built with no `with_metrics` records to the global meter
//! provider when the `otel` feature is on (the default): a process that
//! installs a provider — `Telemetry` does — gets `rpc.server.call.duration`
//! with nothing to enable, whenever it installs it. Its own test binary,
//! because the provider is process-global, and its tests take `GLOBAL` in turn.

#![cfg(feature = "otel")]

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use a2a_protocol_server::{JsonRpcDispatcher, Metrics, RequestHandlerBuilder, agent_executor};
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[derive(Debug)]
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

struct Idle;
agent_executor!(Idle, |_ctx, _queue| async { Ok(()) });

/// The tests share the process's one global provider, so they take turns.
static GLOBAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A provider with a reader the test can collect from.
fn provider() -> (SdkMeterProvider, Arc<ManualReader>) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    (provider, reader)
}

/// How many calls `reader`'s provider recorded in `rpc.server.call.duration`.
fn calls(reader: &ManualReader) -> u64 {
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, HistogramDataPoint, MetricData};
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .filter(|m| m.name() == "rpc.server.call.duration")
        .map(|m| match m.data() {
            AggregatedMetrics::F64(MetricData::Histogram(h)) => {
                h.data_points().map(HistogramDataPoint::count).sum()
            }
            _ => 0,
        })
        .sum()
}

/// One `GetTask` for a task that does not exist, over JSON-RPC.
async fn call(addr: std::net::SocketAddr) {
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"GetTask","params":{"id":"no-such-task"}}"#;
    let request = format!(
        "POST / HTTP/1.1\r\nhost: {addr}\r\ncontent-type: application/json\r\n\
         a2a-version: 1.0\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
    socket.write_all(request.as_bytes()).await.expect("write");
    let mut response = String::new();
    socket.read_to_string(&mut response).await.expect("read");
    assert!(
        response.contains("-32001"),
        "the server did not answer TaskNotFound (-32001)"
    );
}

/// The handler is built, and serves, before any provider is installed; one
/// installed later receives its calls, and so does one that replaces it.
/// Before 0.15 the default bound its instruments when the builder was
/// created, so a handler built before `Telemetry` recorded nothing all its
/// life.
#[tokio::test]
async fn a_handler_records_to_the_global_provider_installed_after_it() {
    let _turn = GLOBAL.lock().await;
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    call(addr).await;

    // Kept: dropping a provider's last handle shuts its reader down.
    let (first, first_reader) = provider();
    opentelemetry::global::set_meter_provider(first.clone());
    call(addr).await;
    assert_eq!(
        calls(&first_reader),
        1,
        "the provider installed after the handler was built recorded no call"
    );

    let (second, second_reader) = provider();
    opentelemetry::global::set_meter_provider(second.clone());
    call(addr).await;
    call(addr).await;
    assert_eq!(calls(&second_reader), 2, "the replacing provider");
    assert_eq!(
        calls(&first_reader),
        1,
        "the replaced provider kept recording"
    );
}

/// The runs `reader`'s provider recorded in `a2a.server.executor.duration`:
/// each outcome with how many runs ended that way.
fn executor_runs(reader: &ManualReader) -> Vec<(String, u64)> {
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    let mut runs = Vec::new();
    for metric in collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .filter(|m| m.name() == "a2a.server.executor.duration")
    {
        if let AggregatedMetrics::F64(MetricData::Histogram(h)) = metric.data() {
            for point in h.data_points() {
                let outcome = point
                    .attributes()
                    .find(|kv| kv.key.as_str() == "outcome")
                    .map(|kv| kv.value.to_string())
                    .unwrap_or_default();
                runs.push((outcome, point.count()));
            }
        }
    }
    runs
}

/// A run's time reaches the global provider through the default metrics, by
/// outcome: the executor histogram is part of what a process that installs a
/// provider gets with nothing to enable.
#[tokio::test]
async fn a_run_records_its_executor_duration_to_the_global_provider() {
    let _turn = GLOBAL.lock().await;
    let (provider, reader) = provider();
    opentelemetry::global::set_meter_provider(provider.clone());
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .build()
        .expect("handler");
    let params = a2a_protocol_types::MessageSendParams::new(a2a_protocol_types::Message::user(
        "m-1",
        vec![a2a_protocol_types::Part::text("hi")],
    ));
    handler
        .on_send_message(params, false, None)
        .await
        .expect("sent");
    // The run ends on its own task, just after the answer.
    let mut runs = Vec::new();
    for _ in 0..100 {
        runs = executor_runs(&reader);
        if !runs.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(runs, [("completed".to_owned(), 1)]);
}

/// Counts the requests it is told of.
#[derive(Default)]
struct Requests(AtomicU64);

impl Metrics for Requests {
    fn on_request(&self, _: &str) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// `with_metrics` replaces the export; `with_additional_metrics` keeps it and
/// adds the caller's, in the same handler. Both calls below reach the global
/// provider and the caller's counter.
#[tokio::test]
async fn additional_metrics_are_reported_beside_the_default_not_instead_of_it() {
    let _turn = GLOBAL.lock().await;
    let (installed, reader) = provider();
    opentelemetry::global::set_meter_provider(installed.clone());

    let requests = Arc::new(Requests::default());
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_additional_metrics(Arc::clone(&requests))
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    call(addr).await;
    assert_eq!(
        calls(&reader),
        1,
        "the default export was replaced, not kept"
    );
    assert_eq!(
        requests.0.load(Ordering::Relaxed),
        1,
        "the added metrics saw nothing"
    );

    // The contrast: `with_metrics` replaces the export, so a handler given
    // the same counter alone leaves the provider's (cumulative) count where
    // it was.
    let replaced = Arc::new(Requests::default());
    let handler = RequestHandlerBuilder::new(Idle)
        .without_agent_card()
        .with_metrics(Arc::clone(&replaced))
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    call(addr).await;
    assert_eq!(replaced.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        calls(&reader),
        1,
        "with_metrics must replace the export: the provider's count moved"
    );
}
