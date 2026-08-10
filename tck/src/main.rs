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
//! a2a-tck --url http://localhost:8080 --binding grpc
//! a2a-tck --url http://localhost:8080 --binding grpc --grpc-url localhost:8079
//! ```
//!
//! `--url` is always the agent's **HTTP origin**, for every binding: §5
//! discovery is served over HTTPS no matter which binding carries the RPCs.
//! For `--binding websocket` and `--binding grpc` the listener's address is
//! read from the agent card's `WEBSOCKET` / `GRPC` interface — which is how a
//! real client would find it — and `--ws-url` / `--grpc-url` override that for
//! an agent that does not advertise one.
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

mod equivalence;
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
                "Usage: a2a-tck --url <http-origin> [--binding {}] \
                 [--ws-url <ws-endpoint>] [--grpc-url <host:port>] [--skip <tests>]",
                BINDINGS.join("|")
            );
            eprintln!();
            eprintln!("Options:");
            eprintln!("  --url <url>        HTTP origin of the A2A server (required).");
            eprintln!("                     Used for agent-card discovery in every binding.");
            eprintln!(
                "  --binding <type>   Protocol binding: {} (default: jsonrpc)",
                BINDINGS.join(", ")
            );
            eprintln!("  --ws-url <url>     WebSocket endpoint, when --binding websocket and the");
            eprintln!("                     agent card advertises no WEBSOCKET interface.");
            eprintln!("  --grpc-url <addr>  gRPC target, when --binding grpc and the agent card");
            eprintln!("                     advertises no GRPC interface.");
            eprintln!("  --skip <tests>     Comma-separated test names to skip (repeatable).");
            eprintln!("                     For documented target-implementation deviations");
            eprintln!("                     only — a skipped test is reported, not silent.");
            eprintln!("  --equivalence      Grade §5.1 cross-binding equivalence");
            eprintln!("                     (BIND-EQUIV-001..004) instead of one binding.");
            eprintln!("                     Drives every binding the card advertises.");
            return ExitCode::from(2);
        }
    };
    let Config {
        url,
        binding,
        endpoint,
        skips,
        equivalence,
    } = config;

    // §5.1 grades the relation between bindings, not any one of them, so it
    // discovers and drives them all rather than taking --binding.
    if equivalence {
        println!("A2A Protocol v1.0 — TCK cross-binding equivalence (§5.1)");
        println!("========================================================");
        println!("Target:  {url}");
        println!();
        return match equivalence::run_equivalence(&url).await {
            Err(msg) => {
                eprintln!("Error: {msg}");
                ExitCode::from(2)
            }
            Ok(results) => {
                let failed: Vec<_> = results.iter().filter(|r| !r.passed()).collect();
                println!();
                println!(
                    "Results: {}/{} requirements passed, {} failed",
                    results.len() - failed.len(),
                    results.len(),
                    failed.len()
                );
                if failed.is_empty() {
                    println!("All §5.1 equivalence requirements passed.");
                    ExitCode::from(0)
                } else {
                    println!();
                    println!("Failed requirements:");
                    for r in &failed {
                        println!("  FAIL  {} — {}", r.name, r.message);
                    }
                    ExitCode::from(1)
                }
            }
        };
    }

    // The RPC endpoint and the discovery origin are the same host for the
    // HTTP bindings and a separate listener for `websocket` and `grpc`, so
    // resolve them apart.
    let rpc_url = match binding.as_str() {
        "websocket" => {
            match resolve_endpoint(&url, endpoint.as_deref(), "WEBSOCKET", "--ws-url").await {
                Ok(resolved) => resolved,
                Err(msg) => {
                    eprintln!("Error: {msg}");
                    return ExitCode::from(2);
                }
            }
        }
        "grpc" => match resolve_endpoint(&url, endpoint.as_deref(), "GRPC", "--grpc-url").await {
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

/// Finds the endpoint a non-HTTP binding should drive.
///
/// Prefers the explicit override. Otherwise reads it from the agent card the
/// way a real client would: §5 discovery first, then the matching entry in
/// `supportedInterfaces` — `GRPC` is one of §5.3's canonical binding names,
/// and `WEBSOCKET` is the custom name §12 registers alongside them. An agent
/// that serves a listener it does not advertise is undiscoverable, so that
/// case is a config error naming the fix rather than a silent fallback to a
/// guessed port.
async fn resolve_endpoint(
    url: &str,
    override_url: Option<&str>,
    protocol_binding: &str,
    flag: &str,
) -> Result<String, String> {
    if let Some(explicit) = override_url {
        return Ok(explicit.to_owned());
    }

    let (status, card) = tests::helpers::rest_get(url, "/.well-known/agent-card.json")
        .await
        .map_err(|e| {
            format!(
                "fetching the agent card from {url} to find the {protocol_binding} endpoint: \
                 {e}\nPass {flag} to skip discovery."
            )
        })?;
    if status != 200 {
        return Err(format!(
            "the agent card at {url}/.well-known/agent-card.json returned HTTP {status}, so the \
             {protocol_binding} endpoint cannot be discovered. Pass {flag} to name it explicitly."
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
                .is_some_and(|b| b.eq_ignore_ascii_case(protocol_binding))
        })
        .and_then(|iface| iface.get("url"))
        .and_then(serde_json::Value::as_str);

    advertised.map(str::to_owned).ok_or_else(|| {
        format!(
            "the agent card at {url} advertises no {protocol_binding} interface in \
             supportedInterfaces, so a client has no way to find that listener. Add the \
             interface to the card, or pass {flag} to test an unadvertised endpoint."
        )
    })
}

struct Config {
    url: String,
    binding: String,
    /// The `--ws-url` / `--grpc-url` override, if given. One field because the
    /// two are mutually exclusive: a run drives exactly one binding.
    endpoint: Option<String>,
    skips: Vec<String>,
    /// Grade §5.1 cross-binding equivalence instead of one binding.
    equivalence: bool,
}

/// Which `--binding` each endpoint override belongs to. An override named
/// alongside any other binding is a config error, not a no-op: silently
/// ignoring it lets a CI step believe it is exercising a listener it never
/// dials.
const ENDPOINT_FLAGS: &[(&str, &str)] = &[("--ws-url", "websocket"), ("--grpc-url", "grpc")];

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut url = None;
    let mut binding = "jsonrpc".to_string();
    let mut endpoint: Option<(&str, String)> = None;
    let mut skips: Vec<String> = Vec::new();
    let mut equivalence = false;
    let mut binding_was_given = false;

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
                binding_was_given = true;
            }
            "--equivalence" => equivalence = true,
            flag if ENDPOINT_FLAGS.iter().any(|(f, _)| *f == flag) => {
                let owned = flag.to_string();
                i += 1;
                let value = args
                    .get(i)
                    .ok_or_else(|| format!("{owned} requires a value"))?
                    .clone();
                let key = ENDPOINT_FLAGS
                    .iter()
                    .find(|(f, _)| *f == owned)
                    .map(|(f, _)| *f)
                    .expect("matched above");
                if let Some((previous, _)) = endpoint {
                    return Err(format!(
                        "{previous} and {key} are mutually exclusive — a run drives one binding"
                    ));
                }
                endpoint = Some((key, value));
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

    // §5.1 drives every advertised binding, so naming one is a contradiction
    // rather than a refinement. Accepting it silently would let a CI step
    // believe it had scoped the comparison when it had not.
    if equivalence {
        if binding_was_given {
            return Err(
                "--equivalence compares every binding the card advertises, so --binding \
                 has nothing to select. Drop one of them."
                    .to_string(),
            );
        }
        if let Some((flag, _)) = endpoint {
            return Err(format!(
                "{flag} names one binding's endpoint, but --equivalence resolves every \
                 binding from the card. Drop one of them."
            ));
        }
        if !skips.is_empty() {
            return Err(
                "--skip names per-binding checks; --equivalence grades the four §5.1 \
                 requirements, which have no per-binding names to waive."
                    .to_string(),
            );
        }
    }

    let endpoint = match endpoint {
        None => None,
        Some((flag, value)) => {
            let expected = ENDPOINT_FLAGS
                .iter()
                .find(|(f, _)| *f == flag)
                .map(|(_, b)| *b)
                .expect("flag came from the table");
            if binding != expected {
                return Err(format!(
                    "{flag} only applies to --binding {expected}, but the binding is '{binding}'"
                ));
            }
            Some(value)
        }
    };
    Ok(Config {
        url,
        binding,
        endpoint,
        skips,
        equivalence,
    })
}

#[cfg(test)]
mod tests_main {
    use super::{parse_args, BINDINGS, ENDPOINT_FLAGS};

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

    /// An endpoint override on a binding that never dials it names something
    /// nothing will ever connect to. Silently ignoring it lets a CI step
    /// believe it is exercising a listener it never touches. Checked for
    /// every flag in the table, so a new one cannot be added without the
    /// guard applying to it.
    #[test]
    fn every_endpoint_override_is_rejected_on_the_wrong_binding() {
        for (flag, owner) in ENDPOINT_FLAGS {
            let wrong = BINDINGS
                .iter()
                .find(|b| *b != owner)
                .expect("more than one binding exists");
            let err = parse_args(&argv(&[
                "--url",
                "http://x",
                "--binding",
                wrong,
                flag,
                "somewhere",
            ]))
            .err()
            .unwrap_or_else(|| panic!("{flag} must not be silently ignored on --binding {wrong}"));
            assert!(err.contains(flag), "{err}");
        }
    }

    #[test]
    fn every_endpoint_override_is_carried_through_on_its_own_binding() {
        for (flag, owner) in ENDPOINT_FLAGS {
            let cfg = parse_args(&argv(&[
                "--url",
                "http://x",
                "--binding",
                owner,
                flag,
                "the-endpoint",
            ]))
            .unwrap_or_else(|e| panic!("{flag} with --binding {owner} must parse: {e}"));
            assert_eq!(cfg.endpoint.as_deref(), Some("the-endpoint"));
        }
    }

    #[test]
    fn two_endpoint_overrides_at_once_are_rejected() {
        let err = parse_args(&argv(&[
            "--url",
            "http://x",
            "--binding",
            "websocket",
            "--ws-url",
            "ws://y",
            "--grpc-url",
            "z:1",
        ]))
        .err()
        .expect("a run drives one binding, so two endpoint overrides is a config error");
        assert!(err.contains("mutually exclusive"), "{err}");
    }

    /// Every flag in the table must name a binding the CLI accepts, or the
    /// override can never be used and its guard can never fire.
    #[test]
    fn every_endpoint_flag_names_a_known_binding() {
        for (flag, owner) in ENDPOINT_FLAGS {
            assert!(
                BINDINGS.contains(owner),
                "{flag} is owned by unknown binding {owner:?}; known: {BINDINGS:?}"
            );
        }
    }

    #[test]
    fn binding_defaults_to_jsonrpc() {
        let cfg = parse_args(&argv(&["--url", "http://x"])).expect("valid config");
        assert_eq!(cfg.binding, "jsonrpc");
        assert!(cfg.endpoint.is_none());
        assert!(!cfg.equivalence);
    }

    #[test]
    fn equivalence_is_opt_in() {
        let cfg = parse_args(&argv(&["--url", "http://x", "--equivalence"]))
            .expect("valid equivalence config");
        assert!(cfg.equivalence);
    }

    /// Every flag that scopes a run to one binding contradicts
    /// `--equivalence`, which resolves them all from the card. Accepting one
    /// silently would let a run look narrower than it is.
    #[test]
    fn equivalence_rejects_flags_that_scope_to_one_binding() {
        for extra in [
            vec!["--binding", "jsonrpc"],
            vec!["--ws-url", "ws://y"],
            vec!["--grpc-url", "y:1"],
            vec!["--skip", "send_message_basic"],
        ] {
            let mut args = vec!["--url", "http://x", "--equivalence"];
            args.extend(extra.iter().copied());
            let err = parse_args(&argv(&args))
                .err()
                .unwrap_or_else(|| panic!("--equivalence with {extra:?} must be rejected"));
            assert!(
                err.contains("--equivalence"),
                "the error must name the conflict: {err}"
            );
        }
    }

    #[test]
    fn url_is_required() {
        assert!(parse_args(&argv(&["--binding", "rest"])).is_err());
    }
}
