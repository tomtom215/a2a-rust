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
mod counting;
mod detect;
mod diagnostics;
mod exporters;
mod hints;
mod http;
mod langfuse;
#[cfg(feature = "otel-prometheus")]
mod prometheus;
mod propagators;
mod proxy;
mod resource_env;
mod runtime;
mod selection;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;

pub use builder::TelemetryBuilder;
pub use compression::OtlpCompression;
pub use config::OtlpProtocol;
pub use langfuse::{LANGFUSE_SELF_HOSTED, Langfuse};

use config::{Env, Signal};
use counting::Counting;
use exporters::{ClientIdentity, Target};
use proxy::ProxySetting;
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
#[derive(Debug)]
pub struct Telemetry {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    logger: Option<SdkLoggerProvider>,
    log_level: tracing::Level,
    /// Dropped after the providers, so their final flush still has it.
    runtime: Option<ExportRuntime>,
    /// Each signal's count of lost exports since the last report
    /// (`counting.rs`).
    failures: Vec<(Signal, Arc<AtomicU64>)>,
    /// `OTEL_SDK_DISABLED` was set: nothing is built, nothing installed.
    disabled: bool,
    /// Whether `install_subscriber` logs [`describe`](Self::describe).
    startup_summary: bool,
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
    pub fn init() -> Result<Self, TelemetryError> {
        Self::builder().build()
    }

    /// The `tracing` layer: spans become OpenTelemetry spans, and events at
    /// or above the log level become log records. Add it to the
    /// application's subscriber, next to any other layers. A signal that is
    /// off contributes nothing.
    #[must_use]
    pub fn layer<S>(&self) -> impl Layer<S> + Send + Sync + 'static
    where
        S: tracing::Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
    {
        use tracing_subscriber::filter::filter_fn;
        use tracing_subscriber::layer::Identity;
        // A signal that is off contributes `Identity`, never `None`:
        // `Option<Layer>::max_level_hint` is `OFF` for `None`
        // (tracing-subscriber 0.3.23, `layer/mod.rs`), and combined with the
        // other half that switched every span off process-wide — traces with
        // logs off (the Langfuse preset) recorded nothing.
        let spans: Box<dyn Layer<S> + Send + Sync> = match &self.tracer {
            Some(provider) => Box::new(
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer(SCOPE))
                    .with_filter(filter_fn(|meta| !is_self_target(meta.target()))),
            ),
            None => Box::new(Identity::new()),
        };
        let level = self.log_level;
        let logs: Box<dyn Layer<S> + Send + Sync> = match &self.logger {
            Some(provider) => Box::new(
                opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(provider)
                    .with_filter(filter_fn(move |meta| {
                        meta.is_event() && *meta.level() <= level && !is_self_target(meta.target())
                    })),
            ),
            None => Box::new(Identity::new()),
        };
        spans.and_then(logs)
    }

    /// Installs [`layer`](Self::layer) as the process's global `tracing`
    /// subscriber, with a layer that prints warnings and errors to standard
    /// error and logs [`describe`](Self::describe) once — what a binary with
    /// no subscriber of its own needs. Without the printing layer a process
    /// whose export is failing, or that was never given a collector, shows
    /// nothing at all.
    ///
    /// `RUST_LOG` sets what is printed, in `target=level` form (default: `warn`
    /// everywhere, `info` for this module's own startup line). Installs
    /// nothing under `OTEL_SDK_DISABLED=true`. Returns whether it installed a
    /// subscriber: `false` also when a global subscriber was already set,
    /// which this never replaces. A process with its own subscriber adds
    /// `layer()` to it instead.
    // Called for its effect; the result is for a caller that cares.
    #[allow(clippy::must_use_candidate)]
    pub fn install_subscriber(&self) -> bool {
        use tracing_subscriber::layer::SubscriberExt as _;
        use tracing_subscriber::util::SubscriberInitExt as _;
        if self.disabled {
            return false;
        }
        let (filter, not_understood) =
            diagnostics::filter(std::env::var("RUST_LOG").ok().as_deref());
        let installed = tracing_subscriber::registry()
            .with(self.layer())
            .with(diagnostics::layer(filter, self.logs_on_console))
            .try_init()
            .is_ok();
        if installed {
            if self.startup_summary {
                tracing::info!("{}", self.describe());
            }
            for notice in not_understood.iter().chain(&self.notices) {
                tracing::warn!("{notice}");
            }
        }
        installed
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
        Counting::new(exporter, counter).reporting(
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

    /// What the environment or the code asked for that this SDK does not do,
    /// and went on without: a propagator it does not have, for one. Each is
    /// also logged at `warn` by [`install_subscriber`](Self::install_subscriber);
    /// an application with its own subscriber reads them here.
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

/// Why [`Telemetry`] could not be built, or lost telemetry.
#[derive(Debug)]
#[non_exhaustive]
pub enum TelemetryError {
    /// A variable, or a value given in code, this crate cannot act on.
    Config {
        /// The variable or setting.
        variable: String,
        /// Its value.
        value: String,
        /// What is wrong with it.
        reason: String,
    },
    /// `opentelemetry-otlp` refused to build an exporter.
    Exporter {
        /// `traces`, `metrics` or `logs`.
        signal: &'static str,
        /// Its error.
        source: opentelemetry_otlp::ExporterBuildError,
    },
    /// The TLS configuration could not be built.
    Tls(String),
    /// The export thread could not be started.
    Runtime(std::io::Error),
    /// A flush or shutdown failed, per signal.
    Export(Vec<String>),
}

impl TelemetryError {
    fn config(variable: &str, value: &str, reason: impl Into<String>) -> Self {
        Self::Config {
            variable: variable.to_owned(),
            value: value.to_owned(),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for TelemetryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config {
                variable,
                value,
                reason,
            } if value.is_empty() => {
                write!(f, "{variable}: {reason}")
            }
            Self::Config {
                variable,
                value,
                reason,
            } => write!(f, "{variable}={value:?}: {reason}"),
            Self::Exporter { signal, source } => write!(f, "{signal} exporter: {source}"),
            Self::Tls(reason) => write!(f, "OTLP TLS: {reason}"),
            Self::Runtime(e) => write!(f, "OTLP export thread: {e}"),
            Self::Export(failed) => write!(f, "OTLP export failed: {}", failed.join("; ")),
        }
    }
}

impl std::error::Error for TelemetryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Exporter { source, .. } => Some(source),
            Self::Runtime(e) => Some(e),
            _ => None,
        }
    }
}

#[cfg(test)]
mod notice_tests;
#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;
