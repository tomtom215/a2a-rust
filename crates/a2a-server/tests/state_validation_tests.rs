// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for handler-level state transition validation.

use std::future::Future;
use std::pin::Pin;

use a2a_types::error::A2aResult;
use a2a_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_types::message::{Message, MessageId, MessageRole, Part};
use a2a_types::params::MessageSendParams;
use a2a_types::responses::SendMessageResponse;
use a2a_types::task::{TaskState, TaskStatus};

use a2a_server::builder::RequestHandlerBuilder;
use a2a_server::executor::AgentExecutor;
use a2a_server::handler::SendMessageResult;
use a2a_server::request_context::RequestContext;
use a2a_server::streaming::EventQueueWriter;

/// An executor that produces an invalid state transition:
/// Submitted -> Completed (skipping Working).
struct InvalidTransitionExecutor;

impl AgentExecutor for InvalidTransitionExecutor {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Skip Working and go directly to Completed — this is invalid.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// An executor with valid transitions: Submitted -> Working -> Completed.
struct ValidTransitionExecutor;

impl AgentExecutor for ValidTransitionExecutor {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// Executor: Working -> InputRequired -> Working -> Completed.
struct InputRequiredExecutor;

impl AgentExecutor for InputRequiredExecutor {
    fn execute<'a>(&'a self, ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::InputRequired),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ctx.context_id.clone(),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new("msg-1"),
            role: MessageRole::User,
            parts: vec![Part::text(text)],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    }
}

#[tokio::test]
async fn invalid_state_transition_is_rejected() {
    let handler = RequestHandlerBuilder::new(InvalidTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await;

    // Should fail because Submitted -> Completed is invalid.
    let err = result.err().expect("expected error for invalid transition");
    assert!(
        matches!(err, a2a_server::ServerError::InvalidStateTransition { .. }),
        "expected InvalidStateTransition, got {err:?}"
    );
}

#[tokio::test]
async fn valid_state_transitions_succeed() {
    let handler = RequestHandlerBuilder::new(ValidTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await
        .expect("valid transitions should succeed");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

#[tokio::test]
async fn input_required_roundtrip_transitions_succeed() {
    let handler = RequestHandlerBuilder::new(InputRequiredExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await
        .expect("input required roundtrip should succeed");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task)"),
    }
}

// ── Executor timeout tests ──────────────────────────────────────────────────

/// An executor that sleeps forever, simulating a hung executor.
struct HungExecutor;

impl AgentExecutor for HungExecutor {
    fn execute<'a>(&'a self, _ctx: &'a RequestContext, _queue: &'a dyn EventQueueWriter) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Sleep indefinitely — should be killed by timeout.
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            Ok(())
        })
    }
}

#[tokio::test]
async fn executor_timeout_produces_failed_task() {
    let handler = RequestHandlerBuilder::new(HungExecutor)
        .with_executor_timeout(std::time::Duration::from_millis(50))
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await
        .expect("timeout should produce a failed task, not a handler error");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(
                task.status.state,
                TaskState::Failed,
                "timed-out executor should produce Failed task"
            );
        }
        _ => panic!("expected Response(Task) for timeout"),
    }
}

#[tokio::test]
async fn executor_without_timeout_completes_normally() {
    let handler = RequestHandlerBuilder::new(ValidTransitionExecutor)
        .with_executor_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params("test"), false)
        .await
        .expect("normal execution with timeout should succeed");

    match result {
        SendMessageResult::Response(SendMessageResponse::Task(task)) => {
            assert_eq!(task.status.state, TaskState::Completed);
        }
        _ => panic!("expected Response(Task) for normal execution"),
    }
}
