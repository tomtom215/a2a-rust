// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

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
//! - 1: One or more tests failed, or a `--skip`ped test now passes (a stale
//!   waiver — see the `STALE SKIP` block in `main`)
//! - 2: Configuration error, including a `--skip` name matching no test

#![forbid(unsafe_code)]

use std::process::ExitCode;

mod runner;
mod tests;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    let (url, binding, skips) = match parse_args(&args) {
        Ok(config) => config,
        Err(msg) => {
            eprintln!("Error: {msg}");
            eprintln!();
            eprintln!(
                "Usage: a2a-tck --url <server-url> [--binding jsonrpc|rest] [--skip <tests>]"
            );
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --url <url>        Base URL of the A2A server (required)");
            eprintln!("  --binding <type>   Protocol binding: jsonrpc (default) or rest");
            eprintln!("  --skip <tests>     Comma-separated test names to skip (repeatable).");
            eprintln!("                     For documented target-implementation deviations");
            eprintln!("                     only — a skipped test is reported, not silent.");
            return ExitCode::from(2);
        }
    };

    println!("A2A Protocol v1.0 — Technology Compatibility Kit");
    println!("================================================");
    println!("Target:  {url}");
    println!("Binding: {binding}");
    println!();

    if !skips.is_empty() {
        println!(
            "Skipping (documented target deviations): {}",
            skips.join(", ")
        );
        println!();
    }

    let results = runner::run_all(&url, &binding).await;

    // A `--skip` name that matches no test is dead configuration — a typo, or
    // a test renamed since the waiver was written. Left silent it is worse
    // than useless: it reads as an active waiver while gating nothing, so the
    // reader cannot tell live skips from fossils. Fail as a config error.
    let unmatched: Vec<&str> = skips
        .iter()
        .filter(|s| !results.iter().any(|r| &r.name == *s))
        .map(String::as_str)
        .collect();

    let skipped: Vec<_> = results
        .iter()
        .filter(|r| skips.iter().any(|s| s == &r.name))
        .collect();
    let counted: Vec<_> = results
        .iter()
        .filter(|r| !skips.iter().any(|s| s == &r.name))
        .collect();
    let total = counted.len();
    let passed = counted.iter().filter(|r| r.passed).count();
    let failed = total - passed;

    for r in &skipped {
        let outcome = if r.passed {
            "passed anyway"
        } else {
            "failed as documented"
        };
        println!("  SKIP  {} — {outcome}", r.name);
    }
    println!();
    println!(
        "Results: {passed}/{total} passed, {failed} failed{}",
        if skipped.is_empty() {
            String::new()
        } else {
            format!(", {} skipped", skipped.len())
        }
    );

    if failed > 0 {
        println!();
        println!("Failed tests:");
        for result in &counted {
            if !result.passed {
                println!("  FAIL  {} — {}", result.name, result.message);
            }
        }
    }

    if !unmatched.is_empty() {
        println!();
        println!(
            "Config error — {} --skip name(s) match no test:",
            unmatched.len()
        );
        for name in &unmatched {
            println!("  {name}");
        }
        println!();
        println!("A waiver that names nothing gates nothing. Fix the name, or");
        println!("drop it if the test it named is gone.");
        return ExitCode::from(2);
    }

    // A skipped test that PASSES is a waiver that has outlived its reason —
    // the upstream defect it documents is fixed. Tolerating it silently is the
    // same rot `tck/scripts/check_conformance.py` refuses for the official
    // suite ("a baseline that is allowed to rot is just continue-on-error with
    // extra steps"). Hold this runner to the same standard: the waiver must
    // shrink to match reality, so going green here turns the job red until the
    // skip is removed.
    let stale: Vec<&str> = skipped
        .iter()
        .filter(|r| r.passed)
        .map(|r| r.name.as_str())
        .collect();
    if !stale.is_empty() {
        println!();
        println!("STALE SKIP — {} skipped test(s) now pass:", stale.len());
        for name in &stale {
            println!("  {name}");
        }
        println!();
        println!("Good news, but the skip list must shrink to match, or it stops");
        println!("meaning anything. Remove these from --skip in the workflow");
        println!("matrix and drop the note that documented the upstream bug.");
        return ExitCode::from(1);
    }

    if failed > 0 {
        return ExitCode::from(1);
    }

    println!("All conformance tests passed.");
    ExitCode::from(0)
}

#[allow(clippy::type_complexity)]
fn parse_args(args: &[String]) -> Result<(String, String, Vec<String>), String> {
    let mut url = None;
    let mut binding = "jsonrpc".to_string();
    let mut skips: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--url" => {
                i += 1;
                url = Some(args.get(i).ok_or("--url requires a value")?.clone());
            }
            "--binding" => {
                i += 1;
                let b = args
                    .get(i)
                    .ok_or("--binding requires a value")?
                    .to_lowercase();
                if b != "jsonrpc" && b != "rest" {
                    return Err(format!(
                        "invalid binding '{b}', expected 'jsonrpc' or 'rest'"
                    ));
                }
                binding = b;
            }
            "--skip" => {
                i += 1;
                let list = args.get(i).ok_or("--skip requires a value")?;
                skips.extend(
                    list.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                );
            }
            other => return Err(format!("unknown argument '{other}'")),
        }
        i += 1;
    }

    let url = url.ok_or("--url is required")?;
    Ok((url, binding, skips))
}
