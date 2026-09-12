// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`a2a_rig::RigExecutor`] against models that never touch a network.
//!
//! `RigExecutor` is generic over `CompletionModel`, so the provider is replaced
//! outright rather than pointed somewhere dead. That makes the *success* path
//! testable offline, which is what lets these assert the model's text arrives
//! verbatim instead of only that a failure is reported.

use std::sync::{Arc, Mutex};

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueReader;
use a2a_protocol_server::streaming::event_queue::new_in_memory_queue;
use a2a_protocol_types::error::{A2aResult, ErrorCode};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::task::{TaskId, TaskState};
use a2a_rig::RigExecutor;
use rig_core::completion::{
    AssistantContent, CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
    Usage,
};
use rig_core::streaming::StreamingCompletionResponse;

const ANSWER: &str = "the model's own words";

/// Answers with a fixed string, and records the chat history it was sent.
#[derive(Clone, Default)]
struct Answering {
    seen_history: Arc<Mutex<Vec<String>>>,
}

impl CompletionModel for Answering {
    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CompletionError> {
        // rig converts a builder preamble into a leading system message and
        // always leaves `request.preamble` as None, so the history is where a
        // system instruction is observable.
        *self.seen_history.lock().expect("history lock") = request
            .chat_history
            .iter()
            .map(|m| format!("{m:?}"))
            .collect();
        Ok(CompletionResponse::new(
            vec![AssistantContent::text(ANSWER)],
            Usage::new(),
            "answering-fake",
        ))
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse, CompletionError> {
        Err(CompletionError::ProviderError("not faked".to_owned()))
    }
}

/// Fails the way an unreachable provider does.
#[derive(Clone)]
struct Failing;

impl CompletionModel for Failing {
    async fn completion(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, CompletionError> {
        Err(CompletionError::ProviderError("no provider".to_owned()))
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse, CompletionError> {
        Err(CompletionError::ProviderError("no provider".to_owned()))
    }
}

/// Never returns within the test's lifetime, so cancellation is what ends it.
#[derive(Clone)]
struct Hanging;

impl CompletionModel for Hanging {
    async fn completion(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, CompletionError> {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        unreachable!("the cancellation branch must win")
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse, CompletionError> {
        Err(CompletionError::ProviderError("not faked".to_owned()))
    }
}

fn ctx(text: &str) -> RequestContext {
    ctx_with(vec![Part::text(text)])
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

fn artifact_text(events: &[StreamResponse]) -> Option<(String, String)> {
    events.iter().find_map(|e| match e {
        StreamResponse::ArtifactUpdate(a) => {
            let text = a.artifact.parts.iter().find_map(|p| match &p.content {
                PartContent::Text(t) => Some(t.clone()),
                _ => None,
            })?;
            Some((a.artifact.id.to_string(), text))
        }
        _ => None,
    })
}

#[tokio::test]
async fn the_models_text_arrives_verbatim_and_the_task_completes() {
    let exec = RigExecutor::new(Answering::default());
    let (result, events) = drive(&exec, &ctx("hello")).await;

    assert!(result.is_ok(), "expected success, got {result:?}");
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );
    let (name, text) = artifact_text(&events).expect("an artifact was emitted");
    assert_eq!(name, "rig-response", "the default artifactId");
    assert_eq!(
        text, ANSWER,
        "the artifact must carry the model's text unaltered"
    );
}

#[tokio::test]
async fn a_message_with_no_text_part_is_invalid_params() {
    let exec = RigExecutor::new(Answering::default());
    // A file part, so the message is well-formed but carries nothing to prompt with.
    let (result, events) = drive(&exec, &ctx_with(vec![])).await;

    let err = result.expect_err("a message with no text must not reach the model");
    assert_eq!(err.code, ErrorCode::InvalidParams);
    assert!(
        states(&events).is_empty(),
        "nothing should be emitted before the input is validated, got {:?}",
        states(&events)
    );
}

#[tokio::test]
async fn a_provider_failure_fails_the_task_rather_than_answering() {
    let exec = RigExecutor::new(Failing);
    let (result, events) = drive(&exec, &ctx("hello")).await;

    let err = result.expect_err("a provider failure must fail the task");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert_eq!(states(&events), vec![TaskState::Working]);
    assert!(
        artifact_text(&events).is_none(),
        "a failed completion must not emit an artifact"
    );
}

#[tokio::test]
async fn cancellation_stops_waiting_on_the_provider_and_ends_canceled() {
    let exec = RigExecutor::new(Hanging);
    let ctx = ctx("hello");
    // Cancel before execute: the select is `biased`, so the cancellation branch
    // is checked first and this is deterministic rather than a race.
    ctx.cancellation_token.cancel();

    let (result, events) = drive(&exec, &ctx).await;

    assert!(
        result.is_ok(),
        "a cancelled task is not a failure, got {result:?}"
    );
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Canceled]
    );
    assert!(
        !states(&events).contains(&TaskState::Completed),
        "Completed after Canceled is an invalid terminal transition"
    );
}

#[tokio::test]
async fn with_preamble_reaches_the_model_as_a_leading_system_message() {
    let model = Answering::default();
    let seen = Arc::clone(&model.seen_history);
    let exec = RigExecutor::new(model).with_preamble("be terse");

    let (result, _) = drive(&exec, &ctx("hello")).await;
    assert!(result.is_ok());

    let history = seen.lock().expect("history lock").clone();
    let first = history.first().expect("the model was sent a chat history");
    assert!(
        first.contains("be terse"),
        "with_preamble must not be inert; first history entry was {first}"
    );
    assert!(
        first.contains("System"),
        "the preamble must arrive as a system message, not a user turn; got {first}"
    );
}

#[tokio::test]
async fn without_a_preamble_the_history_carries_only_the_prompt() {
    let model = Answering::default();
    let seen = Arc::clone(&model.seen_history);
    let exec = RigExecutor::new(model);

    let (result, _) = drive(&exec, &ctx("hello")).await;
    assert!(result.is_ok());

    let history = seen.lock().expect("history lock").clone();
    assert_eq!(
        history.len(),
        1,
        "no system message should be invented: {history:?}"
    );
    assert!(!history[0].contains("System"), "got {:?}", history[0]);
}

#[tokio::test]
async fn with_artifact_id_changes_the_artifact_id() {
    let exec = RigExecutor::new(Answering::default()).with_artifact_id("answer");
    let (_, events) = drive(&exec, &ctx("hello")).await;

    let (id, _) = artifact_text(&events).expect("an artifact was emitted");
    assert_eq!(id, "answer", "with_artifact_id must not be inert");
}
