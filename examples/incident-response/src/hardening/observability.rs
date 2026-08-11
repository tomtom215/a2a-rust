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
                total += sum.data_points().map(point_value).sum::<u64>();
            }
        }
    }
    // Best-effort: the datapoint has already been collected, and a failure to
    // shut the provider down would say nothing about whether it was exported.
    let _ = provider.shutdown();

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

// ── The bundled OTLP pipeline ────────────────────────────────────────────────

const OTLP_LABEL: &str = "OTLP pipeline (bytes reach the configured collector)";

/// Points `init_otlp_pipeline` at a socket this example owns and requires the
/// exporter to actually connect and transmit.
///
/// [`otel_export`] checks the *instrumentation* — that requests reach the
/// meter. It says nothing about the **export** path, which is the half a
/// deployment depends on and the half that involves a network: a wrong
/// endpoint, a channel that never dials, an exporter that silently swallows
/// its errors all leave the in-process reader looking perfect.
///
/// The collector here is a bare TCP listener rather than a real OTLP receiver,
/// because the assertion does not need one. What is being established is that
/// the pipeline opened a connection to the endpoint it was configured with and
/// pushed a payload: the HTTP/2 connection preface arrives verbatim (it is sent
/// before any HPACK state exists), so its presence proves a real HTTP/2 client
/// dialled, and bytes beyond it prove frames followed. A pipeline that built
/// cleanly and exported nothing delivers zero.
#[cfg(feature = "otel")]
pub(super) async fn otlp_pipeline() -> Check {
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    use tokio::io::AsyncReadExt as _;

    /// Sent verbatim by every HTTP/2 client before its first frame (RFC 9113
    /// §3.4), so it survives having no HPACK decoder on this side.
    const H2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

    let listener = match tokio::net::TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(e) => return Check::fail(OTLP_LABEL, format!("binding a collector socket: {e}")),
    };
    let endpoint = match listener.local_addr() {
        Ok(addr) => format!("http://{addr}"),
        Err(e) => return Check::fail(OTLP_LABEL, format!("reading the collector address: {e}")),
    };

    let received = Arc::new(AtomicUsize::new(0));
    let saw_preface = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let received = Arc::clone(&received);
        let saw_preface = Arc::clone(&saw_preface);
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    continue;
                };
                let received = Arc::clone(&received);
                let saw_preface = Arc::clone(&saw_preface);
                tokio::spawn(async move {
                    let mut first = true;
                    let mut buf = vec![0_u8; 8192];
                    while let Ok(n) = stream.read(&mut buf).await {
                        if n == 0 {
                            break;
                        }
                        if first && buf[..n].starts_with(H2_PREFACE) {
                            saw_preface.store(true, Ordering::SeqCst);
                        }
                        first = false;
                        received.fetch_add(n, Ordering::SeqCst);
                    }
                });
            }
        });
    }

    // `MetricExporter` reads its endpoint from the environment. Set before the
    // pipeline is built; the checks run sequentially and nothing else in this
    // process reads the variable, so the process-global write is contained.
    std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", &endpoint);
    let provider = match a2a_protocol_server::otel::init_otlp_pipeline("incident-response") {
        Ok(provider) => provider,
        Err(e) => return Check::fail(OTLP_LABEL, format!("building the pipeline: {e}")),
    };

    let (listener, url) = bind().await;
    let handler = match RequestHandlerBuilder::new(LogSearchExecutor)
        .with_agent_card(plain_card(&url, "Exported Agent"))
        .with_metrics(a2a_protocol_server::otel::OtelMetricsBuilder::new().build())
        .build()
    {
        Ok(handler) => Arc::new(handler),
        Err(e) => return Check::fail(OTLP_LABEL, format!("building the handler: {e}")),
    };
    serve(listener, handler);

    let client = match ClientBuilder::new(&url).build() {
        Ok(client) => client,
        Err(e) => return Check::fail(OTLP_LABEL, format!("building the client: {e}")),
    };
    for n in 0..CALLS {
        if let Err(e) = client
            .send_message(send_params(user_message("payments-api")))
            .await
        {
            return Check::fail(OTLP_LABEL, format!("call {} of {CALLS} failed: {e}", n + 1));
        }
    }

    // The periodic reader would otherwise wait out its interval. The flush is
    // expected to *fail* — nothing here speaks OTLP back — and that is fine:
    // the bytes have already left, which is the whole assertion.
    let _ = tokio::time::timeout(Duration::from_secs(10), async { provider.force_flush() }).await;
    // The write is asynchronous on the collector side; give the reader a moment
    // to drain what the exporter already put on the wire.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = provider.shutdown();

    let bytes = received.load(Ordering::SeqCst);
    if bytes == 0 {
        return Check::fail(
            OTLP_LABEL,
            format!("{CALLS} requests recorded but the collector at {endpoint} received 0 bytes"),
        );
    }
    if !saw_preface.load(Ordering::SeqCst) {
        return Check::fail(
            OTLP_LABEL,
            format!("{bytes} bytes reached {endpoint} but not as HTTP/2 — no connection preface"),
        );
    }
    if bytes <= H2_PREFACE.len() {
        return Check::fail(
            OTLP_LABEL,
            format!("only the {bytes}-byte HTTP/2 preface arrived — the exporter connected but sent no frames"),
        );
    }
    Check::pass(
        OTLP_LABEL,
        format!("{bytes} bytes of HTTP/2 exported to the configured collector"),
    )
}

#[cfg(not(feature = "otel"))]
pub(super) async fn otlp_pipeline() -> Check {
    Check::skipped(OTLP_LABEL, "otel")
}
