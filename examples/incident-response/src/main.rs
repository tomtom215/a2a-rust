// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Incident-response agent team — what agents actually are, hands-on.
//!
//! A wrapped prompt answers once and forgets. An agent holds a **task**: it
//! gathers evidence with tools, delegates to other agents, pauses to ask the
//! caller for missing information, streams progress while it works, can be
//! cancelled mid-flight, and ends in an honest terminal state. This example
//! demonstrates every one of those properties with three cooperating A2A
//! agents you can run on a laptop:
//!
//! | Agent     | Port | Brain        | Job |
//! |-----------|------|--------------|-----|
//! | `triage`  | 9200 | LLM + tools  | Orchestrates an incident: collects evidence from the other two agents, asks for missing info (`INPUT_REQUIRED`), synthesizes an incident report |
//! | `logs`    | 9201 | none (deterministic) | Searches the service log for a service's lines — proof that an "agent" is a capability behind the protocol, not necessarily an LLM |
//! | `runbook` | 9202 | LLM (optional) | Returns the operational runbook for a service, AI-summarized when a model is available, verbatim otherwise |
//!
//! The demo runs in five acts. Acts 1-3 are the agent story: a task that pauses
//! for missing input, resumes on the same task while three agents collaborate,
//! and can be cancelled. Act 4 drives every A2A method over every binding and
//! fails if a cell never ran. Act 5 leaves the protocol behind and exercises
//! what a deployment needs — tenant isolation, authentication, rate limiting,
//! persistence across a restart, card signing, telemetry and graceful shutdown
//! — each asserted, not narrated.
//!
//! Run `cargo run -p incident-response` for a narrated end-to-end demo, or
//! start each role separately (`-- triage|logs|runbook`) and drive them with
//! any A2A client. See the README for the local-model setup (llama-server /
//! Ollama on `:11434`) — the demo also works with no model at all, falling
//! back to mechanical summaries so the protocol mechanics stay visible.
//!
//! Exit codes: `1` a surface call failed, `2` a method x binding cell never
//! ran, `3` a hardening check failed, `4` a hardening capability went
//! unexercised while `INCIDENT_REQUIRE_ALL` was set.

mod agents;
mod demo;
mod hardening;
mod serving;

use std::time::Duration;

use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use crate::serving::{start_logs_agent, start_runbook_agent, start_triage_agent};

/// Bundled at compile time so the demo has zero filesystem setup.
const INCIDENT_LOG: &str = include_str!("../data/incident.log");
const RUNBOOKS: &str = include_str!("../data/runbooks.md");

const TRIAGE_PORT: u16 = 9200;
const LOGS_PORT: u16 = 9201;
const RUNBOOK_PORT: u16 = 9202;

/// Message-text marker that leaves a triage task parked in `INPUT_REQUIRED`.
///
/// Any alert naming no known service parks awaiting input — that is Act 1's
/// whole point — so Act 4's surface sweep reuses it to obtain a non-terminal
/// task for `SubscribeToTask`, rather than inventing a sleep.
const SURFACE_PAUSE_PREFIX: &str = "vague alert, no service named: ";

// ── Small shared helpers ─────────────────────────────────────────────────────

fn extract_text(parts: &[Part]) -> String {
    parts
        .iter()
        .filter_map(|p| match &p.content {
            PartContent::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn agent_message(text: &str) -> Message {
    Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::Agent,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn user_message(text: &str) -> Message {
    Message {
        id: MessageId::new(uuid::Uuid::new_v4().to_string()),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn send_params(message: Message) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message,
        configuration: None,
        metadata: None,
    }
}

/// Emits a Working status carrying a human-readable progress note.
///
/// Status messages are how an agent narrates long-running work — a streaming
/// client sees these the moment they happen.
async fn progress(queue: &dyn EventQueueWriter, ctx: &RequestContext, note: &str) -> A2aResult<()> {
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus {
                state: TaskState::Working,
                message: Some(agent_message(note)),
                timestamp: None,
            },
            metadata: None,
        }))
        .await
}

async fn emit_artifact(
    queue: &dyn EventQueueWriter,
    ctx: &RequestContext,
    name: &str,
    text: &str,
) -> A2aResult<()> {
    queue
        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            artifact: Artifact::new(name, vec![Part::text(text)]),
            append: None,
            last_chunk: Some(true),
            metadata: None,
        }))
        .await
}

async fn complete(queue: &dyn EventQueueWriter, ctx: &RequestContext) -> A2aResult<()> {
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
            task_id: ctx.task_id.clone(),
            context_id: ContextId::new(ctx.context_id.clone()),
            status: TaskStatus::new(TaskState::Completed),
            metadata: None,
        }))
        .await
}

/// Names of the services that have a runbook section (`## <service>`).
fn known_services() -> Vec<&'static str> {
    RUNBOOKS
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .collect()
}

/// Finds the first known service mentioned in `text`, if any.
fn find_service(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    known_services().into_iter().find(|s| lower.contains(*s))
}

/// Optional LLM completion via genai. Model names that don't match a hosted
/// provider route to the local Ollama-compatible endpoint on `:11434`.
/// Returns `None` (rather than failing the task) when no model is reachable,
/// so the team degrades to mechanical summaries instead of going dark.
async fn try_llm(client: &genai::Client, model: &str, prompt: &str) -> Option<String> {
    let req = genai::chat::ChatRequest::new(vec![
        genai::chat::ChatMessage::system(
            "You are a concise SRE assistant. Answer in plain text, no markdown headers.",
        ),
        genai::chat::ChatMessage::user(prompt),
    ]);
    let resp = tokio::time::timeout(Duration::from_secs(60), client.exec_chat(model, req, None))
        .await
        .ok()?
        .ok()?;
    resp.first_text().map(str::to_string)
}

fn incident_model() -> String {
    std::env::var("INCIDENT_MODEL").unwrap_or_else(|_| "qwen3.5:0.8b".to_string())
}

// ── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let role = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "demo".to_string());
    match role.as_str() {
        "demo" => demo::run().await,
        "logs" => {
            let ep = start_logs_agent().await?;
            println!("logs agent listening on {}", ep.http);
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        "runbook" => {
            let ep = start_runbook_agent().await?;
            println!("runbook agent listening on {}", ep.http);
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        "triage" => {
            let ep = start_triage_agent(
                format!("http://127.0.0.1:{LOGS_PORT}"),
                format!("http://127.0.0.1:{RUNBOOK_PORT}"),
            )
            .await?;
            println!("triage agent listening on {}", ep.http);
            println!("(expects logs on :{LOGS_PORT} and runbook on :{RUNBOOK_PORT})");
            tokio::signal::ctrl_c().await?;
            Ok(())
        }
        // Act 5 on its own, for when the question is "is this deployable?"
        // rather than "what is an agent?". Same checks the demo runs, same
        // exit code, without the three-agent narrative in front of them.
        "harden" => {
            println!("Production hardening checks");
            println!("===========================");
            println!();
            let checks = hardening::run().await;
            if hardening::report(&checks) > 0 {
                std::process::exit(3);
            }
            hardening::require_all_exercised(&checks);
            Ok(())
        }
        other => {
            eprintln!("unknown role '{other}' — use: demo | triage | logs | runbook | harden");
            std::process::exit(2);
        }
    }
}
