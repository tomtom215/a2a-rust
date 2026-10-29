// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SDK's telemetry for this process (the `otel` feature) and its log
//! output (the `tracing` feature), on one `tracing` subscriber.

/// What [`init`] returns: keep it for the life of the process.
#[cfg(feature = "otel")]
pub type Guard = a2a_protocol_server::otel::Telemetry;
/// What [`init`] returns without the `otel` feature: nothing to keep.
#[cfg(not(feature = "otel"))]
#[derive(Debug)]
pub struct Guard;

/// The SDK's telemetry from the standard `OTEL_*` environment, set up as in
/// `examples/langfuse-agent`: with an OTLP endpoint configured for a signal
/// (`OTEL_EXPORTER_OTLP_ENDPOINT`, or that signal's own
/// `OTEL_EXPORTER_OTLP_<SIGNAL>_ENDPOINT`), the signal goes there; without one
/// nothing is built — no exporter, no export thread — and no subscriber is
/// installed unless the `tracing` feature's log output wants one. Dropping the
/// guard flushes.
pub fn init() -> Guard {
    #[cfg(feature = "otel")]
    let telemetry = {
        a2a_protocol_server::otel::Telemetry::builder()
            .with_default_service_name(env!("CARGO_BIN_NAME"))
            .with_export_where_configured()
            .build()
            // A configuration it cannot use stops the process before it serves,
            // with the variable and the reason, rather than a panic's backtrace.
            .unwrap_or_else(|e| {
                eprintln!(
                    "error: the OTEL_* environment names a configuration Telemetry cannot use: {e}"
                );
                std::process::exit(2)
            })
    };
    #[cfg(any(feature = "otel", feature = "tracing"))]
    {
        use tracing_subscriber::layer::SubscriberExt as _;
        use tracing_subscriber::util::SubscriberInitExt as _;
        let subscriber = tracing_subscriber::registry();
        #[cfg(feature = "otel")]
        let (subscriber, exporting) = (
            subscriber.with(telemetry.layer()),
            telemetry.tracer_provider().is_some() || telemetry.logger_provider().is_some(),
        );
        #[cfg(not(feature = "otel"))]
        let exporting = false;
        #[cfg(feature = "tracing")]
        let subscriber = subscriber.with({
            use tracing_subscriber::{EnvFilter, Layer as _};
            tracing_subscriber::fmt::layer().with_filter(
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
            )
        });
        if exporting || cfg!(feature = "tracing") {
            subscriber.init();
        }
    }
    #[cfg(feature = "otel")]
    return telemetry;
    #[cfg(not(feature = "otel"))]
    Guard
}

/// Exports what `guard` holds, before a `std::process::exit` that skips its
/// drop.
#[cfg_attr(
    not(feature = "otel"),
    allow(clippy::missing_const_for_fn, unused_variables)
)]
pub fn flush(guard: &Guard) {
    #[cfg(feature = "otel")]
    let _ = guard.force_flush();
}
