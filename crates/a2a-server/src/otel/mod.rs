// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! OpenTelemetry integration for the A2A server.
//!
//! This module provides [`OtelMetrics`], an implementation of the [`Metrics`]
//! trait that records request counts, error counts, latency histograms, and
//! queue depth to OpenTelemetry instruments. Data is exported via the OTLP
//! protocol (gRPC) using the `opentelemetry-otlp` crate.
//!
//! # Module structure
//!
//! | Module | Responsibility |
//! |---|---|
//! | (this file) | `OtelMetrics` struct and `Metrics` trait impl |
//! | `builder` | `OtelMetricsBuilder` — fluent configuration |
//! | `pipeline` | `init_otlp_pipeline` — OTLP export setup |
//!
//! # Feature flag
//!
//! This module is only available when the `otel` feature is enabled.
//!
//! # Quick start
//!
//! ```rust,no_run
//! use a2a_protocol_server::otel::{OtelMetrics, OtelMetricsBuilder, init_otlp_pipeline};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 1. Initialise the OTLP export pipeline (sets the global MeterProvider).
//! let provider = init_otlp_pipeline("my-a2a-agent")?;
//!
//! // 2. Build the metrics instance.
//! let metrics = OtelMetricsBuilder::new()
//!     .meter_name("a2a.server")
//!     .build();
//!
//! // 3. Pass `metrics` to `RequestHandlerBuilder::metrics(metrics)`.
//! # Ok(())
//! # }
//! ```

mod builder;
mod pipeline;

use std::time::Duration;

use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
use opentelemetry::KeyValue;

use crate::metrics::{ConnectionPoolStats, Metrics};

pub use builder::OtelMetricsBuilder;
pub use pipeline::init_otlp_pipeline;

// ── OtelMetrics ──────────────────────────────────────────────────────────────

/// A [`Metrics`] implementation backed by OpenTelemetry instruments.
///
/// Records the following instruments:
///
/// | Instrument | Kind | Unit | Description |
/// |---|---|---|---|
/// | `a2a.server.requests` | Counter | `{request}` | Total inbound requests |
/// | `a2a.server.responses` | Counter | `{response}` | Total outbound responses |
/// | `a2a.server.errors` | Counter | `{error}` | Total errors |
/// | `a2a.server.latency` | Histogram | `s` | Request latency in seconds |
/// | `a2a.server.queue_depth` | Gauge | `{queue}` | Number of active event queues |
/// | `a2a.server.pool.active` | Gauge | `{connection}` | Active (in-use) connections |
/// | `a2a.server.pool.idle` | Gauge | `{connection}` | Idle connections |
/// | `a2a.server.pool.created` | Counter | `{connection}` | Total connections created |
/// | `a2a.server.pool.closed` | Counter | `{connection}` | Connections closed |
///
/// All counters and the histogram carry a `method` attribute.
/// The error counter additionally carries an `error` attribute.
pub struct OtelMetrics {
    request_counter: Counter<u64>,
    response_counter: Counter<u64>,
    error_counter: Counter<u64>,
    latency_histogram: Histogram<f64>,
    queue_depth_gauge: Gauge<u64>,
    pool_active_gauge: Gauge<u64>,
    pool_idle_gauge: Gauge<u64>,
    pool_created_counter: Counter<u64>,
    pool_closed_counter: Counter<u64>,
}

impl std::fmt::Debug for OtelMetrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OtelMetrics").finish_non_exhaustive()
    }
}

impl OtelMetrics {
    /// Create an `OtelMetrics` from an already-configured [`Meter`].
    ///
    /// Prefer [`OtelMetricsBuilder`] for typical usage.
    #[must_use]
    pub fn from_meter(meter: &Meter) -> Self {
        let request_counter = meter
            .u64_counter("a2a.server.requests")
            .with_description("Total number of inbound A2A requests")
            .with_unit("request")
            .build();

        let response_counter = meter
            .u64_counter("a2a.server.responses")
            .with_description("Total number of outbound A2A responses")
            .with_unit("response")
            .build();

        let error_counter = meter
            .u64_counter("a2a.server.errors")
            .with_description("Total number of A2A request errors")
            .with_unit("error")
            .build();

        let latency_histogram = meter
            .f64_histogram("a2a.server.latency")
            .with_description("A2A request latency")
            .with_unit("s")
            .build();

        let queue_depth_gauge = meter
            .u64_gauge("a2a.server.queue_depth")
            .with_description("Number of active event queues")
            .with_unit("queue")
            .build();

        let pool_active_gauge = meter
            .u64_gauge("a2a.server.pool.active")
            .with_description("Number of active (in-use) HTTP connections")
            .with_unit("connection")
            .build();

        let pool_idle_gauge = meter
            .u64_gauge("a2a.server.pool.idle")
            .with_description("Number of idle HTTP connections")
            .with_unit("connection")
            .build();

        let pool_created_counter = meter
            .u64_counter("a2a.server.pool.created")
            .with_description("Total HTTP connections created since process start")
            .with_unit("connection")
            .build();

        let pool_closed_counter = meter
            .u64_counter("a2a.server.pool.closed")
            .with_description("HTTP connections closed due to errors or timeouts")
            .with_unit("connection")
            .build();

        Self {
            request_counter,
            response_counter,
            error_counter,
            latency_histogram,
            queue_depth_gauge,
            pool_active_gauge,
            pool_idle_gauge,
            pool_created_counter,
            pool_closed_counter,
        }
    }
}

