// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the genai executor, with the provider pinned somewhere dead.
//!
//! The two branches worth testing are what happens when the model answers and
//! what happens when it cannot be reached, and only the second is reachable
//! without a model. Rather than assert "either outcome is fine" — which passes
//! whatever happens and so tests nothing — every test here overrides genai's
//! service target to a port nothing listens on. The provider call then fails
//! the same way on a developer's laptop with `llama-server` running and on a
//! CI runner with nothing, so the fallback branch is exercised deterministically
//! instead of incidentally.

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_types::error::{A2aResult, ErrorCode};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{TaskId, TaskState};

use crate::{make_agent_card, GenaiAgentExecutor, SLOW_PREFIX};

/// A genai client whose every request goes to a port nothing listens on.
///
/// Port 1 is reserved and never served, so the connection is refused at once —
/// no timeout to wait out, and no dependence on what happens to be running on
/// the machine.
fn unreachable_client() -> genai::Client {
    genai::Client::builder()
        .with_service_target_resolver_fn(|mut target: genai::ServiceTarget| {
            target.endpoint = genai::resolver::Endpoint::from_static("http://127.0.0.1:1/v1/");
            Ok(target)
        })
        .build()
}

fn executor(fallback_on_error: bool) -> GenaiAgentExecutor {
    GenaiAgentExecutor {
        client: unreachable_client(),
        model: "no-such-model".to_owned(),
        fallback_on_error,
    }
}

fn ctx_with(parts: Vec<Part>) -> RequestContext {
    RequestContext::new(
        Message {
            id: MessageId::new("m-1"),
            role: MessageRole::User,
            parts,
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

fn ctx(text: &str) -> RequestContext {
    ctx_with(vec![Part::text(text)])
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
            StreamResponse::ArtifactUpdate(a) => Some((
                a.artifact.id.0.clone(),
                a.artifact
                    .parts
                    .iter()
                    .filter_map(|p| match &p.content {
                        PartContent::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            _ => None,
        })
        .collect()
}

// ── Input validation ─────────────────────────────────────────────────────────

/// A message with no text part is a client error, and must be refused *before*
/// the provider is called — prompting a model with an empty string spends
/// somebody's tokens to produce an answer to nothing.
#[tokio::test]
async fn a_message_with_no_text_part_is_refused() {
    let exec = executor(true);
    let (result, events) = drive(&exec, &ctx_with(vec![Part::raw("aGk=")])).await;

    let err = result.expect_err("a textless message must not succeed");
    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert!(err.to_string().contains("no text part"), "{err}");
    assert!(
        events.is_empty(),
        "the task was moved to Working before the input was checked"
    );
}

// ── The fallback, and its label ──────────────────────────────────────────────

/// With the fallback on and no provider, the task completes and the artifact
/// says plainly that no model answered.
///
/// The label is the point. Without it "the agent answered" and "the model
/// answered" are indistinguishable, and a reader running this example with
/// nothing on :11434 would conclude the LLM integration works.
#[tokio::test]
async fn an_unreachable_model_produces_a_labelled_fallback() {
    let exec = executor(true);
    let (result, events) = drive(&exec, &ctx("hello")).await;

    assert!(
        result.is_ok(),
        "the fallback must not fail the task: {result:?}"
    );
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );

    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0].0, "llm-response");
    assert!(
        arts[0]
            .1
            .starts_with("[no model reachable — mechanical fallback, not an LLM answer]"),
        "the fallback is unlabelled: {}",
        arts[0].1
    );
    // The reader is told what was echoed and why, not just that something failed.
    assert!(arts[0].1.contains("hello"), "the echo lost the input");
    assert!(
        arts[0].1.contains("underlying error"),
        "the fallback hides why the model was unreachable"
    );
}

/// Without the fallback — which is what server mode uses — the same
/// unreachable provider fails the task instead. A real deployment must not
/// answer mechanically and call it a completion.
#[tokio::test]
async fn without_the_fallback_an_unreachable_model_fails_the_task() {
    let exec = executor(false);
    let (result, events) = drive(&exec, &ctx("hello")).await;

    let err = result.expect_err("an unreachable provider must fail the task");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert!(err.to_string().contains("LLM request failed"), "{err}");
    assert!(
        artifacts(&events).is_empty(),
        "a failed task must not deliver an artifact"
    );
}

/// The two modes differ only in this, and the difference is the whole reason
/// both exist: the surface run needs a completion it can measure, a deployment
/// needs the truth.
#[tokio::test]
async fn the_fallback_flag_is_the_only_difference_between_the_modes() {
    assert!(drive(&executor(true), &ctx("x")).await.0.is_ok());
    assert!(drive(&executor(false), &ctx("x")).await.0.is_err());
}

/// The slow marker holds the task open so `SubscribeToTask` has a
/// non-terminal task to attach to. On tokio's paused clock, so it costs no
/// wall-clock time and cannot flake on a loaded runner.
#[tokio::test(start_paused = true)]
async fn the_slow_marker_holds_the_task_open() {
    let exec = executor(true);

    let start = tokio::time::Instant::now();
    drive(&exec, &ctx("plain")).await.0.expect("plain turn");
    let plain = start.elapsed();

    let start = tokio::time::Instant::now();
    drive(&exec, &ctx(&format!("{SLOW_PREFIX}wait")))
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

/// The server *refuses* the optional methods when the card does not claim
/// them, so a card that under-advertises makes seven of the eleven methods
/// unavailable rather than merely undriven.
#[test]
fn the_card_advertises_the_capabilities_the_sweep_requires() {
    let card = make_agent_card("http://127.0.0.1:9000", "some-model");
    assert_eq!(card.capabilities.streaming, Some(true));
    assert_eq!(card.capabilities.push_notifications, Some(true));
    assert_eq!(card.capabilities.extended_agent_card, Some(true));

    // The model name reaches the description, so a reader can tell which model
    // a running agent is actually backed by.
    assert!(
        card.description.contains("some-model"),
        "{}",
        card.description
    );
    assert_eq!(card.url.as_deref(), Some("http://127.0.0.1:9000"));
    assert!(!card.skills.is_empty());
}
