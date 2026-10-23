// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SDK's telemetry for this process, from the standard `OTEL_*`
//! environment, set up as in `examples/langfuse-agent`.

/// With `OTEL_EXPORTER_OTLP_ENDPOINT` naming a collector, traces, metrics and
/// logs go there; without it nothing is built or installed — no exporter, no
/// export thread, no subscriber. Keep what this returns for the life of the
/// process: dropping it flushes.
pub fn init() -> a2a_protocol_server::otel::Telemetry {
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;
    let export = std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some();
    let telemetry = a2a_protocol_server::otel::Telemetry::builder()
        .with_default_service_name(env!("CARGO_BIN_NAME"))
        .with_traces(export)
        .with_metrics(export)
        .with_logs(export)
        .build()
        .expect("the OTEL_* environment names a configuration Telemetry cannot use");
    if telemetry.tracer_provider().is_some() || telemetry.logger_provider().is_some() {
        tracing_subscriber::registry().with(telemetry.layer()).init();
    }
    telemetry
}
