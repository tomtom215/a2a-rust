// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The narrated five-act demo.
//!
//! Acts 1-3 are the agent story, Act 4 measures the protocol surface, and
//! Act 5 exercises what a deployment needs beyond the protocol. Each act
//! asserts; none of them merely prints.

use a2a_example_harness::{counter, sweep, Binding, Matrix};
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState};

use crate::serving::{
    build_client, start_logs_agent, start_restricted_agent, start_runbook_agent,
    start_triage_agent, start_webhook_sink,
};
use crate::{
    extract_text, hardening, incident_model, send_params, user_message, SURFACE_PAUSE_PREFIX,
};

// ── Demo client ──────────────────────────────────────────────────────────────

/// Streams one message to the triage agent, narrating every event, and
/// returns the task id, context id, and last observed state.
async fn stream_and_narrate(
    client: &a2a_protocol_client::A2aClient,
    message: Message,
) -> Result<(Option<String>, Option<String>, Option<TaskState>), Box<dyn std::error::Error>> {
    let mut stream = client.stream_message(send_params(message)).await?;
    let mut task_id = None;
    let mut context_id = None;
    let mut last_state = None;

    while let Some(event) = stream.next().await {
        match event {
            Ok(StreamResponse::Task(task)) => {
                println!("  ⇢ task {} [{}]", task.id.0, task.status.state);
                task_id = Some(task.id.0);
                context_id = Some(task.context_id.0.clone());
                last_state = Some(task.status.state);
            }
            Ok(StreamResponse::StatusUpdate(ev)) => {
                let note = ev
                    .status
                    .message
                    .as_ref()
                    .map(|m| format!(" — {}", extract_text(&m.parts)))
                    .unwrap_or_default();
                println!("  ⇢ status: {}{note}", ev.status.state);
                last_state = Some(ev.status.state);
            }
            Ok(StreamResponse::ArtifactUpdate(ev)) => {
                println!(
                    "  ⇢ artifact '{}' ({} chars)",
                    ev.artifact.name.as_deref().unwrap_or(&ev.artifact.id.0),
                    extract_text(&ev.artifact.parts).len()
                );
            }
            Ok(_) => {}
            Err(e) => println!("  ⇢ stream error: {e}"),
        }
    }
    Ok((task_id, context_id, last_state))
}

