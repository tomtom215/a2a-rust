// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Whether anyone can tell what the agent is doing.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::metrics::Metrics;

use super::{Check, bind, plain_card, serve};
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

#[cfg(feature = "otel")]
mod meter {
    //! What the two OpenTelemetry checks, and the test of them, share.

    use std::sync::Arc;

    use opentelemetry_sdk::metrics::ManualReader;
    use opentelemetry_sdk::metrics::data::ResourceMetrics;
    use opentelemetry_sdk::metrics::reader::MetricReader;

    /// `SdkMeterProvider::with_reader` takes ownership, so the reader is shared
    /// through a newtype that forwards the trait.
    #[derive(Debug, Clone)]
    pub(super) struct SharedReader(pub(super) Arc<ManualReader>);

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

    /// The provider that was global, handed back to `set_meter_provider`,
    /// which wants a `MeterProvider` and is given an `Arc<dyn MeterProvider>`.
    struct Previous(Arc<dyn opentelemetry::metrics::MeterProvider + Send + Sync>);

    impl opentelemetry::metrics::MeterProvider for Previous {
        fn meter_with_scope(
            &self,
            scope: opentelemetry::InstrumentationScope,
        ) -> opentelemetry::metrics::Meter {
            self.0.meter_with_scope(scope)
        }
    }

    /// Puts the process's global meter provider back as it was when dropped.
    ///
    /// Both checks install a provider of their own and shut it down, and the
    /// demo goes on serving. Handlers on the default metrics follow whichever
    /// provider is global at each measurement, so until 2026-10-24 they
    /// recorded into a provider already shut down, and the export `Telemetry`
    /// was configured for lost everything after this act.
    pub(super) struct RestoreGlobalMeterProvider(
        Arc<dyn opentelemetry::metrics::MeterProvider + Send + Sync>,
    );

    impl RestoreGlobalMeterProvider {
        pub(super) fn capture() -> Self {
            Self(opentelemetry::global::meter_provider())
        }
    }

    impl Drop for RestoreGlobalMeterProvider {
        fn drop(&mut self) {
            opentelemetry::global::set_meter_provider(Previous(Arc::clone(&self.0)));
        }
    }
}

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

    let _restore = meter::RestoreGlobalMeterProvider::capture();
    let reader = Arc::new(ManualReader::builder().build());
    let provider = SdkMeterProvider::builder()
        .with_reader(meter::SharedReader(Arc::clone(&reader)))
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

    let _restore = meter::RestoreGlobalMeterProvider::capture();
    // The endpoint is passed explicitly rather than through
    // `OTEL_EXPORTER_OTLP_ENDPOINT`: writing the environment is `unsafe` in
    // edition 2024 because other threads of this process may be reading it.
    let provider = match a2a_protocol_server::otel::init_otlp_pipeline_with_endpoint(
        Some("incident-response"),
        &endpoint,
    ) {
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
            format!(
                "only the {bytes}-byte HTTP/2 preface arrived — the exporter connected but sent no frames"
            ),
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

#[cfg(all(test, feature = "otel"))]
mod tests {
    use opentelemetry_sdk::metrics::data::ResourceMetrics;
    use opentelemetry_sdk::metrics::reader::MetricReader as _;
    use opentelemetry_sdk::metrics::{ManualReader, SdkMeterProvider};

    use super::*;

    /// Both OpenTelemetry checks install a provider of their own and shut it
    /// down; the demo then keeps serving. The provider that was global before
    /// them must be global after, or everything the demo records from then on
    /// goes to one already shut down (wiring audit, 2026-10-24). A provider
    /// standing in for the one `Telemetry` installs; a measurement made after
    /// both checks must reach its reader.
    /// Multi-threaded, as the demo is: the OTLP check flushes with a blocking
    /// call while the listener it exports to runs on the same runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_checks_leave_the_global_meter_provider_as_they_found_it() {
        let reader = Arc::new(ManualReader::builder().build());
        let before = SdkMeterProvider::builder()
            .with_reader(meter::SharedReader(Arc::clone(&reader)))
            .build();
        opentelemetry::global::set_meter_provider(before.clone());

        for check in [otel_export().await, otlp_pipeline().await] {
            assert!(!check.failed(), "{}", check.label);
        }

        opentelemetry::global::meter("restore-probe")
            .u64_counter("restore.probe")
            .build()
            .add(1, &[]);
        let mut collected = ResourceMetrics::default();
        reader.collect(&mut collected).expect("collect");
        let names: Vec<_> = collected
            .scope_metrics()
            .flat_map(opentelemetry_sdk::metrics::data::ScopeMetrics::metrics)
            .map(|m| m.name().to_owned())
            .collect();
        assert!(
            names.iter().any(|n| n == "restore.probe"),
            "a measurement after the checks did not reach the provider that was global before them: {names:?}"
        );
    }
}
