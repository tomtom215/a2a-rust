// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Trivial executor implementations for benchmarks.
//!
//! These executors do the absolute minimum work so that benchmarks isolate
//! SDK transport/protocol overhead from any "agent logic" latency.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::Part;
use a2a_protocol_types::push::TaskPushNotificationConfig;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::push::PushSender;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

// ── EchoExecutor ────────────────────────────────────────────────────────────

/// A minimal executor that echoes a fixed text part back.
///
/// Writes 3 events: Working status → ArtifactUpdate → Completed status.
/// Each event write involves a queue send (~400-800ns for event allocation +
/// broadcast channel send), so this executor is *not* zero-overhead — it
/// performs real work that contributes measurably to benchmark results.
///
/// For pure transport overhead measurement, prefer [`NoopExecutor`] which
/// writes only 2 events (Working → Completed).
pub struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
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

            // Artifact
            queue
                .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    artifact: Artifact::new("echo", vec![Part::text("Echo: benchmark")]),
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

// ── NoopExecutor ────────────────────────────────────────────────────────────

/// A near-minimal executor that writes 2 status events (Working → Completed).
///
/// The minimum cost of any conforming executor is at least 1 queue write
/// (~400-800ns for event allocation + broadcast channel send). This executor
/// writes 2 events, making it the lowest-overhead executor that produces a
/// valid Working → Completed state transition.
///
/// Useful for measuring transport overhead with minimal executor contribution.
pub struct NoopExecutor;

impl AgentExecutor for NoopExecutor {
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
            Ok(())
        })
    }
}

// ── MultiEventExecutor ──────────────────────────────────────────────────────

/// An executor that emits N status/artifact events before completing.
///
/// Useful for streaming throughput and backpressure benchmarks where the
/// number of events matters more than the content.
pub struct MultiEventExecutor {
    /// Number of intermediate (Working + Artifact) event pairs to emit.
    pub event_pairs: usize,
}

impl AgentExecutor for MultiEventExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // Emit a single Working status, then N artifact events, then Completed.
            // The state machine only allows Working → Completed (not Working → Working),
            // so we emit Working once and use artifact events for the streaming volume.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // Emit N artifact events (the streaming payload).
            for i in 0..self.event_pairs {
                queue
                    .write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                        task_id: ctx.task_id.clone(),
                        context_id: ContextId::new(ctx.context_id.clone()),
                        artifact: Artifact::new(
                            format!("chunk-{i}"),
                            vec![Part::text(format!("Streaming chunk {i}"))],
                        ),
                        append: Some(true),
                        last_chunk: Some(i == self.event_pairs - 1),
                        metadata: None,
                    }))
                    .await?;
            }

            // Final completed status
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

// ── FailingExecutor ─────────────────────────────────────────────────────────

/// An executor that always returns an error after emitting a Working status.
///
/// Useful for benchmarking error path overhead.
pub struct FailingExecutor;

impl AgentExecutor for FailingExecutor {
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
                    status: TaskStatus::new(TaskState::Failed),
                    metadata: None,
                }))
                .await?;

            Err(a2a_protocol_types::error::A2aError::internal(
                "Benchmark: simulated executor failure",
            ))
        })
    }
}

// ── NoopPushSender ─────────────────────────────────────────────────────────

/// A no-op push sender for benchmarks that need push notification support
/// enabled without performing actual webhook delivery.
pub struct NoopPushSender;

impl PushSender for NoopPushSender {
    fn send<'a>(
        &'a self,
        _url: &'a str,
        _event: &'a StreamResponse,
        _config: &'a TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn allows_private_urls(&self) -> bool {
        true
    }
}
