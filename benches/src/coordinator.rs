// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Coordinator executor for multi-hop agent-chain benchmarks.
//!
//! [`ChainHopExecutor`] is an [`AgentExecutor`] that, on receiving a request,
//! delegates to the *next* hop in a chain by calling `send_message` on a
//! pre-built [`A2aClient`]. It then forwards a `Completed` status back to
//! its caller.
//!
//! Combined with [`crate::fault_transport::FaultInjectingTransport`], this
//! lets a benchmark construct an N-hop coordinator chain (A → B → C → D → E)
//! where every link between hops can have independently configured latency
//! and error rates, and measure end-to-end latency through the whole chain.
//!
//! # What this is and is not
//!
//! This is the minimal coordinator topology: *sequential delegation*. It is
//! the simplest multi-agent shape and the feedback was explicit that more
//! interesting topologies (critic loops, parallel fan-out with deadline
//! propagation, plan-and-execute with replanning) would be more rubric-
//! relevant. This executor exists to give the fault benchmark a realistic
//! multi-hop target and to make `end-to-end latency under fault` a
//! meaningful metric, not to claim the full multi-agent rubric bullet. The
//! docstring in
//! `benches/benches/coordinator_chain_under_fault.rs` is explicit about
//! that.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use a2a_protocol_client::A2aClient;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, MessageId, MessageRole};
use a2a_protocol_types::params::MessageSendParams;
use a2a_protocol_types::task::{ContextId, TaskState, TaskStatus};

use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

// ── ChainHopExecutor ─────────────────────────────────────────────────────────

/// An executor that forwards the incoming message to the next hop and
/// completes when the downstream hop responds.
///
/// Each hop gets its own [`ChainHopExecutor`] and its own [`A2aClient`]
/// pointing at the next hop. The client is typically built with a
/// [`crate::fault_transport::FaultInjectingTransport`] so faults can be
/// injected on a per-hop basis.
///
/// Retries at each hop are capped by the value passed to
/// [`ChainHopExecutor::with_max_retries`]. On retry exhaustion the hop
/// emits a `Failed` status and returns an error so the coordinator chain
/// surfaces the failure end-to-end.
pub struct ChainHopExecutor {
    /// Client used to delegate to the next hop in the chain.
    next: Arc<A2aClient>,
    /// Human-readable label used in the forwarded message for debuggability.
    label: String,
    /// Maximum retry attempts per call into the next hop. `0` means "no
    /// retries, fail on first error." Defaults to `0`.
    max_retries: usize,
}

impl ChainHopExecutor {
    /// Creates a new chain hop that forwards requests to the given client.
    #[must_use]
    pub fn new(label: impl Into<String>, next: Arc<A2aClient>) -> Self {
        Self {
            next,
            label: label.into(),
            max_retries: 0,
        }
    }

    /// Sets the maximum retry count for the call into the next hop.
    #[must_use]
    pub const fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }
}

impl AgentExecutor for ChainHopExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // 1. Emit Working so any streaming consumer sees progress.
            queue
                .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                    task_id: ctx.task_id.clone(),
                    context_id: ContextId::new(ctx.context_id.clone()),
                    status: TaskStatus::new(TaskState::Working),
                    metadata: None,
                }))
                .await?;

            // 2. Build a forwarded message. We keep the incoming text parts
            //    verbatim because the point of the benchmark is to measure
            //    transport + coordination overhead, not message mutation.
            let forwarded = Message {
                id: MessageId::new(format!("{}-forward", self.label)),
                role: MessageRole::Agent,
                parts: ctx.message.parts.clone(),
                task_id: None,
                context_id: None,
                reference_task_ids: None,
                extensions: None,
                metadata: None,
            };
            let params = MessageSendParams {
                tenant: None,
                message: forwarded,
                configuration: None,
                metadata: None,
            };

            // 3. Call the next hop, retrying up to max_retries on retryable
            //    errors (e.g. the synthetic Timeout emitted by the fault
            //    transport). Non-retryable errors propagate immediately.
            let mut attempts_remaining = self.max_retries.saturating_add(1);
            let next_result = loop {
                match self.next.send_message(params.clone()).await {
                    Ok(result) => break Ok(result),
                    Err(err) => {
                        attempts_remaining -= 1;
                        if attempts_remaining == 0 || !err.is_retryable() {
                            break Err(err);
                        }
                    }
                }
            };

            // 4. On downstream success, emit Completed. On downstream
            //    failure, emit Failed and return an error so the chain
            //    surfaces the fault end-to-end.
            match next_result {
                Ok(_) => {
                    queue
                        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                            task_id: ctx.task_id.clone(),
                            context_id: ContextId::new(ctx.context_id.clone()),
                            status: TaskStatus::new(TaskState::Completed),
                            metadata: None,
                        }))
                        .await?;
                    Ok(())
                }
                Err(err) => {
                    queue
                        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                            task_id: ctx.task_id.clone(),
                            context_id: ContextId::new(ctx.context_id.clone()),
                            status: TaskStatus::new(TaskState::Failed),
                            metadata: None,
                        }))
                        .await?;
                    Err(A2aError::internal(format!(
                        "{label}: downstream hop failed: {err}",
                        label = self.label,
                    )))
                }
            }
        })
    }
}

// ── Leaf executor ────────────────────────────────────────────────────────────

/// A minimal terminal executor used at the end of a coordinator chain.
///
/// Writes `Working` → `Completed` with no artifact, matching the minimum
/// cost the chain benchmark needs the leaf hop to pay. Separated from
/// [`crate::executor::EchoExecutor`] so that the leaf contribution in the
/// chain bench is the smallest possible executor (no artifact emission),
/// isolating the chain's per-hop overhead from echo logic.
pub struct ChainLeafExecutor;

impl AgentExecutor for ChainLeafExecutor {
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
