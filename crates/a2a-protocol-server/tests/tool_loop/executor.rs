// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `ToolLoopExecutor` served by a `RequestHandler`: the events a caller
//! sees, and how each way a run can end reaches the task.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::handler::{RequestHandler, SendMessageResult};
use a2a_protocol_server::streaming::{EventQueueReader, InMemoryQueueReader};
use a2a_protocol_server::tool_loop::{
    ANSWER_ARTIFACT_ID, ChatRequest, ModelError, ModelTurn, TRACE_ARTIFACT_ID, ToolLoop,
    ToolLoopExecutor, ToolModel, ToolSet,
};
use a2a_protocol_types::events::StreamResponse;
use a2a_protocol_types::failure::FailureClass;
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::params::{CancelTaskParams, MessageSendParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{Task, TaskState};

use super::{Scripted, Status, asks, says};

fn handler(tool_loop: ToolLoop) -> Arc<RequestHandler> {
    Arc::new(
        RequestHandlerBuilder::new(ToolLoopExecutor::new(tool_loop))
            .without_agent_card()
            .build()
            .unwrap(),
    )
}

fn scripted(replies: Vec<Result<ModelTurn, ModelError>>) -> ToolLoop {
    ToolLoop::new(Scripted::new(replies), ToolSet::new().with(Status).unwrap())
}

fn params(text: &str) -> MessageSendParams {
    MessageSendParams::new(Message::user_text("m1", text))
}

/// Each event of a streamed run, as a short label.
async fn streamed(handler: &RequestHandler) -> Vec<String> {
    let Ok(SendMessageResult::Stream(mut stream)) = handler
        .on_send_message(params("how is checkout?"), true, None)
        .await
    else {
        panic!("expected a stream");
    };
    let mut labels = Vec::new();
    while let Some(item) = stream.read().await {
        labels.push(match item.expect("no stream error").event {
            StreamResponse::Task(t) => format!("task {}", t.status.state),
            StreamResponse::StatusUpdate(u) => format!("status {}", u.status.state),
            StreamResponse::ArtifactUpdate(a) => {
                let text: Vec<&str> = a
                    .artifact
                    .parts
                    .iter()
                    .filter_map(Part::text_content)
                    .collect();
                format!("artifact {} {}", a.artifact.id.0, text.join(""))
            }
            other => format!("{other:?}"),
        });
    }
    labels
}

async fn blocking(handler: &RequestHandler, text: &str) -> Task {
    match handler.on_send_message(params(text), false, None).await {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => task,
        other => panic!("expected a task, got {other:?}"),
    }
}

/// The text of a failed task's status message.
fn reason(task: &Task) -> String {
    task.status
        .message
        .as_ref()
        .and_then(Message::text)
        .unwrap_or_default()
        .to_owned()
}

#[tokio::test]
async fn a_run_with_a_tool_streams_working_the_answer_the_trace_and_completed() {
    let handler = handler(scripted(vec![
        asks("service_status", r#"{"service":"checkout"}"#),
        says("checkout is healthy"),
    ]));
    let events = streamed(&handler).await;
    assert_eq!(
        events,
        [
            format!("task {}", TaskState::Submitted),
            format!("status {}", TaskState::Working),
            format!("artifact {ANSWER_ARTIFACT_ID} checkout is healthy"),
            format!(
                "artifact {TRACE_ARTIFACT_ID} service_status({{\"service\":\"checkout\"}}) -> {{\"state\":\"healthy\"}}"
            ),
            format!("status {}", TaskState::Completed),
        ]
    );
}

#[tokio::test]
async fn a_run_without_a_tool_has_no_trace_artifact() {
    let task = blocking(&handler(scripted(vec![says("hello")])), "hi").await;
    assert_eq!(task.status.state, TaskState::Completed);
    let artifacts = task.artifacts.unwrap_or_default();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id.0, ANSWER_ARTIFACT_ID);
    assert_eq!(artifacts[0].parts[0].text_content(), Some("hello"));
}

#[tokio::test]
async fn a_tool_error_the_model_recovers_from_still_completes() {
    let task = blocking(
        &handler(scripted(vec![
            asks("service_status", r#"{"service":"billing"}"#),
            says("there is no billing service"),
        ])),
        "how is billing?",
    )
    .await;
    assert_eq!(task.status.state, TaskState::Completed);
    let artifacts = task.artifacts.unwrap_or_default();
    let trace = artifacts
        .iter()
        .find(|a| a.id.0 == TRACE_ARTIFACT_ID)
        .and_then(|a| a.parts[0].text_content())
        .expect("a trace");
    assert!(
        trace.ends_with(r#"-> error: no service named Some("billing")"#),
        "{trace}"
    );
}

#[tokio::test]
async fn a_provider_failure_fails_the_task_as_internal_with_the_status() {
    let task = blocking(
        &handler(scripted(vec![Err(ModelError::Status {
            status: 500,
            body: "Failed to parse tool call arguments as JSON".to_owned(),
        })])),
        "go",
    )
    .await;
    assert_eq!(task.status.state, TaskState::Failed);
    assert_eq!(task.failure_class(), Some(FailureClass::Internal));
    let reason = reason(&task);
    assert!(reason.contains("HTTP 500"), "{reason}");
    assert!(reason.contains("Failed to parse tool call"), "{reason}");
}

#[tokio::test]
async fn a_fatal_tool_error_fails_the_task_as_internal_naming_the_tool() {
    let task = blocking(
        &handler(scripted(vec![
            asks("service_status", r#"{"service":"outage"}"#),
            says("never"),
        ])),
        "go",
    )
    .await;
    assert_eq!(task.status.state, TaskState::Failed);
    assert_eq!(task.failure_class(), Some(FailureClass::Internal));
    assert_eq!(
        reason(&task),
        "tool 'service_status' failed: the inventory is down"
    );
    assert!(task.artifacts.unwrap_or_default().is_empty());
}

#[tokio::test]
async fn the_turn_bound_fails_the_task_as_budget_exhausted() {
    let tool_loop =
        scripted(vec![asks("service_status", r#"{"service":"checkout"}"#)]).with_max_turns(2);
    let task = blocking(&handler(tool_loop), "go").await;
    assert_eq!(task.status.state, TaskState::Failed);
    assert_eq!(task.failure_class(), Some(FailureClass::BudgetExhausted));
    assert!(reason(&task).contains("all 2 turns"), "{}", reason(&task));
}

#[tokio::test]
async fn a_message_without_text_is_refused_as_an_invalid_request() {
    let handler = handler(scripted(vec![says("unused")]));
    let message = Message::user("m1", vec![Part::url("https://example.com/f.pdf")]);
    let task = match handler
        .on_send_message(MessageSendParams::new(message), false, None)
        .await
    {
        Ok(SendMessageResult::Response(SendMessageResponse::Task(task))) => task,
        other => panic!("expected a task, got {other:?}"),
    };
    assert_eq!(task.status.state, TaskState::Failed);
    assert_eq!(task.failure_class(), Some(FailureClass::InvalidRequest));
}

/// A turn that never finishes, and says when its future is dropped.
struct Hanging(Arc<AtomicBool>);

struct SetOnDrop(Arc<AtomicBool>);
impl Drop for SetOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl ToolModel for Hanging {
    fn model_name(&self) -> &'static str {
        "hanging"
    }
    fn provider_name(&self) -> &'static str {
        "test"
    }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let guard = SetOnDrop(Arc::clone(&self.0));
        Box::pin(async move {
            let _guard = guard;
            std::future::pending().await
        })
    }
}

/// Cancelling the task abandons the model turn in progress, and the task
/// ends `Canceled`, not `Failed`.
#[tokio::test]
async fn cancelling_the_task_abandons_the_turn_and_ends_canceled() {
    let dropped = Arc::new(AtomicBool::new(false));
    let handler = handler(ToolLoop::new(Hanging(Arc::clone(&dropped)), ToolSet::new()));
    let Ok(SendMessageResult::Stream(mut stream)) =
        handler.on_send_message(params("go"), true, None).await
    else {
        panic!("expected a stream");
    };
    let mut task_id = None;
    loop {
        let event = stream
            .read()
            .await
            .expect("an event")
            .expect("no error")
            .event;
        match &event {
            StreamResponse::Task(t) => task_id = Some(t.id.clone()),
            StreamResponse::StatusUpdate(u) if u.status.state == TaskState::Working => break,
            _ => {}
        }
    }
    let task = handler
        .on_cancel_task(
            CancelTaskParams {
                tenant: None,
                id: task_id.expect("the task comes first").0,
                metadata: None,
            },
            None,
        )
        .await
        .expect("cancel");
    assert_eq!(task.status.state, TaskState::Canceled);
    assert_eq!(last_state(stream).await, Some(TaskState::Canceled));
    tokio::time::timeout(Duration::from_secs(10), async {
        while !dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the turn in progress was dropped");
}

async fn last_state(mut stream: InMemoryQueueReader) -> Option<TaskState> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut last = None;
        while let Some(item) = stream.read().await {
            last = match item.expect("no stream error").event {
                StreamResponse::Task(t) => Some(t.status.state),
                StreamResponse::StatusUpdate(u) => Some(u.status.state),
                _ => last,
            };
        }
        last
    })
    .await
    .expect("the stream ends")
}

/// The protocol invariants any executor must keep, graded by the crate's
/// own harness: terminal states, legal transitions, cancellation on entry
/// and mid-run.
#[cfg(feature = "conformance")]
#[tokio::test]
async fn the_executor_passes_the_conformance_harness() {
    let executor = ToolLoopExecutor::new(scripted(vec![
        asks("service_status", r#"{"service":"checkout"}"#),
        says("checkout is healthy"),
    ]));
    a2a_protocol_server::conformance::check(Arc::new(executor))
        .await
        .assert_pass();
}
