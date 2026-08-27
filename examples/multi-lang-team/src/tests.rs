// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the coordinator, its worker table, and what it says when nobody
//! answers.
//!
//! The interesting half of this example is what happens when the workers are
//! *not* running, which is the state a reader who has only cloned the repo is
//! in. Every test here runs in that state deliberately: no worker is started,
//! and the assertions are about the coordinator reporting that honestly rather
//! than presenting an empty fan-out as a successful round-trip.

use std::collections::BTreeSet;

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{TaskId, TaskState};

use crate::{
    call_worker, extract_text, make_coordinator_card, CoordinatorExecutor, Worker, SLOW_PREFIX,
    WORKERS,
};

// ── Harness ──────────────────────────────────────────────────────────────────

fn ctx(text: &str) -> RequestContext {
    RequestContext::new(
        Message {
            id: MessageId::new("m-1"),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        TaskId::new("t-1"),
        "ctx-1".to_owned(),
    )
}

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

// ── The worker table ─────────────────────────────────────────────────────────

/// Four languages, four ports, no collisions. Two workers sharing a port
/// would mean one language's answer silently attributed to the other, and the
/// combined artifact would still look complete.
#[test]
fn the_worker_table_is_four_distinct_languages_on_four_distinct_ports() {
    assert_eq!(WORKERS.len(), 4);

    let languages: BTreeSet<_> = WORKERS.iter().map(|w| w.language).collect();
    assert_eq!(
        languages.len(),
        WORKERS.len(),
        "two workers share a language"
    );

    let urls: BTreeSet<_> = WORKERS.iter().map(|w| w.url).collect();
    assert_eq!(urls.len(), WORKERS.len(), "two workers share a url");
}

/// Every worker is on loopback. These addresses are dialled by an agent that
/// takes its prompt from a caller; pointing one at a routable host would make
/// this example a request forwarder for whoever can reach it.
#[test]
fn every_worker_is_on_loopback() {
    for w in WORKERS {
        assert!(
            w.url.starts_with("http://127.0.0.1:"),
            "{} is not on loopback: {}",
            w.language,
            w.url
        );
    }
}

// ── extract_text ─────────────────────────────────────────────────────────────

#[test]
fn extract_text_joins_text_parts_and_ignores_the_rest() {
    let parts = vec![
        Part::text("hello"),
        Part {
            content: PartContent::Data(serde_json::json!({"k": "v"})),
            ..Part::text("")
        },
        Part::text("world"),
    ];
    assert_eq!(extract_text(&parts), "hello world");
    assert_eq!(extract_text(&[]), "");
}

// ── A worker that is not there ───────────────────────────────────────────────

/// A dead worker becomes a labelled line, not a panic and not a silent
/// omission. Partial results are this coordinator's contract, and a line the
/// reader can attribute to a language is what makes them partial rather than
/// wrong.
#[tokio::test]
async fn an_unreachable_worker_is_reported_as_a_labelled_line() {
    static DEAD: Worker = Worker {
        language: "Nowhere",
        // Port 1 is reserved and never listening: connection refused, at once.
        url: "http://127.0.0.1:1",
    };
    let line = call_worker(&DEAD, "anything").await;
    assert!(
        line.starts_with("[Nowhere]"),
        "the line does not name its worker: {line}"
    );
}

// ── The coordinator with nobody to delegate to ───────────────────────────────

/// With no reachable workers the artifact must *say so*. An empty fan-out that
/// produced an empty artifact would be indistinguishable from a successful
/// cross-language round-trip that happened to return nothing, which is the
/// claim this example exists to make honestly.
#[tokio::test]
async fn with_no_workers_the_artifact_says_nothing_was_delegated() {
    let exec = CoordinatorExecutor { reachable: vec![] };
    let (result, events) = drive(&exec, &ctx("hello")).await;

    assert!(result.is_ok(), "delegating to nobody is not an error");
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );

    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0].0, "cross-lang-result");
    assert!(
        arts[0].1.starts_with("[no worker agents reachable"),
        "the artifact does not disclose that nothing was delegated: {}",
        arts[0].1
    );
    assert!(
        arts[0].1.contains("not a cross-language round-trip"),
        "the disclosure does not say what the run was not"
    );
}

/// The slow marker delays the turn, which is what makes a non-terminal task
/// observable for `SubscribeToTask`. Asserted on tokio's paused clock, so it
/// costs no wall-clock time and cannot flake on a loaded runner.
#[tokio::test(start_paused = true)]
async fn the_slow_marker_holds_the_task_open() {
    let exec = CoordinatorExecutor { reachable: vec![] };

    let start = tokio::time::Instant::now();
    drive(&exec, &ctx("plain")).await.0.expect("plain turn");
    let plain = start.elapsed();

    let start = tokio::time::Instant::now();
    drive(&exec, &ctx(&format!("{SLOW_PREFIX}please wait")))
        .await
        .0
        .expect("slow turn");
    let slow = start.elapsed();

    assert!(
        slow >= std::time::Duration::from_millis(400),
        "the slow marker did not delay the turn: {slow:?}"
    );
    assert!(plain < slow, "a plain turn took as long as a slow one");
}

// ── The card ─────────────────────────────────────────────────────────────────

/// The card must advertise the optional capabilities the surface sweep needs,
/// because the server *refuses* those methods when the card does not claim
/// them — the methods would be unavailable, not merely undriven.
#[test]
fn the_card_advertises_the_capabilities_the_sweep_requires() {
    let card = make_coordinator_card("http://127.0.0.1:9000");
    // `Option<bool>`, and the distinction matters: `None` is "the card does
    // not say", which the server treats as not supported just as `Some(false)`
    // does. Asserting `Some(true)` is asserting the card actually claims it.
    assert_eq!(
        card.capabilities.streaming,
        Some(true),
        "streaming not advertised"
    );
    assert_eq!(
        card.capabilities.push_notifications,
        Some(true),
        "push notifications not advertised"
    );
    assert_eq!(
        card.capabilities.extended_agent_card,
        Some(true),
        "extended card not advertised"
    );
    assert!(!card.skills.is_empty(), "a card with no skills");
    assert!(!card.name.is_empty());
}
