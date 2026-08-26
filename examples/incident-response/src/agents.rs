// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The three agents, as executors.
//!
//! Each is an [`AgentExecutor`]: the SDK owns the task lifecycle, the transport
//! and the event queue, and what is left here is the part that is actually this
//! example's — what the agent does when a message arrives. Two of the three are
//! LLM-assisted and one is deliberately not, because an A2A agent is a
//! capability behind the protocol, not necessarily a model.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::Duration;

use a2a_protocol_client::ClientBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use crate::{
    agent_message, complete, emit_artifact, extract_text, find_service, known_services, progress,
    send_params, try_llm, user_message, INCIDENT_LOG, RUNBOOKS,
};

#[cfg(test)]
mod tests;

// ── Agent 1: logs — a deterministic tool agent (no LLM) ─────────────────────

/// Searches the bundled service log for lines mentioning a service.
///
/// Deliberately not LLM-backed: an A2A agent is a *capability behind the
/// protocol*. Tools with exact semantics (log search, database lookups,
/// deployments) should stay deterministic; the judgment lives in the agents
/// that call them.
pub(crate) struct LogSearchExecutor;

impl AgentExecutor for LogSearchExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let query = extract_text(&ctx.message.parts);
            let service = find_service(&query).ok_or_else(|| {
                A2aError::invalid_params(format!(
                    "no known service in query; known services: {}",
                    known_services().join(", ")
                ))
            })?;

            progress(queue, ctx, &format!("searching log for '{service}'")).await?;

            let matches: Vec<&str> = INCIDENT_LOG
                .lines()
                .filter(|l| l.contains(service))
                .collect();
            let errors = matches.iter().filter(|l| l.contains("ERROR")).count();
            let warns = matches.iter().filter(|l| l.contains("WARN")).count();

            let report = format!(
                "{} lines for '{service}' ({errors} ERROR, {warns} WARN):\n{}",
                matches.len(),
                matches.join("\n"),
            );
            emit_artifact(queue, ctx, "log-findings", &report).await?;
            complete(queue, ctx).await
        })
    }
}

// ── Agent 2: runbook — LLM-assisted with graceful degradation ───────────────

/// Returns the runbook section for a service, AI-summarized when a model is
/// reachable and verbatim when not — degraded output is labeled, never
/// silent.
pub(crate) struct RunbookExecutor {
    pub(crate) client: genai::Client,
    pub(crate) model: String,
}

impl AgentExecutor for RunbookExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let query = extract_text(&ctx.message.parts);
            let service = find_service(&query).ok_or_else(|| {
                A2aError::invalid_params(format!(
                    "no known service in query; known services: {}",
                    known_services().join(", ")
                ))
            })?;

            // Slice this service's section out of the bundled runbook file.
            let marker = format!("## {service}");
            let start = RUNBOOKS
                .find(&marker)
                .ok_or_else(|| A2aError::internal("runbook section disappeared"))?;
            let rest = &RUNBOOKS[start + marker.len()..];
            let end = rest.find("\n## ").unwrap_or(rest.len());
            let section = rest[..end].trim();

            progress(queue, ctx, &format!("loading runbook for '{service}'")).await?;

            let guidance = match try_llm(
                &self.client,
                &self.model,
                &format!(
                    "Summarize this runbook into at most 3 imperative remediation \
                     steps for an on-call engineer:\n\n{section}"
                ),
            )
            .await
            {
                Some(summary) => format!("[AI summary — model: {}]\n{summary}", self.model),
                None => format!("[verbatim runbook — no model reachable]\n{section}"),
            };

            emit_artifact(queue, ctx, "runbook-guidance", &guidance).await?;
            complete(queue, ctx).await
        })
    }
}

// ── Agent 3: triage — the orchestrator ───────────────────────────────────────

/// Coordinates an incident: pauses the task for missing input, delegates to
/// the logs and runbook agents over real A2A client calls, and synthesizes an
/// incident report.
pub(crate) struct TriageExecutor {
    pub(crate) client: genai::Client,
    pub(crate) model: String,
    pub(crate) logs_url: String,
    pub(crate) runbook_url: String,
    /// Alerts parked in `INPUT_REQUIRED`, keyed by task id. The original
    /// alert is also recoverable from `ctx.stored_task` history; keeping it
    /// here shows that agents are allowed to hold working state across the
    /// turns of one task.
    pub(crate) pending: Mutex<HashMap<String, String>>,
}

