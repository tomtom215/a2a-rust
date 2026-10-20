// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `rpc.client.call.duration` from real calls to this repository's server,
//! read through a `ManualReader` as the server's own metric tests read
//! `rpc.server.call.duration`.

use std::sync::Arc;
use std::time::Duration;

use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

use a2a_protocol_server::{JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::{Message, MessageSendParams, Part, TaskQueryParams};

use super::CallMetric;
use crate::{A2aClient, ClientBuilder};

/// A `ManualReader` the provider and the test can both hold.
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

/// One data point: its sorted attributes, its count and its sum.
type Point = (Vec<(String, String)>, u64, f64);

/// The `rpc.client.call.duration` points `reader` holds, checking the unit
/// and the advisory buckets on the way.
fn points(reader: &ManualReader) -> Vec<Point> {
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    let Some(metric) = collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .find(|m| m.name() == "rpc.client.call.duration")
    else {
        return Vec::new();
    };
    assert_eq!(metric.unit(), "s");
    let AggregatedMetrics::F64(MetricData::Histogram(hist)) = metric.data() else {
        panic!("not an f64 histogram");
    };
    hist.data_points()
        .map(|p| {
            assert_eq!(p.bounds().collect::<Vec<_>>(), super::BUCKETS);
            let mut attrs: Vec<(String, String)> = p
                .attributes()
                .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                .collect();
            attrs.sort();
            (attrs, p.count(), p.sum())
        })
        .collect()
}

struct Done;
agent_executor!(Done, |ctx, queue| async {
    a2a_protocol_server::EventEmitter::new(ctx, queue)
        .status(a2a_protocol_types::TaskState::Completed)
        .await
});

/// A JSON-RPC client of a fresh server, recording into a fresh reader.
async fn client() -> (A2aClient, Arc<ManualReader>, SdkMeterProvider, u16) {
    let handler = RequestHandlerBuilder::new(Done)
        .without_agent_card()
        .build()
        .expect("handler");
    let addr = a2a_protocol_server::serve_with_addr(
        "127.0.0.1:0",
        JsonRpcDispatcher::new(Arc::new(handler)),
    )
    .await
    .expect("serve");
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    let meter = opentelemetry::metrics::MeterProvider::meter(&provider, "a2a.client");
    let mut client = ClientBuilder::new(format!("http://{addr}"))
        .build()
        .expect("client");
    client.metric = CallMetric::from_meter(&meter);
    (client, reader, provider, addr.port())
}

fn send() -> MessageSendParams {
    MessageSendParams::new(Message::user("m-1", vec![Part::text("hi")]))
}

fn attrs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v: Vec<_> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    v.sort();
    v
}

/// Each call records one point with the conventions' attributes: a success
/// none of `error.type` or (on JSON-RPC) `rpc.status_code`; a failure the
/// JSON-RPC code it was answered with as both.
#[tokio::test]
async fn each_call_records_its_duration_with_the_conventions_attributes() {
    let (client, reader, _provider, port) = client().await;
    let port = port.to_string();
    client.send_message(send()).await.expect("send");
    let missing = TaskQueryParams {
        tenant: None,
        id: "no-such-task".into(),
        history_length: None,
    };
    client.get_task(missing).await.expect_err("unknown task");

    let mut seen: Vec<_> = points(&reader)
        .into_iter()
        .map(|(a, n, _)| (a, n))
        .collect();
    seen.sort();
    let common = [
        ("rpc.system.name", "jsonrpc"),
        ("server.address", "127.0.0.1"),
        ("server.port", port.as_str()),
    ];
    let mut failed = common.to_vec();
    failed.extend([
        ("rpc.method", "lf.a2a.v1.A2AService/GetTask"),
        ("rpc.status_code", "-32001"),
        ("error.type", "-32001"),
    ]);
    let mut sent = common.to_vec();
    sent.push(("rpc.method", "lf.a2a.v1.A2AService/SendMessage"));
    let mut want = vec![(attrs(&failed), 1), (attrs(&sent), 1)];
    want.sort();
    assert_eq!(seen, want);
}

/// A streaming call lasts until its stream is dropped, as its span does: no
/// point before, and one after whose duration covers the time it was held.
#[tokio::test]
async fn a_streaming_call_is_measured_until_its_stream_is_dropped() {
    const HELD: Duration = Duration::from_millis(60);
    let (client, reader, _provider, _) = client().await;
    let mut stream = client.stream_message(send()).await.expect("stream");
    while let Some(event) = stream.next().await {
        event.expect("event");
    }
    tokio::time::sleep(HELD).await;
    assert!(
        points(&reader).is_empty(),
        "recorded before the stream was dropped"
    );
    drop(stream);
    let points = points(&reader);
    assert_eq!(points.len(), 1, "{points:?}");
    let (attributes, count, sum) = &points[0];
    assert_eq!(*count, 1);
    assert!(
        attributes.contains(&(
            "rpc.method".to_owned(),
            "lf.a2a.v1.A2AService/SendStreamingMessage".to_owned()
        )),
        "{attributes:?}"
    );
    assert!(
        !attributes.iter().any(|(k, _)| k == "error.type"),
        "{attributes:?}"
    );
    assert!(
        *sum >= HELD.as_secs_f64(),
        "measured {sum} s, held {HELD:?}"
    );
}
