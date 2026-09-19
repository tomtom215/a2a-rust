// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tool loop, driven by a scripted model.
//!
//! A third fake, scripted rather than fixed: it replays a list of turns and
//! records every request it was sent. That is what lets the loop be driven
//! through branches a real provider reaches only by chance — a tool error,
//! and a model that never stops calling tools — and it is the only way to
//! assert what the *agent* sent, which is where the two bugs that matter
//! live: a catalogue dropped after the first turn, and a tool result that
//! never reaches the model.
//!
//! Hand-rolled rather than `rig_core::test_utils::MockCompletionModel`, which
//! is real but sits behind rig-core's `test-utils` feature; the two fakes in
//! the parent module set the pattern, and an example is better off
//! self-contained than carrying a dependency feature for its own tests.
//!
//! What no scripted model can tell you is whether a *real* model calls the
//! tools at all. That is measured separately, and the result is in
//! `examples/rig-agent/README.md`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use a2a_protocol_types::error::ErrorCode;
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::task::TaskState;
use rig_core::completion::message::{ToolCall, ToolFunction};
use rig_core::completion::{
    AssistantContent, CompletionError, CompletionModel, CompletionRequest, CompletionResponse,
    Usage,
};
use rig_core::streaming::StreamingCompletionResponse;
use serde_json::json;

use super::{artifacts, ctx, drive, executor, states};
use crate::agent::MAX_TURNS;
use crate::tools::{LIST_SERVICES, SERVICE_STATUS};

