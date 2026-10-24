// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Each push-notification POST is measured in `http.client.request.duration`
//! on the global meter provider, with the HTTP conventions' attributes: a
//! delivered attempt with its status, a refused one with its status as
//! `error.type`, and one that got no response as `transport`. Its own test
//! binary, because the provider is process-global.

#![cfg(feature = "otel")]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_server::push::{HttpPushSender, PushRetryPolicy, PushSender};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
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

/// A webhook answering every request with `status`; its URL and port.
async fn webhook(status: &'static str) -> (String, u16) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let _ = stream.read(&mut buf).await;
                let response =
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    (format!("http://127.0.0.1:{port}/hook/SECRET"), port)
}

async fn deliver(url: &str) {
    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("task-1"),
        context_id: ContextId::new("ctx"),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });
    let _ = HttpPushSender::new()
        .allow_private_urls()
        .with_retry_policy(PushRetryPolicy::default().with_max_attempts(1))
        .send(url, &event, &TaskPushNotificationConfig::new("task-1", url))
        .await;
}

/// Each data point of `http.client.request.duration`: its attributes and
/// its count, with the instrument's unit.
fn points(reader: &ManualReader) -> Vec<(BTreeMap<String, String>, u64, String)> {
    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    let mut out = Vec::new();
    for scope in collected.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() != "http.client.request.duration" {
                continue;
            }
            assert_eq!(scope.scope().name(), "a2a.server");
            let AggregatedMetrics::F64(MetricData::Histogram(h)) = metric.data() else {
                panic!("not an f64 histogram: {:?}", metric.data());
            };
            for point in h.data_points() {
                let attributes = point
                    .attributes()
                    .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                    .collect();
                assert_eq!(
                    point.bounds().collect::<Vec<_>>(),
                    [
                        0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5,
                        10.0
                    ]
                );
                out.push((attributes, point.count(), metric.unit().to_owned()));
            }
        }
    }
    out
}

fn attributes(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

#[tokio::test]
async fn every_webhook_attempt_is_measured_with_the_http_client_attributes() {
    // Kept: dropping a provider's last handle shuts its reader down.
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let (delivered, delivered_port) = webhook("204 No Content").await;
    let (refused, refused_port) = webhook("404 Not Found").await;
    let closed_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    deliver(&delivered).await;
    deliver(&delivered).await;
    deliver(&refused).await;
    deliver(&format!("http://127.0.0.1:{closed_port}/hook")).await;

    let mut got = points(&reader);
    got.sort();
    let mut want = vec![
        (
            attributes(&[
                ("http.request.method", "POST"),
                ("http.response.status_code", "204"),
                ("server.address", "127.0.0.1"),
                ("server.port", &delivered_port.to_string()),
            ]),
            2,
            "s".to_owned(),
        ),
        (
            attributes(&[
                ("error.type", "404"),
                ("http.request.method", "POST"),
                ("http.response.status_code", "404"),
                ("server.address", "127.0.0.1"),
                ("server.port", &refused_port.to_string()),
            ]),
            1,
            "s".to_owned(),
        ),
        (
            attributes(&[
                ("error.type", "transport"),
                ("http.request.method", "POST"),
                ("server.address", "127.0.0.1"),
                ("server.port", &closed_port.to_string()),
            ]),
            1,
            "s".to_owned(),
        ),
    ];
    want.sort();
    assert_eq!(got, want);
}
