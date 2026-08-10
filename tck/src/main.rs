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
//! a2a-tck --url http://localhost:8080 --binding websocket
//! a2a-tck --url http://localhost:8080 --binding websocket --ws-url ws://localhost:8081
//! ```
//!
//! `--url` is always the agent's **HTTP origin**, for every binding: §5
//! discovery is served over HTTPS no matter which binding carries the RPCs.
//! For `--binding websocket` the socket endpoint is read from the agent
//! card's `WEBSOCKET` interface — which is how a real client would find it —
//! and `--ws-url` overrides that for an agent that does not advertise one.
//!
//! # Exit codes
//!
//! - 0: All tests passed
//! - 1: One or more tests failed, or a `--skip`ped test now passes (a stale
//!   waiver — see the `STALE SKIP` block in `main`)
//! - 2: Configuration error: a `--skip` name matching no test, a binding the
//!   target cannot be reached on, or a run that graded nothing

#![forbid(unsafe_code)]

use std::process::ExitCode;

mod runner;
mod tests;

use runner::BINDINGS;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    let config = match parse_args(&args) {
        Ok(config) => config,
        Err(msg) => {
            eprintln!("Error: {msg}");
            eprintln!();
            eprintln!(
                "Usage: a2a-tck --url <http-origin> [--binding jsonrpc|rest|websocket] \
                 [--ws-url <ws-endpoint>] [--skip <tests>]"
            );
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --url <url>        HTTP origin of the A2A server (required).");
            eprintln!("                     Used for agent-card discovery in every binding.");
            eprintln!(
                "  --binding <type>   Protocol binding: jsonrpc (default), rest or websocket"
            );
            eprintln!("  --ws-url <url>     WebSocket endpoint, when --binding websocket and the");
            eprintln!("                     agent card advertises no WEBSOCKET interface.");
            eprintln!("  --skip <tests>     Comma-separated test names to skip (repeatable).");
            eprintln!("                     For documented target-implementation deviations");
            eprintln!("                     only — a skipped test is reported, not silent.");
            return ExitCode::from(2);
        }
    };
    let Config {
        url,
        binding,
        ws_url,
        skips,
    } = config;

    // The RPC endpoint and the discovery origin are the same host for the
    // HTTP bindings and different for `websocket`, so resolve them apart.
    let rpc_url = match binding.as_str() {
        "websocket" => match resolve_ws_endpoint(&url, ws_url.as_deref()).await {
            Ok(resolved) => resolved,
            Err(msg) => {
                eprintln!("Error: {msg}");
                return ExitCode::from(2);
            }
        },
        _ => url.clone(),
    };

    println!("A2A Protocol v1.0 — Technology Compatibility Kit");
    println!("================================================");
    println!("Target:  {url}");
    println!("Binding: {binding}");
    if rpc_url != url {
        println!("RPC via: {rpc_url}");
    }
    println!();

    if !skips.is_empty() {
        println!(
            "Skipping (documented target deviations): {}",
            skips.join(", ")
        );
        println!();
    }

    let results = runner::run_all(&url, &rpc_url, &binding).await;

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
    // Not-applicable checks verified nothing, so they are not part of the
    // score. Folding them into `passed` would report work never done.
    let graded = counted.iter().filter(|r| r.graded()).count();
    let not_applicable = counted.len() - graded;
    let passed = counted.iter().filter(|r| r.passed()).count();
    let failed = graded - passed;

    for r in &skipped {
        let outcome = if !r.graded() {
            "not applicable to this binding — nothing waived"
        } else if r.passed() {
            "passed anyway"
        } else {
            "failed as documented"
        };
        println!("  SKIP  {} — {outcome}", r.name);
    }
    println!();
    println!(
        "Results: {passed}/{graded} graded checks passed, {failed} failed{}{}",
        if not_applicable == 0 {
            String::new()
        } else {
            format!(", {not_applicable} not applicable")
        },
        if skipped.is_empty() {
            String::new()
        } else {
            format!(", {} skipped", skipped.len())
        }
    );

    if not_applicable > 0 {
        println!();
        println!("Not applicable to the {binding} binding (nothing verified):");
        for r in &counted {
            if !r.graded() {
                println!("  N/A   {} — {}", r.name, r.message);
            }
        }
    }

    if failed > 0 {
        println!();
        println!("Failed tests:");
        for result in &counted {
            if result.graded() && !result.passed() {
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

    // A run that grades nothing is the failure mode `--min-graded` exists to
    // catch in `tck/scripts/check_conformance.py`: every check excluded or
    // waived, a clean exit, and no evidence of anything. Refuse to report
    // success for it.
    if graded == 0 {
        println!();
        println!("Config error — this run graded zero checks.");
        println!("Every check was skipped or ruled not applicable, so a green");
        println!("exit here would mean nothing was verified at all.");
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
        .filter(|r| r.passed())
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

/// Finds the endpoint the `websocket` binding should drive.
///
/// Prefers an explicit `--ws-url`. Otherwise reads it from the agent card the
/// way a real client would: §5 discovery first, then the `WEBSOCKET` entry in
/// `supportedInterfaces` (§12 registers it as a custom binding name alongside
/// the canonical `JSONRPC`/`GRPC`/`HTTP+JSON`). An agent that serves the
/// socket but does not advertise it is undiscoverable, so that case is a
/// config error naming the fix rather than a silent fallback to a guessed
/// port.
async fn resolve_ws_endpoint(url: &str, ws_url: Option<&str>) -> Result<String, String> {
    if let Some(explicit) = ws_url {
        return Ok(explicit.to_owned());
    }

    let (status, card) = tests::helpers::rest_get(url, "/.well-known/agent-card.json")
        .await
        .map_err(|e| format!("fetching the agent card from {url} to find the WebSocket endpoint: {e}\nPass --ws-url to skip discovery."))?;
    if status != 200 {
        return Err(format!(
            "the agent card at {url}/.well-known/agent-card.json returned HTTP {status}, so the \
             WebSocket endpoint cannot be discovered. Pass --ws-url to name it explicitly."
        ));
    }

    let advertised = card
        .get("supportedInterfaces")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|iface| {
            iface
                .get("protocolBinding")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|b| b.eq_ignore_ascii_case("websocket"))
        })
        .and_then(|iface| iface.get("url"))
        .and_then(serde_json::Value::as_str);

    advertised.map(str::to_owned).ok_or_else(|| {
        format!(
            "the agent card at {url} advertises no WEBSOCKET interface in supportedInterfaces, \
             so a client has no way to find the socket. Add the interface to the card, or pass \
             --ws-url to test an unadvertised endpoint."
        )
    })
}

struct Config {
    url: String,
    binding: String,
    ws_url: Option<String>,
    skips: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut url = None;
    let mut binding = "jsonrpc".to_string();
    let mut ws_url = None;
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
                if !BINDINGS.contains(&b.as_str()) {
                    return Err(format!(
                        "invalid binding '{b}', expected one of: {}",
                        BINDINGS.join(", ")
                    ));
                }
                binding = b;
            }
            "--ws-url" => {
                i += 1;
                ws_url = Some(args.get(i).ok_or("--ws-url requires a value")?.clone());
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
    if ws_url.is_some() && binding != "websocket" {
        return Err(format!(
            "--ws-url only applies to --binding websocket, but the binding is '{binding}'"
        ));
    }
    Ok(Config {
        url,
        binding,
        ws_url,
        skips,
    })
}

#[cfg(test)]
mod tests_main {
    use super::{parse_args, BINDINGS};

    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("a2a-tck")
            .chain(rest.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn every_advertised_binding_parses() {
        for b in BINDINGS {
            let cfg = parse_args(&argv(&["--url", "http://x", "--binding", b]))
                .unwrap_or_else(|e| panic!("--binding {b} must parse: {e}"));
            assert_eq!(&cfg.binding, b);
        }
    }

    #[test]
    fn unknown_binding_is_a_config_error() {
        let err = parse_args(&argv(&["--url", "http://x", "--binding", "websockets"]))
            .err()
            .expect("a misspelled binding must not be accepted");
        assert!(err.contains("websockets"), "{err}");
    }

    /// `--ws-url` on an HTTP binding names an endpoint nothing will ever
    /// dial. Silently ignoring it lets a CI step believe it is exercising a
    /// socket it never touches.
    #[test]
    fn ws_url_without_the_websocket_binding_is_rejected() {
        let err = parse_args(&argv(&[
            "--url",
            "http://x",
            "--binding",
            "jsonrpc",
            "--ws-url",
            "ws://y",
        ]))
        .err()
        .expect("--ws-url must not be silently ignored on an HTTP binding");
        assert!(err.contains("--ws-url"), "{err}");
    }

    #[test]
    fn ws_url_is_carried_through_for_the_websocket_binding() {
        let cfg = parse_args(&argv(&[
            "--url",
            "http://x",
            "--binding",
            "websocket",
            "--ws-url",
            "ws://y:9091",
        ]))
        .expect("valid websocket config");
        assert_eq!(cfg.ws_url.as_deref(), Some("ws://y:9091"));
    }

    #[test]
    fn binding_defaults_to_jsonrpc() {
        let cfg = parse_args(&argv(&["--url", "http://x"])).expect("valid config");
        assert_eq!(cfg.binding, "jsonrpc");
        assert!(cfg.ws_url.is_none());
    }

    #[test]
    fn url_is_required() {
        assert!(parse_args(&argv(&["--binding", "rest"])).is_err());
    }
}
