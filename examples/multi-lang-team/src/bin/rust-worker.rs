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
