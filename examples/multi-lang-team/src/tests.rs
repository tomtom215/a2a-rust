// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the coordinator, its worker table, and what it says when nobody
//! answers.
//!
//! The interesting half of this example is what happens when the workers are
//! *not* running, which is the state a reader who has only cloned the repo is
//! in. Most tests here run in that state deliberately: no worker is started,
//! and the assertions are about the coordinator reporting that honestly rather
//! than presenting an empty fan-out as a successful round-trip.
//!
//! The exception is the Rust worker, which is in this package and so *can* be
//! started in-process. The last section does, and asserts that the fan-out
//! carries its reply — the round-trip the other four languages can only get
//! from a reader who installed their toolchains.

use std::collections::BTreeSet;

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{TaskId, TaskState};

use crate::{
    CoordinatorExecutor, SLOW_PREFIX, WORKERS, Worker, call_worker, extract_text,
    make_coordinator_card, worker,
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

/// Five languages, five ports, no collisions. Two workers sharing a port
/// would mean one language's answer silently attributed to the other, and the
/// combined artifact would still look complete.
#[test]
fn the_worker_table_is_five_distinct_languages_on_five_distinct_ports() {
    assert_eq!(WORKERS.len(), 5);

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

// ── The Rust worker ──────────────────────────────────────────────────────────

/// The coordinator dials the table's address; the binary listens on
/// `DEFAULT_ADDR`. If the two drift apart, the coordinator reports a running
/// Rust worker as `not reachable` and the reader blames the wrong side.
#[test]
fn the_worker_table_dials_the_address_the_rust_worker_binds_by_default() {
    let rust = WORKERS
        .iter()
        .find(|w| w.language == "Rust")
        .expect("the worker table has no Rust row");
    assert_eq!(rust.url, format!("http://{}", worker::DEFAULT_ADDR));
}

/// Starts the worker on an ephemeral port and returns a table entry for it.
///
/// `Worker` holds `&'static str` because the real table is a `const`; a
/// leaked string is the honest way to give a test-time address the same
/// lifetime, and a test process does not outlive the leak.
async fn start_rust_worker() -> &'static Worker {
    let addr = worker::start("127.0.0.1:0")
        .await
        .expect("the Rust worker did not start on an ephemeral port");
    Box::leak(Box::new(Worker {
        language: "Rust",
        url: Box::leak(format!("http://{addr}").into_boxed_str()),
    }))
}

/// The startup probe is `resolve_agent_card` against the worker's base URL.
/// The other four workers answer it from a hand-written card; this one must
/// answer it from the SDK's own card route, or it would never be delegated
/// to at all.
#[tokio::test]
async fn the_rust_worker_answers_the_probe_the_coordinator_uses() {
    let rust = start_rust_worker().await;
    let card = a2a_protocol_client::resolve_agent_card(rust.url)
        .await
        .expect("the coordinator's reachability probe failed against the Rust worker");
    assert_eq!(card.name, "Rust Echo Agent");
    assert!(
        card.skills.iter().any(|s| s.id == "echo"),
        "the card does not advertise the echo skill: {:?}",
        card.skills
    );
    assert_eq!(
        card.supported_interfaces[0].url, rust.url,
        "the card names an address other than the one it was served from"
    );
}

/// The whole round-trip, against a real worker: the coordinator's executor
/// fans out over the wire to the in-process Rust worker and the combined
/// artifact carries its reply, in the `[<Language> Echo] <text>` shape the
/// other workers use. This is the test the other four languages cannot have
/// without their toolchains, and the one that makes "cross-language
/// delegation" a tested claim rather than a README one.
#[tokio::test]
async fn the_fan_out_carries_the_rust_workers_reply() {
    let rust = start_rust_worker().await;
    let exec = CoordinatorExecutor {
        reachable: vec![rust],
    };
    let (result, events) = drive(&exec, &ctx("Hello from the multi-language team demo!")).await;

    assert!(
        result.is_ok(),
        "delegating to a live worker failed: {result:?}"
    );
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );

    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0].0, "cross-lang-result");
    assert_eq!(
        arts[0].1,
        format!(
            "{}Hello from the multi-language team demo!",
            worker::REPLY_PREFIX
        ),
        "the combined artifact is not the Rust worker's reply"
    );
    assert!(
        !arts[0].1.contains("no worker agents reachable"),
        "a live worker was reported as nobody"
    );
}
