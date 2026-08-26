// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the OpenTelemetry metrics exporter.
//!
//! Split from `mod.rs` to keep the exporter itself readable.
//!
//! Two layers, because neither alone is sufficient:
//!
//! * Noop-meter tests prove the instruments exist and accept the labels the
//!   call sites pass. They cannot prove a value leaves the process.
//! * `scripts/check_otel_metrics_coverage.py` proves every `Metrics` callback
//!   is *present* in the impl, catching the one a defaulted trait method would
//!   silently swallow. It cannot prove the body does anything.
//! * Real-meter tests (bottom of this file) collect from a `ManualReader` and
//!   assert the counter moved.
//!
//! The third layer exists because mutation testing showed the first two miss
//! the same thing: replacing the bodies of `on_persistence_error` and
//! `on_push_delivery` with `()` left every other test in the workspace green.
//! A present-but-empty method satisfies a structural check by construction.

use super::super::*;
use std::time::Duration;

/// Creates an `OtelMetrics` backed by a noop meter (no collector needed).
fn noop_otel_metrics() -> OtelMetrics {
    let meter = opentelemetry::global::meter("test");
    OtelMetrics::from_meter(&meter)
}

#[test]
fn from_meter_creates_all_instruments() {
    let metrics = noop_otel_metrics();
    let debug = format!("{metrics:?}");
    assert!(debug.contains("OtelMetrics"));
}

/// The two failure callbacks reach the exporter's instruments.
///
/// A noop meter cannot prove the values leave the process, so the real
/// guard against this exporter ignoring a callback is
/// `scripts/check_otel_metrics_coverage.py`. This covers the other half:
/// that the instruments exist and accept the labels the call sites pass.
#[test]
fn failure_callbacks_reach_their_instruments() {
    let metrics = noop_otel_metrics();
    metrics.on_persistence_error(
        crate::metrics::persistence_operation::ARTIFACT_APPEND,
        "internal_error",
    );
    metrics.on_persistence_error(
        crate::metrics::persistence_operation::STATUS_UPDATE,
        "internal_error",
    );
    for outcome in [
        crate::metrics::push_outcome::DELIVERED,
        crate::metrics::push_outcome::FAILED,
        crate::metrics::push_outcome::TIMEOUT,
        crate::metrics::push_outcome::SKIPPED,
    ] {
        metrics.on_push_delivery(outcome);
    }
}

#[test]
fn on_request_does_not_panic() {
    let metrics = noop_otel_metrics();
    metrics.on_request("message/send");
    metrics.on_request("tasks/get");
}

#[test]
fn on_response_does_not_panic() {
    let metrics = noop_otel_metrics();
    metrics.on_response("message/send");
}

#[test]
fn on_error_does_not_panic() {
    let metrics = noop_otel_metrics();
    metrics.on_error("message/send", "timeout");
    metrics.on_error("tasks/get", "not_found");
}

#[test]
fn on_latency_does_not_panic() {
    let metrics = noop_otel_metrics();
    metrics.on_latency("message/send", Duration::from_millis(42));
    metrics.on_latency("message/send", Duration::from_secs(0));
}

#[test]
fn on_queue_depth_change_does_not_panic() {
    let metrics = noop_otel_metrics();
    metrics.on_queue_depth_change(0);
    metrics.on_queue_depth_change(100);
}

#[test]
fn on_connection_pool_stats_does_not_panic() {
    let metrics = noop_otel_metrics();
    metrics.on_connection_pool_stats(&ConnectionPoolStats {
        active_connections: 5,
        idle_connections: 10,
        total_connections_created: 42,
        connections_closed: 3,
    });
}

// ── Observable-effect tests ─────────────────────────────────────────────

use opentelemetry::metrics::MeterProvider;
use opentelemetry_sdk::metrics::data::{
    AggregatedMetrics, GaugeDataPoint, HistogramDataPoint, MetricData, ResourceMetrics,
    SumDataPoint,
};
use opentelemetry_sdk::metrics::reader::MetricReader;
use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};
use opentelemetry_sdk::Resource;

struct CloneableReader(std::sync::Arc<ManualReader>);

impl std::fmt::Debug for CloneableReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CloneableReader")
    }
}

