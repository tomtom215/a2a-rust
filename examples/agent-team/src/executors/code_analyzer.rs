// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Code Analyzer executor — analyzes code metrics and streams artifact updates.

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

/// Analyzes "code" sent as text parts. Counts lines, words, characters, and
/// streams incremental artifact updates (exercising append mode).
pub struct CodeAnalyzerExecutor;

impl AgentExecutor for CodeAnalyzerExecutor {
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

            // Extract all text parts.
            let code: String = ctx
                .message
                .parts
                .iter()
                .filter_map(|p| match &p.content {
                    PartContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            let lines = code.lines().count();
            let words = code.split_whitespace().count();
            let chars = code.chars().count();

            // Check for cancellation between steps (exercises cancellation token).
            if ctx.cancellation_token.is_cancelled() {
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

            // Stream artifact 1: line count (append=false, first chunk).
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new(
                        "analysis-metrics",
                        vec![Part::text(format!("Lines: {lines}"))],
                    ),
                    append: Some(false),
                    last_chunk: Some(false),
                    metadata: None,
                }))
                .await?;

            // Small delay to simulate work.
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            // Stream artifact 2: word/char counts (append=true, last chunk).
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new(
                        "analysis-metrics",
                        vec![Part::text(format!("\nWords: {words}\nChars: {chars}"))],
                    ),
                    append: Some(true),
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Also emit a JSON data artifact (exercises PartContent::Data).
            let data_part = Part::data(serde_json::json!({
                "lines": lines,
                "words": words,
                "chars": chars,
                "complexity": if lines > 50 { "high" } else { "low" },
            }));
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("analysis-json", vec![data_part]),
                    append: None,
                    last_chunk: Some(true),
                    metadata: None,
                }))
                .await?;

            // Completed
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Completed),
                    metadata: None,
                }))
                .await?;

            Ok(())
        })
    }
}