#[allow(clippy::too_many_lines)]
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("Incident-Response Agent Team");
    println!("============================");
    println!();
    println!(
        "Model: {} (set INCIDENT_MODEL to change; with no model running",
        incident_model()
    );
    println!("on :11434 the agents fall back to labeled mechanical summaries)");
    println!();

    let logs = start_logs_agent().await?;
    let runbook = start_runbook_agent().await?;
    let triage = start_triage_agent(logs.http.clone(), runbook.http.clone()).await?;
    for (label, ep) in [("logs", &logs), ("runbook", &runbook), ("triage", &triage)] {
        println!("{label:<8} agent: {}  grpc {}  {}", ep.http, ep.grpc, ep.ws);
    }
    let triage_url = triage.http.clone();

    let client = ClientBuilder::new(&triage_url).build()?;

    // ── Act 1: a vague alert — the agent asks instead of guessing ─────────
    println!();
    println!("ACT 1 — vague alert: the task pauses and asks for missing input");
    println!("  → \"Customers report payments failing since 14:00, please investigate\"");
    let (task_id, context_id, state) = stream_and_narrate(
        &client,
        user_message("Customers report payments failing since 14:00, please investigate"),
    )
    .await?;
    let task_id = task_id.ok_or("no task id from stream")?;
    let context_id = context_id.ok_or("no context id from stream")?;
    assert_eq!(
        state,
        Some(TaskState::InputRequired),
        "expected the task to park in INPUT_REQUIRED"
    );

    // ── Act 2: answer on the SAME task — it resumes where it left off ─────
    println!();
    println!("ACT 2 — the operator answers on the same task; the agents collaborate");
    println!("  → \"it's payments-api\"  (task {task_id})");
    // Continuing a task requires BOTH ids: the task id selects the parked
    // task, and the context id must match the conversation it belongs to.
    let mut follow_up = user_message("it's payments-api");
    follow_up.task_id = Some(TaskId(task_id.clone()));
    follow_up.context_id = Some(ContextId::new(context_id));
    let (_, _, state) = stream_and_narrate(&client, follow_up).await?;
    assert_eq!(state, Some(TaskState::Completed), "triage should complete");

    // Fetch the finished task and print the report artifact.
    let task = client
        .get_task(a2a_protocol_types::params::TaskQueryParams {
            tenant: None,
            id: task_id.clone(),
            history_length: None,
        })
        .await?;
    if let Some(report) = task
        .artifacts
        .as_deref()
        .and_then(<[Artifact]>::first)
        .map(|a| extract_text(&a.parts))
    {
        println!();
        println!("─── incident-report artifact ───");
        println!("{report}");
        println!("────────────────────────────────");
    }

    // ── Act 3: cancellation — a paused task can be called off ─────────────
    // A vague alert parks the task in INPUT_REQUIRED; instead of answering,
    // the operator decides it was noise and cancels it. (Cancelling mid-WORK
    // also works, but with a warm local model the whole triage can finish in
    // under a second — a pause is the deterministic place to demonstrate it.)
    println!();
    println!("ACT 3 — tasks are cancellable: the operator calls off a parked task");
    println!("  → \"seeing odd error rates somewhere, look into it\"");
    let (cancel_id, _, state) = stream_and_narrate(
        &client,
        user_message("seeing odd error rates somewhere, look into it"),
    )
    .await?;
    let cancel_id = cancel_id.ok_or("no task id for cancel demo")?;
    assert_eq!(state, Some(TaskState::InputRequired));
    let canceled = client.cancel_task(cancel_id.clone()).await?;
    println!("  → cancel_task(...) ⇒ {}", canceled.status.state);
    assert_eq!(canceled.status.state, TaskState::Canceled);

    // ── Act 4: the whole protocol surface, measured ───────────────────────
    //
    // Acts 1-3 show what an agent *is*. They do not show that this SDK serves
    // the whole A2A surface, and until 2026-08-11 this example drove 4 of the
    // 11 methods over 1 of the 4 bindings while `examples/README` presented it
    // as the place to start. That gap was invisible because nothing counted.
    //
    // A "vague alert" is the marker that parks a task in INPUT_REQUIRED, which
    // is what `SubscribeToTask` needs: the server refuses to re-attach to a
    // terminal task, correctly, so without a non-terminal one the success path
    // is unreachable and only the refusal is ever observed. Act 1 already
    // relies on this behaviour, so the sweep reuses the example's own
    // semantics rather than inventing a sleep.
    println!();
    println!("ACT 4 — every A2A method over every binding, counted");
    let webhook = start_webhook_sink().await?;
    let mut matrix = Matrix::new();
    let mut failures: Vec<String> = Vec::new();

    for binding in Binding::ALL {
        let surface_client = match build_client(*binding, &triage).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("could not build a {} client: {e}", binding.label());
                std::process::exit(1);
            }
        };
        println!("  --- {} ---", binding.label());
        let outcome = sweep(
            &surface_client,
            *binding,
            &webhook,
            SURFACE_PAUSE_PREFIX,
            &mut matrix,
        )
        .await;
        for l in &outcome.lines {
            println!("  {l}");
        }
        failures.extend(outcome.failures);
    }

    // Counter-tests need an agent that advertises nothing optional; one agent
    // cannot both support and refuse a capability.
    println!("  --- counter-tests (calls that must be refused) ---");
    let restricted = start_restricted_agent().await?;
    let counter_out = counter::run(
        &ClientBuilder::new(&triage_url).build()?,
        &ClientBuilder::new(&restricted).build()?,
    )
    .await;
    for l in &counter_out.lines {
        println!("  {l}");
    }
    failures.extend(counter_out.failures);

    println!();
    let missing = matrix.report();
    if !failures.is_empty() {
        println!("\n{} call(s) failed:", failures.len());
        for f in &failures {
            println!("  - {f}");
        }
        std::process::exit(1);
    }
    if !missing.is_empty() {
        println!("\n{} matrix cell(s) never ran:", missing.len());
        for (m, b) in &missing {
            println!("  - {} over {}", m.wire_name(), b.label());
        }
        std::process::exit(2);
    }
    println!();
    println!("Every A2A method was exercised over every binding, and every");
    println!("counter-test was refused as the specification requires.");

    // ── Act 5: the parts of a deployment that are not the protocol ────────
    //
    // Acts 1-4 cover the protocol completely and say nothing about running
    // this for more than one caller. Each check below exercises a shipped SDK
    // capability over a socket and asserts the specific wrong answer is not
    // what came back; see `hardening/mod.rs` for why each one is shaped the
    // way it is.
    println!();
    println!("ACT 5 — production hardening: tenancy, auth, limits, durability, telemetry");
    let hardening_checks = hardening::run().await;
    let hardening_failures = hardening::report(&hardening_checks);
    if hardening_failures > 0 {
        println!();
        println!("{hardening_failures} hardening check(s) failed.");
        std::process::exit(3);
    }

    println!();
    // `INCIDENT_EXIT_WHEN_DONE=1` returns instead of parking on Ctrl+C, so CI
    // can gate on the exit code. Without it the demo stays up for a human to
    // poke at, which is the point of the three agents still serving.
    if std::env::var("INCIDENT_EXIT_WHEN_DONE").is_ok() {
        println!("Done (INCIDENT_EXIT_WHEN_DONE set — exiting).");
        return Ok(());
    }
    println!("Done. The three agents are still serving — probe them with curl or");
    println!("the TCK (cargo run -p a2a-tck -- --url {triage_url}),");
    println!("or press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
