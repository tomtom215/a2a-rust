// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Real-meter tests for the connection counters and `rpc.server.call.duration`.

use std::time::Duration;

use opentelemetry_sdk::metrics::ManualReader;
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};

use super::tests::{recording_otel_metrics, sum_for};
use crate::metrics::{ConnectionPoolStats, Metrics, RpcCall};

/// A pool report carries totals since start; the counters must move by what
/// is new, once. Adding each total counted every earlier connection again on
/// every report (audit O10): three reports of 1, 2, 3 connections totalled 6.
/// A stale report — a smaller total arriving after a larger one — adds nothing.
#[test]
fn pool_counters_count_each_connection_once() {
    let (metrics, reader, provider) = recording_otel_metrics();

    for (created, closed) in [(1, 0), (2, 1), (3, 1), (2, 0)] {
        metrics.on_connection_pool_stats(&ConnectionPoolStats {
            active_connections: 0,
            idle_connections: 0,
            total_connections_created: created,
            connections_closed: closed,
        });
    }

    assert_eq!(sum_for(&reader, "a2a.server.pool.created"), Some(3));
    assert_eq!(sum_for(&reader, "a2a.server.pool.closed"), Some(1));

    let _ = provider.shutdown();
}

/// One `rpc.server.call.duration` data point: sorted attributes, sum, bounds.
type RpcPoint = (Vec<(String, String)>, f64, Vec<f64>);

/// The `rpc.server.call.duration` data points `reader` holds, checking the
/// instrument's unit and kind on the way.
fn rpc_call_points(reader: &ManualReader) -> Vec<RpcPoint> {
    use opentelemetry_sdk::metrics::reader::MetricReader as _;

    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect");
    let metric = collected
        .scope_metrics()
        .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
        .find(|m| m.name() == "rpc.server.call.duration")
        .expect("rpc.server.call.duration exported");
    assert_eq!(metric.unit(), "s");
    let AggregatedMetrics::F64(MetricData::Histogram(hist)) = metric.data() else {
        panic!("not an f64 histogram");
    };
    hist.data_points()
        .map(|p| {
            let mut attrs: Vec<(String, String)> = p
                .attributes()
                .map(|kv| (kv.key.to_string(), kv.value.to_string()))
                .collect();
            attrs.sort();
            (attrs, p.sum(), p.bounds().collect())
        })
        .collect()
}

/// `on_rpc_call` records `rpc.server.call.duration` in seconds, with the
/// conventions' buckets and attributes — `error.type` only on failure.
#[test]
fn on_rpc_call_records_the_semconv_histogram() {
    let (metrics, reader, provider) = recording_otel_metrics();
    metrics.on_rpc_call(&RpcCall {
        system: "grpc",
        method: Some("lf.a2a.v1.A2AService/GetTask"),
        duration: Duration::from_millis(30),
        status_code: Some("OK"),
        error_type: None,
    });
    metrics.on_rpc_call(&RpcCall {
        system: "jsonrpc",
        method: None,
        duration: Duration::from_millis(1),
        status_code: Some("-32700"),
        error_type: Some("-32700"),
    });

    let mut points = rpc_call_points(&reader);
    points.sort_by(|a, b| a.0.cmp(&b.0));
    let kv = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    assert_eq!(
        points[0].0,
        [
            kv("error.type", "-32700"),
            kv("rpc.status_code", "-32700"),
            kv("rpc.system.name", "jsonrpc"),
        ]
    );
    assert_eq!(
        points[1].0,
        [
            kv("rpc.method", "lf.a2a.v1.A2AService/GetTask"),
            kv("rpc.status_code", "OK"),
            kv("rpc.system.name", "grpc"),
        ]
    );
    assert!(
        (points[1].1 - 0.030).abs() < 1e-9,
        "seconds, not ms: {}",
        points[1].1
    );
    for (_, _, bounds) in &points {
        assert_eq!(bounds.as_slice(), super::RPC_DURATION_BUCKETS.as_slice());
    }

    let _ = provider.shutdown();
}
