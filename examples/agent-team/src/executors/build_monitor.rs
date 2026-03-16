// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Build Monitor executor — simulates cargo builds with streaming output.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

/// Simulates running cargo commands and streaming build output. Supports
/// cancellation to test the cancel flow.
pub struct BuildMonitorExecutor;

impl AgentExecutor for BuildMonitorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Working
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Parse command from message.
            let command = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "check".to_owned());

            // Simulate build phases with streaming artifacts.
            let phases = match command.as_str() {
                "fail" => vec![
                    ("Compiling...", false),
                    ("error[E0308]: mismatched types", false),
                    ("Build FAILED", true),
                ],
                "slow" => vec![
                    ("Compiling phase 1/5...", false),
                    ("Compiling phase 2/5...", false),
                    ("Compiling phase 3/5...", false),
                    ("Compiling phase 4/5...", false),
                    ("Compiling phase 5/5...", false),
                    ("Build OK", true),
                ],
                "very-slow" => vec![
                    ("Compiling phase 1/10...", false),
                    ("Compiling phase 2/10...", false),
                    ("Compiling phase 3/10...", false),
                    ("Compiling phase 4/10...", false),
                    ("Compiling phase 5/10...", false),
                    ("Compiling phase 6/10...", false),
                    ("Compiling phase 7/10...", false),
                    ("Compiling phase 8/10...", false),
                    ("Compiling phase 9/10...", false),
                    ("Compiling phase 10/10...", false),
                    ("Build OK", true),
                ],
                _ => vec![
                    ("Compiling dependencies...", false),
                    ("Compiling project...", false),
                    ("Build OK: 0 errors, 0 warnings", true),
                ],
            };

            let should_fail = command == "fail";

            for (i, (phase_msg, is_last)) in phases.iter().enumerate() {
                // Check cancellation before each phase.
                if ctx.cancellation_token.is_cancelled() {
                    queue
                        .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                            task_id: ctx.task_id.clone(),
                            context_id: ContextId::new(ctx.context_id.clone()),
                            artifact: Artifact::new(
                                "build-output",
                                vec![Part::text("Build CANCELED by user")],
                            ),
                            append: Some(i > 0),
                            last_chunk: Some(true),
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
                    return Ok(());
                }

                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new("build-output", vec![Part::text(*phase_msg)]),
                        append: Some(i > 0),
                        last_chunk: Some(*is_last),
                        metadata: None,
                    }))
                    .await?;

                // Simulate work.
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            }

            // Terminal state.
            let final_state = if should_fail {
                TaskState::Failed
            } else {
                TaskState::Completed
            };

            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(final_state),
                    metadata: None,
                }))
                .await?;

            Ok(())
        })
    }

    fn cancel<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // BuildMonitor supports cancellation — write a Canceled status.
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
