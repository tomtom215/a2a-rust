// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One entry point for traces, metrics and logs (ADR 0013, option 5).
//!
//! [`Telemetry::init`] reads the `OTEL_*` environment, builds an OTLP
//! exporter per signal on the transport it names, installs the tracer and
//! meter providers globally, and returns a guard whose drop flushes all
//! three. [`Telemetry::layer`] is the `tracing` layer that turns this crate's
//! spans — and the application's own spans and events — into OpenTelemetry
//! spans and log records.
//!
//! ```rust,no_run
//! use a2a_protocol_server::otel::Telemetry;
//! use tracing_subscriber::layer::SubscriberExt as _;
//! use tracing_subscriber::util::SubscriberInitExt as _;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let telemetry = Telemetry::builder()
//!     .with_default_service_name("my-agent") // OTEL_SERVICE_NAME wins
//!     .build()?;
//! tracing_subscriber::registry().with(telemetry.layer()).init();
//!
//! // ... serve, with `.with_metrics(telemetry.otel_metrics())` on the
//! // handler builder ...
//!
//! telemetry.shutdown()?; // or let it drop
//! # Ok(())
//! # }
//! ```
//!
//! It can be called from anywhere — a plain `fn main`, a `current_thread`
//! runtime, a test — because export runs on a private runtime of its own
//! (`runtime.rs` says why); and with `OTEL_SDK_DISABLED=true` it builds
//! nothing and the layer records nothing.

mod builder;
mod compression;
mod config;
mod console;
mod console_format;
mod counting;
mod detect;
mod diagnostics;
mod error;
mod exporters;
mod hints;
mod http;
mod langfuse;
mod process;
#[cfg(feature = "otel-prometheus")]
mod prometheus;
mod propagators;
mod resource_env;
mod runtime;
mod selection;
mod self_metrics;
mod subscriber;
mod upstream;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;

pub use builder::TelemetryBuilder;
pub use compression::OtlpCompression;
pub use config::OtlpProtocol;
pub use console_format::ConsoleFormat;
pub use error::TelemetryError;
pub use langfuse::{LANGFUSE_SELF_HOSTED, Langfuse};
#[cfg(feature = "otel-prometheus")]
pub use prometheus::PrometheusScrape;

use crate::net::proxy::{self, ProxySetting};
use config::{Env, Signal};
use counting::Counting;
use exporters::{ClientIdentity, Target};
use runtime::ExportRuntime;

/// The instrumentation scope this crate's spans are recorded under.
const SCOPE: &str = "a2a-protocol-server";

/// Targets whose spans and events are never exported: the exporter's own
/// stack, and this module's own diagnostics (a lost export, an ignored proxy). Exporting them would make every export produce telemetry that
/// needs exporting, and an export failure would log a record whose export
/// fails. They still reach the application's other layers — a `fmt` layer
/// still shows an export error.
const SELF_TARGETS: &[&str] = &[
    "opentelemetry",
    "opentelemetry_sdk",
    "opentelemetry_otlp",
    "opentelemetry_http",
    "hyper",
    "hyper_util",
    "h2",
    "tonic",
    "tower",
    "rustls",
    "a2a_protocol_server::otel::telemetry",
];

fn is_self_target(target: &str) -> bool {
    SELF_TARGETS.iter().any(|t| {
        target
            .strip_prefix(t)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    })
}

/// Exporting providers for traces, metrics and logs, and the guard that
/// flushes them. Built by [`Telemetry::init`] or [`Telemetry::builder`].
///
/// Dropping it flushes and shuts every provider down, best-effort;
/// [`shutdown`](Self::shutdown) does the same and reports what it can see
/// failed.
/// Either is safe on any thread, inside a Tokio runtime of either flavour or
/// outside one, and may block while a collector is slow: each of the three
/// providers is given the SDK's 5 s shutdown timeout (`opentelemetry_sdk`
/// 0.33.1), one after the other. Measured on `hello-agent` after `SIGTERM`
/// (debug build, one run each): 0.1 s to a collector that answers, 1.8 s to a
/// port that refuses, 15.0 s to one that accepts the connection and never
/// answers, 4.1 s to that one with `OTEL_EXPORTER_OTLP_TIMEOUT=2000`.
///
/// `#[must_use]`: `Telemetry::init()?;` alone on a line drops the guard at
/// once, which shuts every exporter down before anything is recorded (audit
/// G-14). Keep it for as long as the process should export.
#[derive(Debug)]
#[must_use = "dropping `Telemetry` shuts every exporter down; keep it until the process exits"]
pub struct Telemetry {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    logger: Option<SdkLoggerProvider>,
    log_level: tracing::Level,
    span_level: tracing::Level,
    /// The default service name as a `tracing` target, whose INFO lines the
    /// default filter prints.
    app_target: Option<String>,
    /// Dropped after the providers, so their final flush still has it.
    runtime: Option<ExportRuntime>,
    /// Each signal's count of lost exports since the last report
    /// (`counting.rs`).
    failures: Vec<(Signal, Arc<AtomicU64>)>,
    /// The same, never reset, for `a2a.telemetry.export.failed` (audit C-13).
    failed_totals: Vec<(Signal, Arc<AtomicU64>)>,
    /// `OTEL_SDK_DISABLED` was set: nothing is built or exported. The printing
    /// layer is still installed (audit C-07).
    disabled: bool,
    /// Whether `install_subscriber` logs [`describe`](Self::describe).
    startup_summary: bool,
    /// What `install_subscriber` prints to standard error.
    console_format: ConsoleFormat,
    /// Log records go to standard output (`OTEL_LOGS_EXPORTER=console`), so
    /// `install_subscriber` need not print them again on standard error.
    logs_on_console: bool,
    /// One entry per signal: where it goes, or `off` ([`describe`](Self::describe)).
    report: Vec<String>,
    /// What the configuration asked for that this SDK does not do and carried
    /// on without ([`notices`](Self::notices)).
    notices: Vec<String>,
    /// Where the Prometheus endpoint listens ([`prometheus_addr`](Self::prometheus_addr)).
    #[cfg(feature = "otel-prometheus")]
    prometheus: Option<std::net::SocketAddr>,
    /// The same text, for the application to serve ([`prometheus`](Self::prometheus)).
    #[cfg(feature = "otel-prometheus")]
    prometheus_scrape: Option<prometheus::PrometheusScrape>,
}

