// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The SDK end to end with a real model behind an OpenAI-compatible API
//! (llama.cpp's `llama-server`, vLLM, Ollama). See the README for what each
//! check proves and how a run was recorded.
//!
//! Prints one line per check as it finishes, then one JSON line per check,
//! and exits non-zero if any failed. A check the model did not exercise is
//! reported as such and does not fail the run; it does not pass it either.

use std::process::ExitCode;
use std::time::Duration;

use a2a_protocol_sdk::prelude::Telemetry;
use a2a_protocol_sdk::server::tool_loop::OpenAiChatModel;

mod agents;
mod checks;
mod tools;

use agents::{Binding, Worker};
use checks::{Check, Verdict};

/// The model from `LIVE_BASE_URL` (default llama-server's
/// `http://127.0.0.1:8080/v1`) and `LIVE_MODEL`, with thinking off unless
/// `LIVE_THINKING=1`, and `LIVE_EXTRA_BODY` merged into every request.
fn model() -> Result<OpenAiChatModel, Box<dyn std::error::Error>> {
    let base =
        std::env::var("LIVE_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080/v1".to_owned());
    let name = std::env::var("LIVE_MODEL").unwrap_or_else(|_| "qwen3:1.7b".to_owned());
    let model = OpenAiChatModel::new(&base, name)?.with_timeout(Duration::from_secs(
        std::env::var("LIVE_MODEL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600),
    ));
    let mut extra = serde_json::Map::new();
    if std::env::var("LIVE_THINKING").as_deref() != Ok("1") {
        extra.insert(
            "chat_template_kwargs".to_owned(),
            serde_json::json!({"enable_thinking": false}),
        );
    }
    if let Ok(more) = std::env::var("LIVE_EXTRA_BODY") {
        let serde_json::Value::Object(more) = serde_json::from_str(&more)? else {
            return Err("LIVE_EXTRA_BODY must be a JSON object".into());
        };
        extra.extend(more);
    }
    Ok(model.with_extra_body(extra))
}

fn report(check: &Check) {
    let mark = match check.verdict {
        Verdict::Pass => "ok",
        Verdict::Fail => "FAIL",
        Verdict::NotExercised => "NOT EXERCISED",
    };
    println!(
        "[{mark}] {} ({:.1}s): {}",
        check.name, check.seconds, check.detail
    );
}

async fn run() -> Result<Vec<Check>, Box<dyn std::error::Error>> {
    let worker = Worker::serve(model()?).await?;
    let coordinator =
        agents::serve_coordinator(worker.client(Binding::JsonRpc, worker.keys.clone()).await?)
            .await?;
    let approver = agents::serve_approver(model()?).await?;
    let mut done = Vec::new();
    for binding in Binding::ALL {
        let check = checks::run(
            format!("tool loop streamed and verified, {}", binding.name()),
            checks::stream_on(&worker, binding),
        )
        .await;
        report(&check);
        done.push(check);
    }
    let rest = [
        checks::run(
            "drafts: each turn's text streams before the answer",
            checks::drafts(&worker),
        )
        .await,
        checks::run(
            "provenance: another key's client refuses",
            checks::provenance_refused(&worker),
        )
        .await,
        checks::run(
            "idempotency: a repeated restart runs once",
            checks::idempotency(&worker),
        )
        .await,
        checks::run(
            "budget: one token is exhausted",
            checks::budget_exhausted(&worker),
        )
        .await,
        checks::run(
            "delegation under a budget",
            checks::delegation(&coordinator),
        )
        .await,
        checks::run("halt cancels a working run", checks::halt(&worker)).await,
        checks::run("quorum of two approvers", checks::quorum(&approver)).await,
    ];
    for check in rest {
        report(&check);
        done.push(check);
    }
    Ok(done)
}

#[tokio::main]
async fn main() -> ExitCode {
    // OpenTelemetry from the standard `OTEL_*` environment, for each signal
    // an endpoint is configured for: every binding's server span and
    // `rpc.server.call.duration`, the client's, and the tool loop's
    // `invoke_agent`, `chat` and `execute_tool` spans and `gen_ai.*`
    // metrics. Nothing is built without an endpoint. Kept until `main`
    // returns, which flushes it.
    let telemetry = Telemetry::builder()
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
    let done = match run().await {
        Ok(done) => done,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    for check in &done {
        println!(
            "{}",
            serde_json::json!({
                "check": check.name,
                "verdict": format!("{:?}", check.verdict),
                "seconds": (check.seconds * 10.0).round() / 10.0,
                "detail": check.detail,
            })
        );
    }
    let failed = done.iter().filter(|c| c.verdict == Verdict::Fail).count();
    let unexercised = done
        .iter()
        .filter(|c| c.verdict == Verdict::NotExercised)
        .count();
    println!(
        "{} passed, {failed} failed, {unexercised} not exercised, of {}",
        done.len() - failed - unexercised,
        done.len()
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
