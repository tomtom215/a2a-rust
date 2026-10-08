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
mod config;
mod exporters;
mod http;
mod langfuse;
mod runtime;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use opentelemetry::propagation::TextMapCompositePropagator;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::LookupSpan;

pub use builder::TelemetryBuilder;
pub use config::OtlpProtocol;
pub use langfuse::{LANGFUSE_SELF_HOSTED, Langfuse};

use config::{Env, Signal};
use exporters::Target;
use runtime::ExportRuntime;

/// The instrumentation scope this crate's spans are recorded under.
const SCOPE: &str = "a2a-protocol-server";

/// Targets whose spans and events are never exported: the exporter's own
/// stack. Exporting them would make every export produce telemetry that
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
/// Either may block for up to the export timeout per signal, and is safe on
/// any thread, inside a Tokio runtime of either flavour or outside one.
#[derive(Debug)]
pub struct Telemetry {
    tracer: Option<SdkTracerProvider>,
    meter: Option<SdkMeterProvider>,
    logger: Option<SdkLoggerProvider>,
    log_level: tracing::Level,
    /// Dropped after the providers, so their final flush still has it.
    runtime: Option<ExportRuntime>,
    /// Each signal's count of failed OTLP/HTTP exports since the last flush.
    failures: Vec<(Signal, Arc<AtomicU64>)>,
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

    /// A new failure counter for `signal`'s exporter.
    fn counter(&mut self, signal: Signal) -> Arc<AtomicU64> {
        let counter = Arc::new(AtomicU64::new(0));
        self.failures.push((signal, Arc::clone(&counter)));
        counter
    }

    /// Adds to `failed` each signal whose OTLP/HTTP exports failed since the
    /// last report — telemetry that was dropped, which the SDK's own flush
    /// and shutdown results do not say (`http.rs`).
    fn report_failures(&self, failed: &mut Vec<String>) {
        for (signal, count) in &self.failures {
            let n = count.swap(0, Ordering::Relaxed);
            if n > 0 {
                failed.push(format!(
                    "{}: {n} export(s) did not reach the collector or were refused",
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
    /// timed out, or whose OTLP/HTTP exports failed since the last report —
    /// see [`shutdown`](Self::shutdown) for what is and is not caught.
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
    /// or timed out, or whose OTLP/HTTP exports did not reach the collector
    /// or were refused since the last report: telemetry on it was lost.
    /// Termination should not be blocked on it.
    ///
    /// **Over gRPC a failed export is only logged.** The SDK's batch
    /// processors log a failed export (`BatchSpanProcessor.ExportError`) and
    /// drop the batch, and their `shutdown` returns `Ok` regardless
    /// (`opentelemetry_sdk` 0.32.1, `trace/span_processor.rs`); this crate
    /// counts failures in the HTTP client it supplies, and has no such hook
    /// in tonic's.
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
mod tests;
