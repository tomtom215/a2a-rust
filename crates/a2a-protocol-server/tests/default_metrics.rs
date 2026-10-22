// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A handler built with no `with_metrics` records to the global meter
//! provider when the `otel` feature is on (the default): a process that
//! installs a provider — `Telemetry` does — gets `rpc.server.call.duration`
//! with nothing to enable. Its own test binary, because the provider is
//! process-global.

#![cfg(feature = "otel")]

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::{JsonRpcDispatcher, RequestHandlerBuilder, agent_executor};
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

#[tokio::test]
async fn a_handler_records_to_the_global_provider_by_default() {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    opentelemetry::global::set_meter_provider(provider);

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

    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    let names: Vec<String> = collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .map(|m| m.name().to_owned())
        .collect();
    assert!(
        names.iter().any(|n| n == "rpc.server.call.duration"),
        "a handler with no metrics configured recorded nothing to the global provider: {names:?}"
    );
}
