// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Shared helpers used across the agent team example.
//!
//! ## Dogfood finding: event emission boilerplate
//!
//! Every executor must repeat `task_id`, `context_id` in every status/artifact
//! event. The [`EventEmitter`] helper reduces this to one-liners, showing
//! what the SDK *should* provide or what users will inevitably build.

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

/// Creates a simple [`MessageSendParams`] with a single text part.
pub fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
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

/// Reduces event emission boilerplate by caching task_id and context_id.
///
/// **Dogfood note**: This helper demonstrates a pattern the SDK should
/// provide natively. Without it, every executor repeats:
/// ```rust,ignore
/// queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
///     task_id: ctx.task_id.clone(),
///     context_id: ContextId::new(ctx.context_id.clone()),
///     status: TaskStatus::new(TaskState::Working),
///     metadata: None,
/// })).await?;
/// ```
///
/// With EventEmitter:
/// ```rust,ignore
/// emit.status(TaskState::Working).await?;
/// ```
pub struct EventEmitter<'a> {
    pub ctx: &'a RequestContext,
    pub queue: &'a dyn EventQueueWriter,
}

impl<'a> EventEmitter<'a> {
    pub fn new(ctx: &'a RequestContext, queue: &'a dyn EventQueueWriter) -> Self {
        Self { ctx, queue }
    }

    /// Emit a status update event.
    pub async fn status(&self, state: TaskState) -> A2aResult<()> {
        self.queue
            .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: self.ctx.task_id.clone(),
                context_id: ContextId::new(self.ctx.context_id.clone()),
                status: TaskStatus::new(state),
                metadata: None,
            }))
            .await
    }

    /// Emit an artifact update event.
    pub async fn artifact(
        &self,
        id: &str,
        parts: Vec<Part>,
        append: Option<bool>,
        last_chunk: Option<bool>,
    ) -> A2aResult<()> {
        self.queue
            .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: self.ctx.task_id.clone(),
                context_id: ContextId::new(self.ctx.context_id.clone()),
                artifact: Artifact::new(id, parts),
                append,
                last_chunk,
                metadata: None,
            }))
            .await
    }

    /// Check if the task has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.ctx.cancellation_token.is_cancelled()
    }
}
