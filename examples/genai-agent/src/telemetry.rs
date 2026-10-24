// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SDK's telemetry for this process, from the standard `OTEL_*`
//! environment, set up as in `examples/langfuse-agent`.

/// With an OTLP endpoint configured for a signal
/// (`OTEL_EXPORTER_OTLP_ENDPOINT`, or that signal's own
/// `OTEL_EXPORTER_OTLP_<SIGNAL>_ENDPOINT`), the signal goes there; without one
/// nothing is built or installed — no exporter, no export thread, no
/// subscriber. Keep what this returns for the life of the process: dropping it
/// flushes.
pub fn init() -> a2a_protocol_server::otel::Telemetry {
    let telemetry = a2a_protocol_server::otel::Telemetry::builder()
        .with_default_service_name(env!("CARGO_BIN_NAME"))
        .with_export_where_configured()
        .build()
        .expect("the OTEL_* environment names a configuration Telemetry cannot use");
    telemetry.install_subscriber();
    telemetry
}