impl Telemetry {
    /// Configuration with every default; see [`TelemetryBuilder`].
    #[must_use]
    pub fn builder() -> TelemetryBuilder {
        TelemetryBuilder::new(Arc::new(config::process_env))
    }

    /// Every signal over OTLP, configured by the environment alone: the same
    /// as [`Telemetry::builder`] followed by [`TelemetryBuilder::build`].
    ///
    /// # Errors
    ///
    /// As [`TelemetryBuilder::build`].
    ///
    /// Without an endpoint in the environment, every signal is exported to
    /// `localhost` (4318, or 4317 over gRPC), as the OpenTelemetry environment
    /// specification has it — what a deployment with a collector beside it
    /// (a sidecar) and no `OTEL_*` variable relies on. A binary that must run
    /// as well without a collector wants
    /// [`init_where_configured`](Self::init_where_configured).
    pub fn init() -> Result<Self, TelemetryError> {
        Self::builder().build()
    }

    /// Like [`init`](Self::init), but a signal is built only where an endpoint
    /// is configured for it: the same as [`Telemetry::builder`] followed by
    /// [`TelemetryBuilder::with_export_where_configured`] and
    /// [`TelemetryBuilder::build`]. With no `OTEL_EXPORTER_OTLP_*ENDPOINT` set,
    /// nothing is exported and no export thread is started.
    ///
    /// # Errors
    ///
    /// As [`TelemetryBuilder::build`].
    pub fn init_where_configured() -> Result<Self, TelemetryError> {
        Self::builder().with_export_where_configured().build()
    }

    /// An [`OtelMetrics`](super::OtelMetrics) recording through this
    /// telemetry's meter provider, for `RequestHandlerBuilder::with_metrics`.
    /// With metrics off it records into a no-op meter.
    #[must_use]
    pub fn otel_metrics(&self) -> super::OtelMetrics {
        use opentelemetry::metrics::MeterProvider as _;
        let meter = self.meter.as_ref().map_or_else(
            || opentelemetry::metrics::noop::NoopMeterProvider::new().meter("a2a.server"),
            |provider| provider.meter("a2a.server"),
        );
        super::OtelMetrics::from_meter(&meter)
    }

    /// The tracer provider, when traces are exported.
    #[must_use]
    pub const fn tracer_provider(&self) -> Option<&SdkTracerProvider> {
        self.tracer.as_ref()
    }

    /// The meter provider, when metrics are exported.
    #[must_use]
    pub const fn meter_provider(&self) -> Option<&SdkMeterProvider> {
        self.meter.as_ref()
    }

    /// The logger provider, when logs are exported.
    #[must_use]
    pub const fn logger_provider(&self) -> Option<&SdkLoggerProvider> {
        self.logger.as_ref()
    }

    /// `exporter`, counting its lost exports into a new counter for
    /// `signal` that [`report_failures`](Self::report_failures) reads, and
    /// saying so, now and then, while they keep being lost.
    fn counting<E>(&mut self, signal: Signal, exporter: E, target: &Target) -> Counting<E> {
        let counter = Arc::new(AtomicU64::new(0));
        self.failures.push((signal, Arc::clone(&counter)));
        let total = Arc::new(AtomicU64::new(0));
        self.failed_totals.push((signal, Arc::clone(&total)));
        Counting::new(exporter, counter)
            .with_total(total)
            .reporting(
                signal,
                &target.destination,
                hints::Route::of(target.protocol, &target.destination),
                target.proxy_display(),
                Arc::clone(&target.cause),
            )
    }

    /// The address `/metrics` is served on under
    /// `OTEL_METRICS_EXPORTER=prometheus` — the port the operating system
    /// chose when `OTEL_EXPORTER_PROMETHEUS_PORT` is `0`.
    #[cfg(feature = "otel-prometheus")]
    #[must_use]
    pub const fn prometheus_addr(&self) -> Option<std::net::SocketAddr> {
        self.prometheus
    }