impl Clone for CloneableReader {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl MetricReader for CloneableReader {
    fn register_pipeline(&self, pipeline: std::sync::Weak<opentelemetry_sdk::metrics::Pipeline>) {
        self.0.register_pipeline(pipeline);
    }
    fn collect(&self, rm: &mut ResourceMetrics) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.collect(rm)
    }
    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.force_flush()
    }
    fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.0.shutdown_with_timeout(timeout)
    }
    fn temporality(
        &self,
        kind: opentelemetry_sdk::metrics::InstrumentKind,
    ) -> opentelemetry_sdk::metrics::Temporality {
        self.0.temporality(kind)
    }
}

fn metrics_with_reader() -> (OtelMetrics, CloneableReader) {
    let reader = CloneableReader(std::sync::Arc::new(ManualReader::default()));
    let provider = SdkMeterProvider::builder()
        .with_reader(reader.clone())
        .with_resource(Resource::builder().build())
        .build();
    let meter = provider.meter("test");
    let metrics = OtelMetrics::from_meter(&meter);
    std::mem::forget(provider);
    (metrics, reader)
}

fn collect_metrics(reader: &CloneableReader) -> ResourceMetrics {
    let mut rm = ResourceMetrics::default();
    reader.collect(&mut rm).expect("collect");
    rm
}

fn find_sum_u64(rm: &ResourceMetrics, name: &str) -> u64 {
    for scope in rm.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() == name {
                if let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() {
                    return sum.data_points().map(SumDataPoint::value).sum();
                }
            }
        }
    }
    0
}

#[test]
fn on_request_increments_counter() {
    let (metrics, reader) = metrics_with_reader();
    metrics.on_request("test/method");
    let rm = collect_metrics(&reader);
    assert!(
        find_sum_u64(&rm, "a2a.server.requests") > 0,
        "request counter should be incremented"
    );
}

#[test]
fn on_response_increments_counter() {
    let (metrics, reader) = metrics_with_reader();
    metrics.on_response("test/method");
    let rm = collect_metrics(&reader);
    assert!(
        find_sum_u64(&rm, "a2a.server.responses") > 0,
        "response counter should be incremented"
    );
}

#[test]
fn on_error_increments_counter() {
    let (metrics, reader) = metrics_with_reader();
    metrics.on_error("test/method", "timeout");
    let rm = collect_metrics(&reader);
    assert!(
        find_sum_u64(&rm, "a2a.server.errors") > 0,
        "error counter should be incremented"
    );
}

#[test]
fn on_latency_records_histogram() {
    let (metrics, reader) = metrics_with_reader();
    metrics.on_latency("test/method", Duration::from_millis(42));
    let rm = collect_metrics(&reader);

    let mut found = false;
    for scope in rm.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() == "a2a.server.latency" {
                if let AggregatedMetrics::F64(MetricData::Histogram(hist)) = metric.data() {
                    let count: u64 = hist.data_points().map(HistogramDataPoint::count).sum();
                    assert!(count > 0, "histogram should have recorded a value");
                    found = true;
                }
            }
        }
    }
    assert!(found, "latency histogram metric should exist");
}

#[test]
fn on_queue_depth_records_gauge() {
    let (metrics, reader) = metrics_with_reader();
    metrics.on_queue_depth_change(42);
    let rm = collect_metrics(&reader);

    let mut found = false;
    for scope in rm.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() == "a2a.server.queue_depth" {
                if let AggregatedMetrics::U64(MetricData::Gauge(gauge)) = metric.data() {
                    let val: u64 = gauge.data_points().map(GaugeDataPoint::value).sum();
                    assert_eq!(val, 42, "gauge should record 42");
                    found = true;
                }
            }
        }
    }
    assert!(found, "queue_depth gauge metric should exist");
}

#[test]
fn on_connection_pool_stats_records_all_instruments() {
    let (metrics, reader) = metrics_with_reader();
    metrics.on_connection_pool_stats(&ConnectionPoolStats {
        active_connections: 5,
        idle_connections: 10,
        total_connections_created: 42,
        connections_closed: 3,
    });
    let rm = collect_metrics(&reader);

    assert!(
        find_sum_u64(&rm, "a2a.server.pool.created") > 0,
        "pool.created counter should be incremented"
    );
    assert!(
        find_sum_u64(&rm, "a2a.server.pool.closed") > 0,
        "pool.closed counter should be incremented"
    );
}