impl Metrics for OtelMetrics {
    fn on_request(&self, method: &str) {
        self.request_counter
            .add(1, &[KeyValue::new("method", method.to_owned())]);
    }

    fn on_response(&self, method: &str) {
        self.response_counter
            .add(1, &[KeyValue::new("method", method.to_owned())]);
    }

    fn on_error(&self, method: &str, error: &str) {
        self.error_counter.add(
            1,
            &[
                KeyValue::new("method", method.to_owned()),
                KeyValue::new("error", error.to_owned()),
            ],
        );
    }

    fn on_latency(&self, method: &str, duration: Duration) {
        self.latency_histogram.record(
            duration.as_secs_f64(),
            &[KeyValue::new("method", method.to_owned())],
        );
    }

    fn on_queue_depth_change(&self, active_queues: usize) {
        #[allow(clippy::cast_possible_truncation)]
        self.queue_depth_gauge.record(active_queues as u64, &[]);
    }

    fn on_connection_pool_stats(&self, stats: &ConnectionPoolStats) {
        self.pool_active_gauge
            .record(u64::from(stats.active_connections), &[]);
        self.pool_idle_gauge
            .record(u64::from(stats.idle_connections), &[]);
        self.pool_created_counter
            .add(stats.total_connections_created, &[]);
        self.pool_closed_counter.add(stats.connections_closed, &[]);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
    use opentelemetry_sdk::metrics::data::{ResourceMetrics, Sum};
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
        fn register_pipeline(
            &self,
            pipeline: std::sync::Weak<opentelemetry_sdk::metrics::Pipeline>,
        ) {
            self.0.register_pipeline(pipeline);
        }
        fn collect(
            &self,
            rm: &mut ResourceMetrics,
        ) -> opentelemetry_sdk::metrics::MetricResult<()> {
            self.0.collect(rm)
        }
        fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
            self.0.force_flush()
        }
        fn shutdown(&self) -> opentelemetry_sdk::error::OTelSdkResult {
            self.0.shutdown()
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
        let mut rm = ResourceMetrics {
            resource: Resource::builder().build(),
            scope_metrics: vec![],
        };
        reader.collect(&mut rm).expect("collect");
        rm
    }

    fn find_sum_u64(rm: &ResourceMetrics, name: &str) -> u64 {
        for scope in &rm.scope_metrics {
            for metric in &scope.metrics {
                if metric.name == name {
                    if let Some(sum) = metric.data.as_any().downcast_ref::<Sum<u64>>() {
                        return sum.data_points.iter().map(|dp| dp.value).sum();
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
        use opentelemetry_sdk::metrics::data::Histogram as DataHistogram;

        let (metrics, reader) = metrics_with_reader();
        metrics.on_latency("test/method", Duration::from_millis(42));
        let rm = collect_metrics(&reader);

        let mut found = false;
        for scope in &rm.scope_metrics {
            for metric in &scope.metrics {
                if metric.name == "a2a.server.latency" {
                    if let Some(hist) = metric.data.as_any().downcast_ref::<DataHistogram<f64>>() {
                        let count: u64 = hist.data_points.iter().map(|dp| dp.count).sum();
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
        use opentelemetry_sdk::metrics::data::Gauge as DataGauge;

        let (metrics, reader) = metrics_with_reader();
        metrics.on_queue_depth_change(42);
        let rm = collect_metrics(&reader);

        let mut found = false;
        for scope in &rm.scope_metrics {
            for metric in &scope.metrics {
                if metric.name == "a2a.server.queue_depth" {
                    if let Some(gauge) = metric.data.as_any().downcast_ref::<DataGauge<u64>>() {
                        let val: u64 = gauge.data_points.iter().map(|dp| dp.value).sum();
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
}