    /// Under `OTEL_METRICS_EXPORTER=prometheus`, a handle that renders the
    /// metrics in the Prometheus text format, for the application to serve at
    /// `/metrics` on its own port, behind its own authentication; with
    /// [`TelemetryBuilder::without_prometheus_listener`] it is the only way
    /// out (audit C-16, ADR 0038 §3). `None` when that exporter is not chosen.
    #[cfg(feature = "otel-prometheus")]
    #[must_use]
    pub fn prometheus(&self) -> Option<prometheus::PrometheusScrape> {
        self.prometheus_scrape.clone()
    }

    /// What the environment or the code asked for that this SDK does not do,
    /// and went on without: an exporter name, protocol or compression it does
    /// not have, a timeout or port it could not read, a propagator it does not
    /// have, an `OTEL_SDK_DISABLED` that is not `true` or `false`. Each line
    /// names the variable and its value and says what was used instead. Each is
    /// also logged at `warn` by [`install_subscriber`](Self::install_subscriber);
    /// an application with its own subscriber reads them here. Until
    /// 2026-11-01 most of these stopped [`build`](TelemetryBuilder::build).
    #[must_use]
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// Where each signal goes, in one line: the transport, the endpoint
    /// (with any login removed), gzip, the proxy it is sent through, or
    /// `off`; or why nothing is exported at all. `install_subscriber` logs it
    /// at startup; a binary with its own subscriber can log it too.
    #[must_use]
    pub fn describe(&self) -> String {
        if self.disabled {
            return "telemetry: disabled by OTEL_SDK_DISABLED".to_owned();
        }
        if self.tracer.is_none() && self.meter.is_none() && self.logger.is_none() {
            return "telemetry: nothing is exported; set OTEL_EXPORTER_OTLP_ENDPOINT to send \
                    it to a collector, or OTEL_TRACES_EXPORTER=console to print spans"
                .to_owned();
        }
        format!("telemetry: {}", self.report.join("; "))
    }

    /// Adds to `failed` each signal with exports lost since the last report
    /// — telemetry that was dropped, which the SDK's own shutdown result does
    /// not say (`counting.rs`).
    fn report_failures(&self, failed: &mut Vec<String>) {
        for (signal, count) in &self.failures {
            let n = count.swap(0, Ordering::Relaxed);
            if n > 0 {
                failed.push(format!(
                    "{}: {n} export(s) failed after retrying and were dropped",
                    signal.name()
                ));
            }
        }
    }

    /// Exports everything recorded so far, without shutting down.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::Export`] naming each signal whose flush failed or
    /// timed out, or whose exports were lost since the last report — see
    /// [`shutdown`](Self::shutdown).
    pub fn force_flush(&self) -> Result<(), TelemetryError> {
        let mut failed = Vec::new();
        if let Some(p) = &self.tracer
            && let Err(e) = p.force_flush()
        {
            failed.push(format!("traces: {e}"));
        }
        if let Some(p) = &self.meter
            && let Err(e) = p.force_flush()
        {
            failed.push(format!("metrics: {e}"));
        }
        if let Some(p) = &self.logger
            && let Err(e) = p.force_flush()
        {
            failed.push(format!("logs: {e}"));
        }
        self.report_failures(&mut failed);
        if failed.is_empty() {
            Ok(())
        } else {
            Err(TelemetryError::Export(failed))
        }
    }

    /// Flushes and shuts down every provider, then the export runtime.
    ///
    /// # Errors
    ///
    /// [`TelemetryError::Export`] naming each signal whose shutdown failed
    /// or timed out, or whose exports were lost since the last report, on
    /// either transport: an export the exporter gave up on after its retries
    /// (`opentelemetry-otlp` makes four attempts by default), whose batch the
    /// SDK then dropped. An attempt refused and then retried successfully is
    /// not a loss. Termination should not be blocked on it.
    ///
    /// The SDK's batch processors log a lost export and their `shutdown`
    /// returns `Ok` regardless (`opentelemetry_sdk` 0.33.1,
    /// `trace/span_processor.rs`); the count is this crate's.
    pub fn shutdown(mut self) -> Result<(), TelemetryError> {
        self.shutdown_inner()
    }

    fn shutdown_inner(&mut self) -> Result<(), TelemetryError> {
        let mut failed = Vec::new();
        if let Some(p) = self.tracer.take()
            && let Err(e) = p.shutdown()
        {
            failed.push(format!("traces: {e}"));
        }
        if let Some(p) = self.meter.take()
            && let Err(e) = p.shutdown()
        {
            failed.push(format!("metrics: {e}"));
        }
        if let Some(p) = self.logger.take()
            && let Err(e) = p.shutdown()
        {
            failed.push(format!("logs: {e}"));
        }
        drop(self.runtime.take());
        self.report_failures(&mut failed);
        if failed.is_empty() {
            Ok(())
        } else {
            Err(TelemetryError::Export(failed))
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        let _ = self.shutdown_inner();
    }
}

#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;