impl TriageExecutor {
    /// Delegates one question to a specialist agent and returns the first
    /// artifact's text. Failures are reported inline so a missing specialist
    /// degrades the report instead of killing the incident response.
    async fn ask_specialist(&self, url: &str, what: &str, question: &str) -> String {
        let client = match ClientBuilder::new(url).build() {
            Ok(c) => c,
            Err(e) => return format!("[{what} agent unavailable: {e}]"),
        };
        match tokio::time::timeout(
            Duration::from_secs(90),
            client.send_message(send_params(user_message(question))),
        )
        .await
        {
            Ok(Ok(SendMessageResponse::Task(task))) => task
                .artifacts
                .as_deref()
                .and_then(<[Artifact]>::first)
                .map(|a| extract_text(&a.parts))
                .unwrap_or_else(|| format!("[{what} agent returned no artifact]")),
            Ok(Ok(_)) => format!("[{what} agent returned an unexpected response type]"),
            Ok(Err(e)) => format!("[{what} agent error: {e}]"),
            Err(_) => format!("[{what} agent timed out]"),
        }
    }
}

impl AgentExecutor for TriageExecutor {
    /// Cancellation is cooperative in A2A: the default implementation
    /// reports tasks as not cancelable, and executors opt in by overriding
    /// `cancel` to release whatever the task holds. Triage holds parked
    /// alerts, so cancel drops them; the server then records the Canceled
    /// state.
    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            self.pending.lock().unwrap().remove(&ctx.task_id.0);
            Ok(())
        })
    }

    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let incoming = extract_text(&ctx.message.parts);

            // Multi-turn: if this task previously parked in INPUT_REQUIRED,
            // this message is the answer — recombine it with the original
            // alert. This is the part a wrapped prompt cannot do: the task
            // id carries state across turns.
            let parked = self.pending.lock().unwrap().remove(&ctx.task_id.0);
            let alert = match parked {
                Some(original) => format!("{original}\n(operator follow-up: {incoming})"),
                None => incoming,
            };

            // The A2A state machine requires SUBMITTED → WORKING before any
            // interrupted state, so announce work before deciding to ask.
            progress(queue, ctx, "reading alert").await?;

            // Do we know which service this is about? If not, *ask* — the
            // task parks in INPUT_REQUIRED and resumes when the caller sends
            // the answer on the same task id.
            let Some(service) = find_service(&alert) else {
                self.pending
                    .lock()
                    .unwrap()
                    .insert(ctx.task_id.0.clone(), alert);
                return queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus {
                            state: TaskState::InputRequired,
                            message: Some(agent_message(&format!(
                                "Which service is affected? I have runbooks for: {}",
                                known_services().join(", ")
                            ))),
                            timestamp: None,
                        },
                        metadata: None,
                    }))
                    .await;
            };

            // Evidence gathering: real A2A calls to the specialist agents.
            progress(
                queue,
                ctx,
                &format!("triaging '{service}': querying log agent"),
            )
            .await?;
            let log_findings = self
                .ask_specialist(&self.logs_url, "log", &format!("service={service}"))
                .await;

            progress(queue, ctx, "querying runbook agent").await?;
            let runbook = self
                .ask_specialist(&self.runbook_url, "runbook", &format!("service={service}"))
                .await;

            // Synthesis: LLM if reachable, labeled mechanical fallback if not.
            progress(queue, ctx, "synthesizing incident report").await?;
            let synthesis = try_llm(
                &self.client,
                &self.model,
                &format!(
                    "Write a short incident report (3-6 sentences): likely root \
                     cause and next actions.\n\nAlert: {alert}\n\nLog findings:\n\
                     {log_findings}\n\nRunbook guidance:\n{runbook}"
                ),
            )
            .await
            .unwrap_or_else(|| {
                "[mechanical summary — no model reachable]\nSee the log findings and \
                 runbook guidance below; follow the runbook steps in order."
                    .to_string()
            });

            let report = format!(
                "INCIDENT REPORT — service: {service}\n\
                 ================================{pad}\n\n\
                 Alert:\n{alert}\n\n\
                 Assessment:\n{synthesis}\n\n\
                 Evidence (log agent):\n{log_findings}\n\n\
                 Guidance (runbook agent):\n{runbook}\n",
                pad = "=".repeat(service.len()),
            );
            emit_artifact(queue, ctx, "incident-report", &report).await?;
            complete(queue, ctx).await
        })
    }
}
