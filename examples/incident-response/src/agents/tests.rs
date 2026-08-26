// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the three executors, driven directly against a real event queue.
//!
//! No servers, no ports, no model. An [`AgentExecutor`] is just a function from
//! a [`RequestContext`] and a queue to a sequence of events, so the whole of
//! Act 1's behaviour — the pause, the question, the held alert, the release on
//! cancel — is reachable without starting anything.
//!
//! What is deliberately *not* asserted here is anything that depends on a model
//! being reachable. A developer with `llama-server` on `:11434` and CI with
//! nothing there must both pass, so the LLM-touching tests assert the invariant
//! the README actually claims — that degraded output is *labeled*, never silent
//! — rather than which branch was taken.

use std::collections::HashMap;
use std::sync::Mutex;

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_types::error::{A2aResult, ErrorCode};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::{TaskId, TaskState};

use super::{LogSearchExecutor, RunbookExecutor, TriageExecutor};
use crate::{extract_text, incident_model, known_services, user_message, INCIDENT_LOG};

// ── Harness ──────────────────────────────────────────────────────────────────

fn ctx_for(task: &str, text: &str) -> RequestContext {
    RequestContext::new(user_message(text), TaskId::new(task), format!("ctx-{task}"))
}

/// Runs an executor to completion and returns its result with every event it
/// wrote. Dropping the writer closes the channel, so the drain terminates
/// without needing a timeout or a guess about how many events to expect.
async fn drive(
    exec: &dyn AgentExecutor,
    ctx: &RequestContext,
) -> (A2aResult<()>, Vec<StreamResponse>) {
    let (writer, mut reader) = new_in_memory_queue();
    let result = exec.execute(ctx, &writer).await;
    drop(writer);

    let mut events = Vec::new();
    while let Some(item) = reader.read().await {
        events.push(item.expect("the queue delivered an error rather than an event"));
    }
    (result, events)
}

fn states(events: &[StreamResponse]) -> Vec<TaskState> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamResponse::StatusUpdate(s) => Some(s.status.state),
            _ => None,
        })
        .collect()
}

/// Artifacts as `(id, text)`. The executors label artifacts through
/// `Artifact::new`, which sets the *id*; `name` stays `None` and `demo.rs`
/// falls back to the id when printing. Asserting on the id is therefore
/// asserting on what a client actually sees.
fn artifacts(events: &[StreamResponse]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamResponse::ArtifactUpdate(a) => {
                Some((a.artifact.id.0.clone(), extract_text(&a.artifact.parts)))
            }
            _ => None,
        })
        .collect()
}

fn status_texts(events: &[StreamResponse]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            StreamResponse::StatusUpdate(s) => {
                s.status.message.as_ref().map(|m| extract_text(&m.parts))
            }
            _ => None,
        })
        .collect()
}

// ── Agent 1: logs, the deterministic one ─────────────────────────────────────

#[tokio::test]
async fn log_search_narrates_then_delivers_then_completes() {
    let ctx = ctx_for("t-logs", "service=payments-api");
    let (result, events) = drive(&LogSearchExecutor, &ctx).await;

    assert!(result.is_ok(), "deterministic search failed: {result:?}");
    // Working before the artifact, Completed after it: status messages
    // narrate, the artifact delivers.
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );
    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0].0, "log-findings");
    assert!(status_texts(&events)
        .iter()
        .any(|t| t.contains("searching log for 'payments-api'")));
}

#[tokio::test]
async fn log_search_counts_match_the_bundled_log() {
    // Exact numbers on purpose. Computing the expectation the same way the
    // executor does would assert nothing; these are the counts a reader can
    // verify by opening data/incident.log, and they catch a change to either
    // the filter or the data.
    let ctx = ctx_for("t-count", "service=payments-api");
    let (_, events) = drive(&LogSearchExecutor, &ctx).await;
    let (_, text) = artifacts(&events).remove(0);

    assert!(
        text.starts_with("11 lines for 'payments-api' (8 ERROR, 1 WARN):"),
        "unexpected header: {}",
        text.lines().next().unwrap_or_default()
    );
    // The evidence itself, not just the tally.
    let quoted = text.lines().skip(1).count();
    assert_eq!(quoted, 11, "the report must carry the matching lines");
    assert!(text.lines().skip(1).all(|l| l.contains("payments-api")));
}

#[tokio::test]
async fn log_search_does_not_match_a_service_that_merely_shares_a_prefix() {
    // The log also contains `payments-db` lines. A substring filter that
    // matched them would inflate the evidence with another service's rows.
    let ctx = ctx_for("t-prefix", "service=payments-api");
    let (_, events) = drive(&LogSearchExecutor, &ctx).await;
    let (_, text) = artifacts(&events).remove(0);

    assert!(
        INCIDENT_LOG.contains("payments-db"),
        "this test is only meaningful while the log has a same-prefix service"
    );
    assert!(
        !text
            .lines()
            .skip(1)
            .any(|l| l.contains("payments-db") && !l.contains("payments-api")),
        "payments-db lines leaked into the payments-api evidence"
    );
}

#[tokio::test]
async fn log_search_rejects_a_query_naming_no_known_service() {
    let ctx = ctx_for("t-unknown", "something is broken somewhere");
    let (result, events) = drive(&LogSearchExecutor, &ctx).await;

    let err = result.expect_err("a query naming no service must not succeed");
    assert_eq!(err.code, ErrorCode::InvalidParams);
    // The error has to tell the caller what it *could* have asked for.
    let msg = err.to_string();
    for service in known_services() {
        assert!(
            msg.contains(service),
            "error does not offer '{service}': {msg}"
        );
    }
    assert!(
        artifacts(&events).is_empty(),
        "a failed search must not deliver an artifact"
    );
}

