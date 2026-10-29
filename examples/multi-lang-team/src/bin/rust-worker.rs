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
        "Replies to every message with \"{}<text>\"; Ctrl+C or SIGTERM to stop.",
        worker::REPLY_PREFIX
    );
    a2a_protocol_server::shutdown_signal().await;
    Ok(())
}

/// The SDK's telemetry from the standard `OTEL_*` environment, set up as in
/// `examples/hello-agent`: with an OTLP endpoint configured for a signal
/// (`OTEL_EXPORTER_OTLP_ENDPOINT`, or that signal's own
/// `OTEL_EXPORTER_OTLP_<SIGNAL>_ENDPOINT`), the signal goes there; without one
/// no exporter and no export thread are built; the
/// subscriber still prints warnings and errors, and one line saying where
/// each signal goes. Keep it for the life of the process: dropping it flushes.
fn telemetry() -> a2a_protocol_server::otel::Telemetry {
    let telemetry = a2a_protocol_server::otel::Telemetry::builder()
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
        });
    telemetry.install_subscriber();
    telemetry
}
