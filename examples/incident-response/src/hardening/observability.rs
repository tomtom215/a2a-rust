// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Whether anyone can tell what the agent is doing.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::metrics::Metrics;

use super::{bind, plain_card, serve, Check};
use crate::agents::LogSearchExecutor;
use crate::{send_params, user_message};

/// How many requests each observability check drives.
const CALLS: u64 = 3;

// ── The Metrics hook ─────────────────────────────────────────────────────────

/// Counts what the SDK reports, so the counts can be compared against reality.
#[derive(Debug, Default)]
struct CountingMetrics {
    requests: AtomicU64,
    responses: AtomicU64,
    latencies: AtomicU64,
}

impl Metrics for CountingMetrics {
    fn on_request(&self, _method: &str) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    fn on_response(&self, _method: &str) {
        self.responses.fetch_add(1, Ordering::Relaxed);
    }

    fn on_latency(&self, _method: &str, _duration: std::time::Duration) {
        self.latencies.fetch_add(1, Ordering::Relaxed);
    }
}

/// Every served request must reach the [`Metrics`] hook.
///
/// This is the check the OpenTelemetry one cannot be: it owns the recorder, so
/// it can read the numbers back and compare them against the number of calls it
/// made. A handler that accepts a `Metrics` implementation and never calls it
/// serves traffic perfectly and reports nothing — the exact shape of an
/// observability gap that looks fine until an incident.
pub(super) async fn metrics_hook() -> Check {
    const LABEL: &str = "Metrics hook (every served request is recorded)";

    let (listener, url) = bind().await;
    let recorder = Arc::new(CountingMetrics::default());
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Measured Agent"))
        // `Arc<T>` implements `Metrics` when `T` does, so the example keeps a
        // handle to the same recorder the handler is writing to.
        .with_metrics(Arc::clone(&recorder))
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    let client = match ClientBuilder::new(&url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(LABEL, format!("building the client: {e}")),
    };
    for n in 0..CALLS {
        if let Err(e) = client
            .send_message(send_params(user_message("payments-api")))
            .await
        {
            return Check::fail(LABEL, format!("call {} of {CALLS} failed: {e}", n + 1));
        }
    }

    let requests = recorder.requests.load(Ordering::Relaxed);
    let responses = recorder.responses.load(Ordering::Relaxed);
    let latencies = recorder.latencies.load(Ordering::Relaxed);
    for (name, seen) in [
        ("on_request", requests),
        ("on_response", responses),
        ("on_latency", latencies),
    ] {
        if seen < CALLS {
            return Check::fail(
                LABEL,
                format!("{CALLS} requests succeeded but {name} fired {seen} time(s)"),
            );
        }
    }
    Check::pass(
        LABEL,
        format!(
            "{CALLS} calls ⇒ {requests} requests, {responses} responses, {latencies} latencies"
        ),
    )
}

// ── OpenTelemetry export ─────────────────────────────────────────────────────

const OTEL_LABEL: &str = "OpenTelemetry export (a2a.server.requests reaches a reader)";

/// Drives an OTel-instrumented handler and reads the exported datapoint back.
///
/// Building `OtelMetrics` and serving a request proves nothing on its own: the
/// global meter provider defaults to a no-op, so a handler that never records
/// anything and one that records everything produce identical output. This
/// check installs a real SDK meter provider with a [`ManualReader`], drives
/// traffic, collects, and requires a non-zero `a2a.server.requests` sum — the
/// number an operator's dashboard would be reading.
///
/// [`ManualReader`]: opentelemetry_sdk::metrics::ManualReader
#[cfg(feature = "otel")]
pub(super) async fn otel_export() -> Check {
    use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData, ResourceMetrics};
    use opentelemetry_sdk::metrics::reader::MetricReader;
    use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

    /// `SdkMeterProvider::with_reader` takes ownership, so the reader is shared
    /// through a newtype that forwards the trait.
    #[derive(Debug, Clone)]
    struct SharedReader(Arc<ManualReader>);

    impl MetricReader for SharedReader {
        fn register_pipeline(
            &self,
            pipeline: std::sync::Weak<opentelemetry_sdk::metrics::Pipeline>,
        ) {
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

    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(SharedReader(Arc::clone(&reader)))
        .build();
    // `OtelMetricsBuilder` resolves its meter from the global provider, so the
    // provider has to be installed before the instruments are created.
    opentelemetry::global::set_meter_provider(provider.clone());

    let (listener, url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Instrumented Agent"))
        .with_metrics(a2a_protocol_server::otel::OtelMetricsBuilder::new().build())
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(OTEL_LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    let client = match ClientBuilder::new(&url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(OTEL_LABEL, format!("building the client: {e}")),
    };
    for n in 0..CALLS {
        if let Err(e) = client
            .send_message(send_params(user_message("payments-api")))
            .await
        {
            return Check::fail(OTEL_LABEL, format!("call {} of {CALLS} failed: {e}", n + 1));
        }
    }

    let mut collected = ResourceMetrics::default();
    if let Err(e) = reader.collect(&mut collected) {
        return Check::fail(OTEL_LABEL, format!("collecting from the reader: {e}"));
    }

    let mut names = Vec::new();
    let mut total = 0_u64;
    let mut found = false;
    for scope in collected.scope_metrics() {
        for metric in scope.metrics() {
            names.push(metric.name().to_owned());
            if metric.name() != "a2a.server.requests" {
                continue;
            }
            found = true;
            if let AggregatedMetrics::U64(MetricData::Sum(sum)) = metric.data() {
                total += sum
                    .data_points()
                    .map(super::observability::point_value)
                    .sum::<u64>();
            }
        }
    }
    provider.shutdown().ok();

    if !found {
        return Check::fail(
            OTEL_LABEL,
            format!(
                "{CALLS} requests served but no a2a.server.requests metric was exported \
                 (collected: {})",
                if names.is_empty() {
                    "nothing".to_owned()
                } else {
                    names.join(", ")
                }
            ),
        );
    }
    if total < CALLS {
        return Check::fail(
            OTEL_LABEL,
            format!("{CALLS} requests served but a2a.server.requests summed to {total}"),
        );
    }
    Check::pass(
        OTEL_LABEL,
        format!("a2a.server.requests = {total} after {CALLS} calls"),
    )
}

/// Reads the value out of a sum datapoint.
#[cfg(feature = "otel")]
fn point_value(point: &opentelemetry_sdk::metrics::data::SumDataPoint<u64>) -> u64 {
    point.value()
}

#[cfg(not(feature = "otel"))]
pub(super) async fn otel_export() -> Check {
    Check::skipped(OTEL_LABEL, "otel")
}
