// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! A2A Protocol v1.0 — Technology Compatibility Kit (TCK)
//!
//! A standalone conformance test runner that validates any A2A server
//! implementation against the official protocol specification.
//!
//! # Usage
//!
//! ```bash
//! a2a-tck --url http://localhost:8080
//! a2a-tck --url http://localhost:8080 --binding jsonrpc
//! a2a-tck --url http://localhost:8080 --binding rest
//! ```
//!
//! # Exit codes
//!
//! - 0: All tests passed
//! - 1: One or more tests failed
//! - 2: Configuration error

use std::process::ExitCode;

mod runner;
mod tests;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    let (url, binding) = match parse_args(&args) {
        Ok(config) => config,
        Err(msg) => {
            eprintln!("Error: {msg}");
            eprintln!();
            eprintln!("Usage: a2a-tck --url <server-url> [--binding jsonrpc|rest]");
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --url <url>        Base URL of the A2A server (required)");
            eprintln!("  --binding <type>   Protocol binding: jsonrpc (default) or rest");
            return ExitCode::from(2);
        }
    };

    println!("A2A Protocol v1.0 — Technology Compatibility Kit");
    println!("================================================");
    println!("Target:  {url}");
    println!("Binding: {binding}");
    println!();

    let results = runner::run_all(&url, &binding).await;

    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    println!();
    println!("Results: {passed}/{total} passed, {failed} failed");

    if failed > 0 {
        println!();
        println!("Failed tests:");
        for result in &results {
            if !result.passed {
                println!("  FAIL  {} — {}", result.name, result.message);
            }
        }
        ExitCode::from(1)
    } else {
        println!("All conformance tests passed.");
        ExitCode::from(0)
    }
}

fn parse_args(args: &[String]) -> Result<(String, String), String> {
    let mut url = None;
    let mut binding = "jsonrpc".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                url = Some(
                    args.get(i)
                        .ok_or("--url requires a value")?
                        .clone(),
                );
            }
            "--binding" => {
                i += 1;
                let b = args
                    .get(i)
                    .ok_or("--binding requires a value")?
                    .to_lowercase();
                if b != "jsonrpc" && b != "rest" {
                    return Err(format!("invalid binding '{b}', expected 'jsonrpc' or 'rest'"));
                }
                binding = b;
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
        i += 1;
    }

    let url = url.ok_or("--url is required")?;
    Ok((url, binding))
}
