// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! OpenTelemetry integration for the A2A server.
//!
//! This module provides [`OtelMetrics`], an implementation of the [`Metrics`]
//! trait that records request counts, error counts, latency histograms, and
//! queue depth to OpenTelemetry instruments. Data is exported via the OTLP
//! protocol (gRPC) using the `opentelemetry-otlp` crate.
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

use std::time::Duration;

use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::SdkMeterProvider;

use crate::metrics::{ConnectionPoolStats, Metrics};

/// Default meter name used when none is specified via the builder.
const DEFAULT_METER_NAME: &str = "a2a.server";

// ── Builder ──────────────────────────────────────────────────────────────────

/// Builder for [`OtelMetrics`].
///
/// Use [`OtelMetricsBuilder::new`] to create a builder, optionally configure
/// the meter name, then call [`build`](OtelMetricsBuilder::build) to obtain an
/// [`OtelMetrics`] instance.
#[derive(Debug, Clone)]
pub struct OtelMetricsBuilder {
    meter_name: &'static str,
}

impl Default for OtelMetricsBuilder {
    fn default() -> Self {
        Self {
            meter_name: DEFAULT_METER_NAME,
        }
    }
}

impl OtelMetricsBuilder {
    /// Create a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the OpenTelemetry meter name.
    ///
    /// Defaults to `"a2a.server"`.
    #[must_use]
    pub const fn meter_name(mut self, name: &'static str) -> Self {
        self.meter_name = name;
        self
    }

    /// Build the [`OtelMetrics`] instance.
    ///
    /// Instruments are created from the global [`MeterProvider`]. Make sure
    /// you have called [`init_otlp_pipeline`] (or otherwise installed a
    /// `MeterProvider`) before calling this method.
    ///
    /// [`MeterProvider`]: opentelemetry::metrics::MeterProvider
    #[must_use]
    pub fn build(self) -> OtelMetrics {
        let meter = opentelemetry::global::meter(self.meter_name);
        OtelMetrics::from_meter(&meter)
    }
}

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

// ── Pipeline initialisation ──────────────────────────────────────────────────

/// Initialise an OTLP metrics export pipeline and install it as the global
/// [`MeterProvider`].
///
/// This configures a periodic reader that exports metrics via gRPC to an
/// OTLP-compatible collector (e.g. the OpenTelemetry Collector, Grafana
/// Alloy, or Datadog Agent). The endpoint defaults to `http://localhost:4317`
/// and can be overridden via the `OTEL_EXPORTER_OTLP_ENDPOINT` environment
/// variable.
///
/// Returns the [`SdkMeterProvider`] so the caller can hold onto it and call
/// [`SdkMeterProvider::shutdown`] during graceful termination.
///
/// # Arguments
///
/// * `service_name` — value for the `service.name` resource attribute.
///
/// # Errors
///
/// Returns an error if the OTLP exporter or meter provider cannot be created.
///
/// [`MeterProvider`]: opentelemetry::metrics::MeterProvider
pub fn init_otlp_pipeline(
    service_name: &str,
) -> Result<SdkMeterProvider, Box<dyn std::error::Error>> {
    use opentelemetry::KeyValue as Kv;
    use opentelemetry_otlp::MetricExporter;
    use opentelemetry_sdk::metrics::PeriodicReader;
    use opentelemetry_sdk::Resource;

    let exporter = MetricExporter::builder().with_tonic().build()?;

    let reader = PeriodicReader::builder(exporter).build();

    let resource = Resource::builder()
        .with_attributes([Kv::new("service.name", service_name.to_owned())])
        .build();

    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(resource)
        .build();

    opentelemetry::global::set_meter_provider(provider.clone());

    Ok(provider)
}
