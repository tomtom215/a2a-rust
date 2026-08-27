// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the rig executor, against models that never touch a network.
//!
//! `RigAgentExecutor` is generic over `CompletionModel`, which means the
//! provider can be replaced outright rather than pointed somewhere dead. Two
//! fakes below cover both branches: one that always fails, and — the part no
//! other test in this repository can do — one that *answers*. The success path
//! of an LLM-backed example is normally only reachable with a model running;
//! here it is reachable in a unit test, so the assertion can be that the
//! model's text arrives verbatim and unlabelled.

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_types::error::{A2aResult, ErrorCode};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{TaskId, TaskState};
use rig_core::completion::{
    CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
};

use crate::{make_agent_card, RigAgentExecutor, SLOW_PREFIX};

// ── Two models, neither of which has a provider ──────────────────────────────

/// Always fails, the way an unreachable provider does.
#[derive(Clone)]
struct FailingModel;

/// Always answers with a fixed string.
#[derive(Clone)]
struct AnsweringModel(&'static str);

const ANSWER: &str = "the model's own words";

impl CompletionModel for FailingModel {
    type Response = ();
    type StreamingResponse = ();
    type Client = ();

    fn make(_client: &Self::Client, _model: impl Into<String>) -> Self {
        Self
    }

    async fn completion(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
        Err(CompletionError::ProviderError("no provider".to_owned()))
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<
        rig_core::streaming::StreamingCompletionResponse<Self::StreamingResponse>,
        CompletionError,
    > {
        Err(CompletionError::ProviderError("no provider".to_owned()))
    }
}

impl CompletionModel for AnsweringModel {
    type Response = ();
    type StreamingResponse = ();
    type Client = ();

    fn make(_client: &Self::Client, _model: impl Into<String>) -> Self {
        Self(ANSWER)
    }

    async fn completion(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
        Ok(CompletionResponse {
            choice: rig_core::OneOrMany::one(
                rig_core::completion::message::AssistantContent::text(self.0),
            ),
            usage: rig_core::completion::Usage::new(),
            raw_response: (),
            message_id: None,
        })
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<
        rig_core::streaming::StreamingCompletionResponse<Self::StreamingResponse>,
        CompletionError,
    > {
        Err(CompletionError::ProviderError(
            "streaming not faked".to_owned(),
        ))
    }
}

fn executor<M: CompletionModel + 'static>(
    model: M,
    fallback_on_error: bool,
) -> RigAgentExecutor<M> {
    RigAgentExecutor {
        agent: rig_core::agent::AgentBuilder::new(model)
            .preamble("You are a helpful A2A protocol agent.")
            .build(),
        fallback_on_error,
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

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

// ── The success path ─────────────────────────────────────────────────────────

/// The model's answer reaches the artifact unchanged and *unlabelled*.
///
/// The label is what marks a mechanical fallback, so its absence here is the
/// assertion: a real answer must not be dressed as one, and a fallback must
/// not be mistaken for a real answer.
#[tokio::test]
async fn a_model_answer_reaches_the_artifact_verbatim() {
    let exec = executor(AnsweringModel(ANSWER), true);
    let (result, events) = drive(&exec, &ctx("hello")).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );
    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert_eq!(arts[0].0, "rig-response");
    assert_eq!(arts[0].1, ANSWER);
    assert!(
        !arts[0].1.contains("mechanical fallback"),
        "a real answer was labelled as a fallback"
    );
}

// ── Input validation ─────────────────────────────────────────────────────────

/// A textless message is refused before the model is prompted — prompting with
/// an empty string spends somebody's tokens answering nothing.
#[tokio::test]
async fn a_message_with_no_text_part_is_refused() {
    let exec = executor(AnsweringModel(ANSWER), true);
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

#[tokio::test]
async fn a_failing_provider_produces_a_labelled_fallback() {
    let exec = executor(FailingModel, true);
    let (result, events) = drive(&exec, &ctx("hello")).await;

    assert!(
        result.is_ok(),
        "the fallback must not fail the task: {result:?}"
    );
    let arts = artifacts(&events);
    assert_eq!(arts.len(), 1);
    assert!(
        arts[0]
            .1
            .starts_with("[no model reachable — mechanical fallback, not an LLM answer]"),
        "the fallback is unlabelled: {}",
        arts[0].1
    );
    assert!(arts[0].1.contains("hello"), "the echo lost the input");
}

/// Server mode leaves the fallback off, and there a provider failure must fail
/// the task rather than answer mechanically.
#[tokio::test]
async fn without_the_fallback_a_failing_provider_fails_the_task() {
    let exec = executor(FailingModel, false);
    let (result, events) = drive(&exec, &ctx("hello")).await;

    let err = result.expect_err("a provider failure must fail the task");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert!(err.to_string().contains("rig agent error"), "{err}");
    assert!(
        artifacts(&events).is_empty(),
        "a failed task must not deliver an artifact"
    );
}

/// The slow marker holds the task open for `SubscribeToTask`. On a paused
/// clock, so it costs no wall-clock time.
#[tokio::test(start_paused = true)]
async fn the_slow_marker_holds_the_task_open() {
    let exec = executor(AnsweringModel(ANSWER), true);

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

#[test]
fn the_card_advertises_the_capabilities_the_sweep_requires() {
    let card = make_agent_card("http://127.0.0.1:9000", "some-model");
    assert_eq!(card.capabilities.streaming, Some(true));
    assert_eq!(card.capabilities.push_notifications, Some(true));
    assert_eq!(card.capabilities.extended_agent_card, Some(true));
    assert!(
        card.description.contains("some-model"),
        "{}",
        card.description
    );
    assert_eq!(card.url.as_deref(), Some("http://127.0.0.1:9000"));
    assert!(!card.skills.is_empty());
}
