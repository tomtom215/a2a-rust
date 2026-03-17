// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! State transition tests.
//!
//! These tests verify that the `RequestHandler` correctly enforces the task
//! state machine: rejecting invalid transitions (e.g. Working -> Submitted,
//! Completed -> Working) and accepting valid multi-step transitions
//! (Working -> InputRequired -> Working -> Completed, Working -> Canceled,
//! Working -> Failed).

use super::*;

// ── Invalid transition executors ────────────────────────────────────────────

/// Executor that emits Working then attempts Submitted (invalid transition).
struct InvalidTransitionExecutor;

impl AgentExecutor for InvalidTransitionExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Working -> Submitted is invalid.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Submitted),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// Executor that reaches Completed, then tries Working (terminal -> non-terminal).
struct TerminalTransitionExecutor;

impl AgentExecutor for TerminalTransitionExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            // Completed -> Working is invalid (terminal state).
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

// ── Valid transition executors ───────────────────────────────────────────────

/// Executor that exercises: Working -> InputRequired -> Working -> Completed.
struct MultiTransitionExecutor;

impl AgentExecutor for MultiTransitionExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            for state in [
                TaskState::Working,
                TaskState::InputRequired,
                TaskState::Working,
                TaskState::Completed,
            ] {
                queue
                    .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        status: TaskStatus::new(state),
                        metadata: None,
                    }))
                    .await?;
            }
            Ok(())
        })
    }
}

/// Executor that emits Working -> Canceled.
struct CanceledExecutor;

impl AgentExecutor for CanceledExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Canceled),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

/// Executor that emits Working -> Failed (via status update, not error).
struct FailedStatusExecutor;

impl AgentExecutor for FailedStatusExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Failed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

// ── Invalid transition tests ────────────────────────────────────────────────

#[tokio::test]
async fn sync_mode_invalid_state_transition_returns_error() {
    let handler = RequestHandlerBuilder::new(InvalidTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Err(ref err) => {
            assert!(
                matches!(
                    err,
                    a2a_protocol_server::ServerError::InvalidStateTransition { .. }
                ),
                "expected InvalidStateTransition, got {err:?}"
            );
        }
        Ok(_) => panic!("expected error for invalid state transition"),
    }
}

#[tokio::test]
async fn sync_mode_completed_to_working_is_invalid() {
    let handler = RequestHandlerBuilder::new(TerminalTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), false, None)
        .await;

    match result {
        Err(ref err) => {
            assert!(
                matches!(
                    err,
                    a2a_protocol_server::ServerError::InvalidStateTransition { .. }
                ),
                "expected InvalidStateTransition, got {err:?}"
            );
        }
        Ok(_) => panic!("expected error for terminal state transition"),
    }
}

#[tokio::test]
async fn streaming_mode_invalid_transition_does_not_crash_stream() {
    let handler = RequestHandlerBuilder::new(InvalidTransitionExecutor)
        .build()
        .expect("build handler");

    let result = handler
        .on_send_message(make_send_params(), true, None)
        .await
        .expect("send streaming");

    let mut reader = match result {
        SendMessageResult::Stream(r) => r,
        _ => panic!("expected Stream"),
    };

    let mut events = vec![];
    while let Some(event) = reader.read().await {
        events.push(event);
    }

    // The SSE reader still sees all events (the invalid transition is only
    // rejected by the background processor, not the SSE layer).
    assert!(!events.is_empty(), "stream should still produce events");
}

// ── Valid transition tests ──────────────────────────────────────────────────

#[tokio::test]
async fn sync_mode_multiple_valid_transitions() {
    let handler = RequestHandlerBuilder::new(MultiTransitionExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(task.status.state, TaskState::Completed);
}

#[tokio::test]
async fn sync_mode_working_to_canceled() {
    let handler = RequestHandlerBuilder::new(CanceledExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(task.status.state, TaskState::Canceled);
}

#[tokio::test]
async fn sync_mode_working_to_failed_via_status_update() {
    let handler = RequestHandlerBuilder::new(FailedStatusExecutor)
        .build()
        .expect("build handler");

    let task = extract_task(
        handler
            .on_send_message(make_send_params(), false, None)
            .await
            .expect("send"),
    );
    assert_eq!(task.status.state, TaskState::Failed);
}