// ── Agent 2: runbook, LLM-assisted with a labeled fallback ───────────────────

#[tokio::test]
async fn runbook_guidance_is_always_labeled_with_its_provenance() {
    let exec = RunbookExecutor {
        client: genai::Client::default(),
        model: incident_model(),
    };
    let service = known_services()[0];
    let ctx = ctx_for("t-runbook", &format!("service={service}"));
    let (result, events) = drive(&exec, &ctx).await;

    assert!(result.is_ok(), "runbook lookup failed: {result:?}");
    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0].0, "runbook-guidance");

    // Whether a model was reachable is an environment fact, and this suite
    // must pass either way. What must hold in both cases is that the reader
    // is told which they got — the README's claim is that degraded output is
    // labeled, never silent.
    let text = &arts[0].1;
    assert!(
        text.starts_with("[AI summary") || text.starts_with("[verbatim runbook"),
        "guidance carries no provenance label: {text}"
    );
    assert!(text.len() > "[verbatim runbook — no model reachable]".len());
}

#[tokio::test]
async fn runbook_rejects_a_service_it_has_no_section_for() {
    let exec = RunbookExecutor {
        client: genai::Client::default(),
        model: incident_model(),
    };
    let ctx = ctx_for("t-runbook-unknown", "please advise");
    let (result, _) = drive(&exec, &ctx).await;
    assert_eq!(
        result.expect_err("no service named").code,
        ErrorCode::InvalidParams
    );
}

// ── Agent 3: triage — the pause, and what it holds ───────────────────────────

fn triage() -> TriageExecutor {
    TriageExecutor {
        client: genai::Client::default(),
        model: incident_model(),
        // Unused by the parking path: it returns before delegating. Pointing
        // these at an address nothing serves keeps that honest — a test that
        // started reaching the network would fail rather than hang.
        logs_url: "http://127.0.0.1:1/logs".to_string(),
        runbook_url: "http://127.0.0.1:1/runbook".to_string(),
        pending: Mutex::new(HashMap::new()),
    }
}

#[tokio::test]
async fn a_vague_alert_parks_the_task_and_asks_which_service() {
    let exec = triage();
    let ctx = ctx_for("t-vague", "everything is on fire, pages are firing");
    let (result, events) = drive(&exec, &ctx).await;

    assert!(result.is_ok(), "parking is a normal outcome, not an error");
    // Working first: the A2A state machine requires SUBMITTED -> WORKING
    // before any interrupted state, so the question cannot be the first event.
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::InputRequired]
    );

    let question = status_texts(&events)
        .pop()
        .expect("the InputRequired status must carry the question");
    for service in known_services() {
        assert!(
            question.contains(service),
            "the question does not offer '{service}': {question}"
        );
    }
    assert!(
        artifacts(&events).is_empty(),
        "a parked task has produced no deliverable yet"
    );
}

#[tokio::test]
async fn the_parked_alert_is_held_against_its_own_task_id() {
    let exec = triage();
    let alert = "disk pressure, no service named";
    let ctx = ctx_for("t-hold", alert);
    assert!(drive(&exec, &ctx).await.0.is_ok());

    let pending = exec.pending.lock().unwrap();
    assert_eq!(
        pending.get("t-hold").map(String::as_str),
        Some(alert),
        "the original alert must survive the pause — that is what a task \
         carries and a wrapped prompt does not"
    );
    assert_eq!(pending.len(), 1);
}

#[tokio::test]
async fn two_vague_alerts_park_independently() {
    let exec = triage();
    assert!(drive(&exec, &ctx_for("t-a", "first vague alert"))
        .await
        .0
        .is_ok());
    assert!(drive(&exec, &ctx_for("t-b", "second vague alert"))
        .await
        .0
        .is_ok());

    let pending = exec.pending.lock().unwrap();
    assert_eq!(pending.len(), 2);
    assert_eq!(
        pending.get("t-a").map(String::as_str),
        Some("first vague alert")
    );
    assert_eq!(
        pending.get("t-b").map(String::as_str),
        Some("second vague alert")
    );
}

#[tokio::test]
async fn cancel_releases_only_the_cancelled_task() {
    let exec = triage();
    assert!(drive(&exec, &ctx_for("t-keep", "vague one"))
        .await
        .0
        .is_ok());
    assert!(drive(&exec, &ctx_for("t-drop", "vague two"))
        .await
        .0
        .is_ok());
    assert_eq!(exec.pending.lock().unwrap().len(), 2);

    // Cancellation is cooperative: the executor opts in by releasing what the
    // task holds. Nothing else may be released with it.
    let (writer, _reader) = new_in_memory_queue();
    let ctx = ctx_for("t-drop", "");
    exec.cancel(&ctx, &writer).await.expect("cancel");

    let pending = exec.pending.lock().unwrap();
    assert!(
        !pending.contains_key("t-drop"),
        "cancelled alert still held"
    );
    assert!(
        pending.contains_key("t-keep"),
        "cancel released a bystander"
    );
}

#[tokio::test]
async fn cancelling_a_task_that_never_parked_is_not_an_error() {
    let exec = triage();
    let (writer, _reader) = new_in_memory_queue();
    let ctx = ctx_for("t-never", "");
    assert!(exec.cancel(&ctx, &writer).await.is_ok());
    assert!(exec.pending.lock().unwrap().is_empty());
}
