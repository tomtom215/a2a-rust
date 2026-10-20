// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The Rust worker agent, as a process the coordinator can dial.
//!
//! Starts next to the four `itk/agents/` workers and answers the same way:
//!
//! ```bash
//! cargo run -p multi-lang-team --bin rust-worker &
//! cargo run -p multi-lang-team
//! ```
//!
//! Listens on `127.0.0.1:9104` unless `RUST_WORKER_ADDR` names another
//! `host:port`. The agent itself is in `../worker.rs`, shared with the
//! coordinator's tests so the fan-out is tested against this exact executor.

#[path = "../worker.rs"]
mod worker;

/// Environment variable that overrides [`worker::DEFAULT_ADDR`].
const ADDR_ENV: &str = "RUST_WORKER_ADDR";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _telemetry = telemetry();
    let addr = std::env::var(ADDR_ENV).unwrap_or_else(|_| worker::DEFAULT_ADDR.to_owned());
    let bound = worker::start(&addr).await?;
    println!("Rust worker agent listening on http://{bound}");
    println!(
        "Replies to every message with \"{}<text>\"; Ctrl+C to stop.",
        worker::REPLY_PREFIX
    );
    tokio::signal::ctrl_c().await?;
    Ok(())
}

/// The SDK's telemetry from the standard `OTEL_*` environment, set up as in
/// `examples/langfuse-agent`: with `OTEL_EXPORTER_OTLP_ENDPOINT` naming a
/// collector, traces, metrics and logs go there; without it nothing is built
/// or installed — no exporter, no export thread, no subscriber. Keep it for
/// the life of the process: dropping it flushes.
fn telemetry() -> a2a_protocol_server::otel::Telemetry {
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
        tracing_subscriber::registry()
            .with(telemetry.layer())
            .init();
    }
    telemetry
}
