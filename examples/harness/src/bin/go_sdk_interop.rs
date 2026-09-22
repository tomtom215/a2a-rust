// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! This SDK's CLIENT, driven against an agent built on the official Go SDK.
//!
//! The mirror of `itk/interop/go-sdk-client`, and run by the same script
//! (`scripts/go_sdk_interop.sh`). CI's TCK leg against a2a-go deliberately does
//! not use `a2a-protocol-client` (`tck/Cargo.toml`), so before this binary the
//! client a Rust coordinator actually calls Go agents with had never been
//! pointed at one in CI. Each check below is something the client's own tests
//! pass while a real a2a-go server answers differently:
//!
//! * a card with `securityRequirements` in a2a-go's JSON shape (T1);
//! * a v0.3 interface listed before the v1.0 one (C10);
//! * `DeleteTaskPushNotificationConfig` answered with no `result` / an empty
//!   body (C9) — driven by the shared sweep, which calls all eleven methods;
//! * streaming errors framed inside a 200 SSE stream (C8).
//!
//! Usage: `go_sdk_interop <agent-base-url>`. Exits non-zero on any failure or
//! any unexercised method × binding cell.

use std::time::Duration;

use a2a_example_harness::sweep::{make_send_params, sweep};
use a2a_example_harness::{Binding, Excuse, Matrix};
use a2a_protocol_client::{A2aClient, ClientBuilder, ClientError, resolve_agent_card};
use a2a_protocol_types::agent_card::AgentCard;
use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::method::Method;
use a2a_protocol_types::params::TaskQueryParams;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The bindings the Go agent serves. WebSocket is this SDK's §12 custom
/// binding; a2a-go does not implement it.
const BINDINGS: &[Binding] = &[Binding::JsonRpc, Binding::HttpJson, Binding::Grpc];

/// Bounds each error-path call, so a client that hangs instead of reporting
/// is a failure with a name rather than a stalled job.
const CALL_BUDGET: Duration = Duration::from_secs(15);

#[derive(Default)]
struct Tally {
    failures: Vec<String>,
}

impl Tally {
    fn ok(&self, name: &str, detail: &str) {
        println!("  [ok]   {name:<46} {detail}");
    }

