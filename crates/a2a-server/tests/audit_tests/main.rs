// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Audit-driven tests covering P2 coverage gaps.
//!
//! This module contains shared test executors and helpers used by all
//! audit test submodules.

mod edge_cases;
mod lifecycle;
mod public_api;
mod security_boundaries;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult, ErrorCode};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole, Part};
use a2a_protocol_types::params::{ListTasksParams, MessageSendParams};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::responses::SendMessageResponse;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

use a2a_protocol_server::builder::RequestHandlerBuilder;
use a2a_protocol_server::call_context::CallContext;
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::handler::SendMessageResult;
use a2a_protocol_server::push::{InMemoryPushConfigStore, PushConfigStore};
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::{EventQueueManager, EventQueueReader, EventQueueWriter};

// ── Test executor implementations ────────────────────────────────────────────

/// An executor that immediately completes with a status update.
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
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("art-1", vec![Part::text("echo response")]),
                    append: None,
                    last_chunk: None,
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

            Ok(())
        })
    }
}

/// An executor that fails during execution.
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

/// An executor that waits on its cancellation token (for shutdown tests).
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
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;
            // Wait for cancellation instead of a fixed sleep, so shutdown
            // tests complete quickly.
            ctx.cancellation_token.cancelled().await;
            Ok(())
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_message(text: &str) -> Message {
    Message {
        id: MessageId::new("msg-1"),
        role: MessageRole::User,
        parts: vec![Part::text(text)],
        task_id: None,
        context_id: None,
        reference_task_ids: None,
        extensions: None,
        metadata: None,
    }
}

fn make_send_params(text: &str) -> MessageSendParams {
    MessageSendParams {
        tenant: None,
        context_id: None,
        message: make_message(text),
        configuration: None,
        metadata: None,
    }
}

/// Extracts the error from a `Result<SendMessageResult, ServerError>`,
/// panicking if it was `Ok`.
fn unwrap_send_err(
    result: Result<SendMessageResult, a2a_protocol_server::ServerError>,
) -> a2a_protocol_server::ServerError {
    match result {
        Err(e) => e,
        Ok(_) => panic!("expected error, got Ok(SendMessageResult)"),
    }
}

fn minimal_agent_card() -> AgentCard {
    AgentCard {
        url: None,
        name: "Test Agent".into(),
        description: "A test agent".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: "https://agent.example.com/rpc".into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "echo".into(),
            name: "Echo".into(),
            description: "Echoes input".into(),
            tags: vec!["echo".into()],
            examples: None,
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none(),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}