// ── Real-meter assertions ────────────────────────────────────────────────────
//
// Everything above runs against a noop meter, which cannot tell a callback that
// records from one that does nothing. `check_otel_metrics_coverage.py` does not
// close that gap either: it proves the method is *present* in the impl, and a
// present-but-empty body satisfies it.
//
// Mutation testing found exactly that hole — replacing the bodies of
// `on_persistence_error` and `on_push_delivery` with `()` survived the entire
// workspace suite. These tests collect from a real `ManualReader` and assert the
// counter actually moved, which is the only formulation that fails when the
// body is emptied.

// `AggregatedMetrics`, `MetricData`, `ResourceMetrics`, `ManualReader` and
// `SdkMeterProvider` all arrive through the `use super::super::*;` glob above.
use std::sync::Arc;

/// `SdkMeterProvider::with_reader` takes ownership, so the reader is shared
/// through an `Arc` to stay readable after the provider is built.
#[derive(Debug, Clone)]
struct SharedReader(Arc<ManualReader>);

impl opentelemetry_sdk::metrics::reader::MetricReader for SharedReader {
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

/// Builds an `OtelMetrics` over a real SDK meter, returning the reader so the
/// caller can collect what was recorded.
fn recording_otel_metrics() -> (OtelMetrics, Arc<ManualReader>, SdkMeterProvider) {
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    let meter = opentelemetry::metrics::MeterProvider::meter(&provider, "a2a-otel-record-test");
    (OtelMetrics::from_meter(&meter), reader, provider)
}

/// Total of every u64 sum data point recorded under `name`.
///
/// `None` means the instrument never appeared, which is a different failure
/// from appearing with a zero total and is reported as such by the callers.
fn sum_for(reader: &ManualReader, name: &str) -> Option<u64> {
    use opentelemetry_sdk::metrics::reader::MetricReader as _;

    let mut collected = ResourceMetrics::default();
    reader.collect(&mut collected).expect("collect should work");

    let mut found = None;
    for scope in collected.scope_metrics() {
        for metric in scope.metrics() {
            if metric.name() != name {
                continue;
            }
            let mut total = 0_u64;
            if let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() {
                total += sum
                    .data_points()
                    .map(opentelemetry_sdk::metrics::data::SumDataPoint::value)
                    .sum::<u64>();
            }
            found = Some(found.unwrap_or(0) + total);
        }
    }
    found
}

/// `on_persistence_error` must actually increment `a2a.server.persistence_errors`.
///
/// This is the SDK's one silent-data-loss path: the streaming reader is a
/// separate subscriber and receives the event whether or not the store accepted
/// it, so a dropped persistence error is invisible to the client. An exporter
/// that accepts the callback and records nothing reproduces exactly the bug the
/// callback was added to fix.
#[test]
fn on_persistence_error_increments_its_counter() {
    let (metrics, reader, provider) = recording_otel_metrics();

    metrics.on_persistence_error(
        crate::metrics::persistence_operation::ARTIFACT_APPEND,
        "internal_error",
    );
    metrics.on_persistence_error(
        crate::metrics::persistence_operation::STATUS_UPDATE,
        "internal_error",
    );

    let total = sum_for(&reader, "a2a.server.persistence_errors")
        .expect("a2a.server.persistence_errors should have been exported");
    assert_eq!(
        total, 2,
        "two persistence errors were reported but the counter totals {total}"
    );

    let _ = provider.shutdown();
}

/// `on_push_delivery` must actually increment `a2a.server.push_deliveries`,
/// once per call, whatever the outcome label.
#[test]
fn on_push_delivery_increments_its_counter() {
    let (metrics, reader, provider) = recording_otel_metrics();

    let outcomes = [
        crate::metrics::push_outcome::DELIVERED,
        crate::metrics::push_outcome::FAILED,
    ];
    for outcome in outcomes {
        metrics.on_push_delivery(outcome);
    }

    let total = sum_for(&reader, "a2a.server.push_deliveries")
        .expect("a2a.server.push_deliveries should have been exported");
    assert_eq!(
        total,
        outcomes.len() as u64,
        "{} deliveries were reported but the counter totals {total}",
        outcomes.len()
    );

    let _ = provider.shutdown();
}