    fn fail(&mut self, name: &str, detail: &str) {
        println!("  [FAIL] {name:<46} {detail}");
        self.failures.push(format!("{name}: {detail}"));
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let Some(base) = std::env::args().nth(1) else {
        eprintln!("usage: go_sdk_interop <agent-base-url>");
        std::process::exit(2);
    };
    let mut tally = Tally::default();

    println!("=== agent card ({base}) ===");
    let card = match resolve_agent_card(&base).await {
        Ok(card) => {
            tally.ok("resolve agent card", &card.name);
            card
        }
        Err(e) => {
            tally.fail("resolve agent card", &e.to_string());
            finish(&tally, None);
        }
    };
    check_card_security(&mut tally, &card);
    check_default_selection(&mut tally, &card).await;

    let webhook = start_webhook().await;
    let mut matrix = Matrix::new();
    for method in Method::ALL {
        matrix.excuse(
            *method,
            Binding::WebSocket,
            Excuse::NotApplicable("a2a-go serves no WebSocket binding"),
        );
    }

    for &binding in BINDINGS {
        println!("=== {} ===", binding.label());
        let client = match build(&card, binding).await {
            Ok(client) => client,
            Err(e) => {
                tally.fail("build client", &e.to_string());
                continue;
            }
        };
        let outcome = sweep(&client, binding, &webhook, "slow:", &mut matrix).await;
        for line in &outcome.lines {
            println!("{line}");
        }
        tally.failures.extend(outcome.failures);
        check_errors(&mut tally, &client).await;
    }

    println!();
    let missing = matrix.report();
    finish(&tally, Some(missing.len()));
}

fn finish(tally: &Tally, missing_cells: Option<usize>) -> ! {
    println!();
    println!("{} failure(s)", tally.failures.len());
    let complete = missing_cells == Some(0);
    if !tally.failures.is_empty() || !complete {
        std::process::exit(1);
    }
    std::process::exit(0);
}

async fn build(card: &AgentCard, binding: Binding) -> Result<A2aClient, ClientError> {
    let builder = ClientBuilder::from_card_preferring(card, &[binding.label().to_owned()])?;
    let chosen = builder
        .chosen_interface()
        .map(|i| (i.protocol_binding.clone(), i.protocol_version.clone()));
    match chosen {
        Some((b, _)) if !b.eq_ignore_ascii_case(binding.label()) => {
            return Err(ClientError::InvalidEndpoint(format!(
                "asked for {}, builder chose {b}",
                binding.label()
            )));
        }
        Some((_, v)) if !v.starts_with("1.") => {
            return Err(ClientError::InvalidEndpoint(format!(
                "builder chose protocolVersion {v} for {}",
                binding.label()
            )));
        }
        _ => {}
    }
    if binding == Binding::Grpc {
        builder.build_grpc().await
    } else {
        builder.build()
    }
}

/// T1: the card-level and skill-level requirements a2a-go publishes.
fn check_card_security(tally: &mut Tally, card: &AgentCard) {
    let name = "card securityRequirements (a2a-go shape)";
    let scopes = card
        .security_requirements
        .as_ref()
        .and_then(|reqs| reqs.first())
        .and_then(|req| req.schemes.get("apiKey"))
        .map(|scopes| scopes.list.clone());
    match scopes {
        Some(list) if list == ["read", "write"] => tally.ok(name, "apiKey [read write]"),
        other => tally.fail(
            name,
            &format!("apiKey scopes {other:?}; start the Go agent with INTEROP_CARD=1"),
        ),
    }
    let skill = card
        .skills
        .first()
        .and_then(|s| s.security_requirements.as_ref())
        .and_then(|reqs| reqs.first())
        .and_then(|req| req.schemes.get("apiKey"))
        .map(|scopes| scopes.list.clone());
    match skill {
        Some(list) if list == ["read"] => tally.ok("skill securityRequirements", "apiKey [read]"),
        other => tally.fail("skill securityRequirements", &format!("{other:?}")),
    }
}

/// C10: with no preference, the builder must skip the v0.3 decoy the Go agent
/// lists first, and the client it builds must answer.
async fn check_default_selection(tally: &mut Tally, card: &AgentCard) {
    let name = "default interface skips protocolVersion 0.3";
    let builder = match ClientBuilder::from_card(card) {
        Ok(b) => b,
        Err(e) => return tally.fail(name, &e.to_string()),
    };
    let version = builder
        .chosen_interface()
        .map(|i| i.protocol_version.clone())
        .unwrap_or_default();
    if !version.starts_with("1.") {
        return tally.fail(name, &format!("chose protocolVersion {version:?}"));
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => return tally.fail(name, &e.to_string()),
    };
    match tokio::time::timeout(CALL_BUDGET, client.send_message(make_send_params("hi"))).await {
        Ok(Ok(_)) => tally.ok(name, &format!("protocolVersion {version}")),
        Ok(Err(e)) => tally.fail(name, &e.to_string()),
        Err(_) => tally.fail(name, "send timed out"),
    }
}

/// C8 and friends: a missing task must come back as a typed `TaskNotFound`
/// on every method that can name one, streaming included.
async fn check_errors(tally: &mut Tally, client: &A2aClient) {
    let missing = || TaskQueryParams {
        tenant: None,
        id: "does-not-exist".into(),
        history_length: None,
    };
    let get = tokio::time::timeout(CALL_BUDGET, client.get_task(missing())).await;
    expect_not_found(tally, "GetTask(missing) -> TaskNotFound", flatten(get));

    let cancel = tokio::time::timeout(CALL_BUDGET, client.cancel_task("does-not-exist")).await;
    expect_not_found(
        tally,
        "CancelTask(missing) -> TaskNotFound",
        flatten(cancel),
    );

    let subscribe = tokio::time::timeout(CALL_BUDGET, async {
        let mut stream = client.subscribe_to_task("does-not-exist").await?;
        match stream.next().await {
            Some(Err(e)) => Err(e),
            Some(Ok(event)) => Ok(format!("an event: {event:?}")),
            None => Ok("an empty stream".to_owned()),
        }
    })
    .await;
    expect_not_found(
        tally,
        "SubscribeToTask(missing) -> TaskNotFound",
        flatten(subscribe),
    );

    let mut orphan = make_send_params("continue");
    orphan.message.task_id = Some("does-not-exist".into());
    let stream = tokio::time::timeout(CALL_BUDGET, async {
        let mut stream = client.stream_message(orphan).await?;
        match stream.next().await {
            Some(Err(e)) => Err(e),
            Some(Ok(event)) => Ok(format!("an event: {event:?}")),
            None => Ok("an empty stream".to_owned()),
        }
    })
    .await;
    expect_not_found(
        tally,
        "SendStreamingMessage(missing) -> TaskNotFound",
        flatten(stream),
    );
}

fn flatten<T: std::fmt::Debug>(
    r: Result<Result<T, ClientError>, tokio::time::error::Elapsed>,
) -> Result<String, String> {
    match r {
        Err(_) => Err("timed out".to_owned()),
        Ok(Ok(v)) => Ok(format!("{v:?}")),
        Ok(Err(ClientError::Protocol(e))) if e.code == ErrorCode::TaskNotFound => {
            Err(String::new())
        }
        Ok(Err(e)) => Err(format!("{e:?}")),
    }
}

fn expect_not_found(tally: &mut Tally, name: &str, got: Result<String, String>) {
    match got {
        Err(e) if e.is_empty() => tally.ok(name, "Protocol(TaskNotFound)"),
        Err(e) => tally.fail(name, &format!("wrong error: {e}")),
        Ok(v) => tally.fail(name, &format!("succeeded with {v}")),
    }
}

/// A webhook that answers 200 to anything, so push configs the sweep creates
/// point at an address that accepts deliveries.
async fn start_webhook() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind webhook");
    let addr = listener.local_addr().expect("webhook address");
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0_u8; 64 * 1024];
                // One read is enough for the small JSON bodies a push carries;
                // the reply is what the sender waits on.
                let _ = sock.read(&mut buf).await;
                let _ = sock
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    format!("http://{addr}/hook")
}
