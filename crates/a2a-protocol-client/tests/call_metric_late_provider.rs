// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A client records `rpc.client.call.duration` to the global meter provider
//! as it stands at each call: one installed after the client was built gets
//! its calls, and so does one that replaces it. Its own test binary, because
//! the provider is process-global.

#![cfg(feature = "otel")]

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::{JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
use a2a_protocol_types::TaskQueryParams;
use opentelemetry_sdk::metrics::data::{
    AggregatedMetrics, HistogramDataPoint, MetricData, ResourceMetrics, ScopeMetrics,
};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

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

fn provider() -> (SdkMeterProvider, Arc<ManualReader>) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    (provider, reader)
}

/// How many calls `reader`'s provider recorded in `rpc.client.call.duration`.
fn calls(reader: &ManualReader) -> u64 {
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    collected
        .scope_metrics()
        .flat_map(ScopeMetrics::metrics)
        .filter(|m| m.name() == "rpc.client.call.duration")
        .map(|m| match m.data() {
            AggregatedMetrics::F64(MetricData::Histogram(h)) => {
                h.data_points().map(HistogramDataPoint::count).sum()
            }
            _ => 0,
        })
        .sum()
}

#[tokio::test]
async fn a_client_records_to_the_global_provider_installed_after_it() {
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
    let client = ClientBuilder::new(format!("http://{addr}"))
        .build()
        .expect("client");
    let call = || client.get_task(TaskQueryParams::new("no-such-task"));
    let _ = call().await;

    // Kept: dropping a provider's last handle shuts its reader down.
    let (first, first_reader) = provider();
    opentelemetry::global::set_meter_provider(first.clone());
    let _ = call().await;
    assert_eq!(
        calls(&first_reader),
        1,
        "the provider installed after the client was built recorded no call"
    );

    let (second, second_reader) = provider();
    opentelemetry::global::set_meter_provider(second.clone());
    let _ = call().await;
    let _ = call().await;
    assert_eq!(calls(&second_reader), 2, "the replacing provider");
    assert_eq!(
        calls(&first_reader),
        1,
        "the replaced provider kept recording"
    );
}
