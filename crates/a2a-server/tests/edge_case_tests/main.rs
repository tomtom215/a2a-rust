// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Edge-case integration tests — shared test executors and helpers.
//!
//! Submodules are organised by responsibility so each file stays focused
//! and easy to navigate.

mod concurrency;
mod error_mapping;
mod event_queue;
mod handler_basics;
mod store_and_eviction;
mod type_system;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::error::{A2aError, A2aResult, ErrorCode};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part, PartContent};
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams, TaskQueryParams};
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, Task, TaskId, TaskState, TaskStatus, TaskVersion};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::store::InMemoryTaskStore;
use a2a_protocol_server::streaming::{EventQueueReader, EventQueueWriter};
use a2a_protocol_server::{ServerError, TaskStoreConfig};

// ── Test executors ───────────────────────────────────────────────────────────

struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
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
                    status: TaskStatus::with_timestamp(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::with_timestamp(TaskState::Completed),
                    metadata: None,
                }))
                .await?;
            Ok(())
        })
    }
}

struct FailingExecutor;

impl AgentExecutor for FailingExecutor {
    fn execute<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move { Err(A2aError::internal("executor exploded")) })
    }
}

struct SlowExecutor;

impl AgentExecutor for SlowExecutor {
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
                    status: TaskStatus::with_timestamp(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Check for cancellation
            tokio::select! {
                _ = ctx.cancellation_token.cancelled() => {
                    Err(A2aError::internal("task was cancelled"))
                }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    Ok(())
                }
            }
        })
    }
}

// ── Helper ───────────────────────────────────────────────────────────────────

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        context_id: None,
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