/// Replays scripted turns and records what it was asked.
#[derive(Clone)]
struct ScriptedModel {
    turns: Arc<Mutex<VecDeque<Vec<AssistantContent>>>>,
    seen: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl ScriptedModel {
    fn new(turns: impl IntoIterator<Item = Vec<AssistantContent>>) -> Self {
        Self {
            turns: Arc::new(Mutex::new(turns.into_iter().collect())),
            seen: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The requests this model received, in order. `Clone` shares the
    /// recording, so a clone kept by the test observes what the executor's
    /// copy was sent.
    fn requests(&self) -> Vec<CompletionRequest> {
        self.seen
            .lock()
            .expect("no test panics while holding this")
            .clone()
    }

    fn request_count(&self) -> usize {
        self.requests().len()
    }
}

/// One scripted turn that answers with text.
fn says(text: &str) -> Vec<AssistantContent> {
    vec![AssistantContent::text(text)]
}

/// One scripted turn that asks for a tool.
fn calls(id: &str, name: &str, arguments: serde_json::Value) -> Vec<AssistantContent> {
    vec![AssistantContent::ToolCall(ToolCall::from_wire(
        id,
        ToolFunction::new(name.to_owned(), arguments),
    ))]
}

impl CompletionModel for ScriptedModel {
    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, CompletionError> {
        self.seen
            .lock()
            .expect("no test panics while holding this")
            .push(request);
        let next = self
            .turns
            .lock()
            .expect("no test panics while holding this")
            .pop_front();
        match next {
            Some(choice) => Ok(CompletionResponse::new(choice, Usage::new(), "scripted")),
            // Running past the script is a test bug, and a silent repeat of
            // the last turn would hide it as an infinite loop.
            None => Err(CompletionError::ProviderError(
                "the script ran out of turns".to_owned(),
            )),
        }
    }

    async fn stream(
        &self,
        _request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse, CompletionError> {
        Err(CompletionError::ProviderError(
            "streaming not scripted".to_owned(),
        ))
    }
}

/// Text blocks of the named artifact, or `None` if it was never emitted.
fn artifact(events: &[StreamResponse], id: &str) -> Option<String> {
    artifacts(events)
        .into_iter()
        .find(|(name, _)| name == id)
        .map(|(_, text)| text)
}

/// Every message the given request carried, as one JSON string to search.
fn history_of(request: &CompletionRequest) -> String {
    serde_json::to_string(&request.chat_history).expect("rig messages serialize")
}

#[tokio::test]
async fn a_tool_call_is_executed_and_its_result_reaches_the_next_turn() {
    let model = ScriptedModel::new([
        calls("call-1", SERVICE_STATUS, json!({ "service": "checkout" })),
        says("checkout is healthy on 2.8.0."),
    ]);
    let observer = model.clone();

    let (result, events) = drive(&executor(model, false), &ctx("how is checkout?")).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );
    assert_eq!(
        artifact(&events, "rig-response").as_deref(),
        Some("checkout is healthy on 2.8.0.")
    );

    // Two turns, because the first asked for a tool.
    assert_eq!(observer.request_count(), 2);

    // The tool's output has to be *in* the second request, or the model
    // answered from nothing and the loop is decorative.
    let replayed = history_of(&observer.requests()[1]);
    assert!(
        replayed.contains("healthy") && replayed.contains("2.8.0"),
        "the tool result never reached the model: {replayed}"
    );
}

#[tokio::test]
async fn the_tool_catalogue_is_sent_on_every_turn() {
    // A provider holds no state between turns. A loop that sends the
    // catalogue only on the first request leaves the model unable to call
    // anything afterwards — and still passes a single-round test.
    let model = ScriptedModel::new([
        calls("call-1", LIST_SERVICES, json!({})),
        calls("call-2", SERVICE_STATUS, json!({ "service": "search" })),
        says("search is healthy."),
    ]);
    let observer = model.clone();

    let (result, _) = drive(&executor(model, false), &ctx("how is the search service?")).await;
    assert!(result.is_ok(), "{result:?}");

    let requests = observer.requests();
    assert_eq!(requests.len(), 3);
    for (turn, request) in requests.iter().enumerate() {
        let names: Vec<&str> = request.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(
            names.contains(&LIST_SERVICES) && names.contains(&SERVICE_STATUS),
            "turn {turn} was sent {names:?}"
        );
    }
}

#[tokio::test]
async fn a_tool_error_is_answered_to_the_model_rather_than_failing_the_task() {
    // The recovery this makes possible is the point: the model asks for a
    // service that does not exist, is told so, discovers the inventory, and
    // answers. Raising the tool error instead would end the task at step one.
    let model = ScriptedModel::new([
        calls("call-1", SERVICE_STATUS, json!({ "service": "billing" })),
        calls("call-2", LIST_SERVICES, json!({})),
        says("There is no billing service; I have payments-api, checkout and search."),
    ]);
    let observer = model.clone();

    let (result, events) = drive(&executor(model, false), &ctx("how is billing?")).await;

    assert!(
        result.is_ok(),
        "a tool error must not fail the task: {result:?}"
    );
    assert_eq!(
        states(&events),
        vec![TaskState::Working, TaskState::Completed]
    );
    assert_eq!(observer.request_count(), 3);

    let replayed = history_of(&observer.requests()[1]);
    assert!(
        replayed.contains("no service named"),
        "the model was not told why the tool failed: {replayed}"
    );
}

#[tokio::test]
async fn the_trace_artifact_names_every_call_that_ran() {
    let model = ScriptedModel::new([
        calls("call-1", LIST_SERVICES, json!({})),
        calls(
            "call-2",
            SERVICE_STATUS,
            json!({ "service": "payments-api" }),
        ),
        says("payments-api is degraded."),
    ]);

    let (_, events) = drive(&executor(model, false), &ctx("how is payments-api?")).await;

    let trace = artifact(&events, "tool-trace").expect("two calls ran, so a trace is due");
    let lines: Vec<&str> = trace.lines().collect();
    assert_eq!(lines.len(), 2, "{trace}");
    assert!(lines[0].starts_with(LIST_SERVICES), "{}", lines[0]);
    assert!(lines[1].starts_with(SERVICE_STATUS), "{}", lines[1]);
    assert!(lines[1].contains("degraded"), "{}", lines[1]);
}

#[tokio::test]
async fn an_answer_with_no_tool_calls_emits_no_trace_artifact() {
    // An empty trace artifact would tell a caller "tools ran and found
    // nothing", which is a different claim from "no tool ran".
    let model = ScriptedModel::new([says("Paris.")]);

    let (result, events) = drive(&executor(model, false), &ctx("capital of France?")).await;

    assert!(result.is_ok(), "{result:?}");
    assert_eq!(artifact(&events, "rig-response").as_deref(), Some("Paris."));
    assert!(artifact(&events, "tool-trace").is_none());
}

#[tokio::test]
async fn a_model_that_only_calls_tools_hits_the_turn_limit() {
    // Unbounded, this holds the A2A task open until the server's executor
    // timeout — an hour by default — with the caller unable to tell a slow
    // agent from a stuck one.
    let model = ScriptedModel::new(
        std::iter::repeat_with(|| calls("call-n", LIST_SERVICES, json!({}))).take(MAX_TURNS),
    );
    let observer = model.clone();

    let (result, _) = drive(&executor(model, false), &ctx("loop forever")).await;

    let err = result.expect_err("the loop must give up rather than run on");
    assert_eq!(err.code, ErrorCode::InternalError);
    assert!(
        err.message.contains(&MAX_TURNS.to_string()),
        "the failure should say how many turns were spent: {}",
        err.message
    );
    assert_eq!(
        observer.request_count(),
        MAX_TURNS,
        "the loop must spend exactly its budget — no more, and no fewer"
    );
}
