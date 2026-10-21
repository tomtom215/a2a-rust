// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoopExecutor`]: a [`ToolLoop`] as an A2A task.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::failure::FailureClass;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

use super::run::{LoopError, ToolLoop};
use crate::executor::AgentExecutor;
use crate::executor_helpers::EventEmitter;
use crate::request_context::RequestContext;
use crate::streaming::EventQueueWriter;

/// The id of the artifact carrying the answer.
pub const ANSWER_ARTIFACT_ID: &str = "answer";

/// The id of the artifact carrying the tool trace.
///
/// One line per call and its result, emitted when at least one tool ran. It
/// is what lets a caller tell an answer the tools backed from one the model
/// made up.
pub const TRACE_ARTIFACT_ID: &str = "tool-trace";

/// An [`AgentExecutor`] that answers each message by running a [`ToolLoop`].
///
/// The loop is wholly inside one A2A task: the caller sees `Working`, the
/// answer artifact ([`ANSWER_ARTIFACT_ID`]), the trace artifact
/// ([`TRACE_ARTIFACT_ID`]) when a tool ran, and `Completed`. The prompt is
/// the message's text parts, joined by newlines; a message with none is
/// refused as an invalid request.
///
/// A run that ends without an answer fails the task with the failure-class
/// extension (`a2a_protocol_types::failure`): `internal` when the model
/// provider failed or a tool returned [`ToolError::Fatal`], and
/// `budget-exhausted` when the model was still calling tools at the turn
/// bound or the caller's budget (`RequestContext::budget`, ADR 0023) ran
/// out, its token usage charged to the `tokens` meter. The status message
/// says which, in words. Cancellation ends the run
/// at the next turn or call, with nothing written: the cancel path writes
/// `Canceled`.
///
/// [`ToolError::Fatal`]: super::ToolError::Fatal
#[derive(Debug)]
pub struct ToolLoopExecutor {
    tool_loop: ToolLoop,
}

impl ToolLoopExecutor {
    /// An executor running `tool_loop` for each message.
    #[must_use]
    pub const fn new(tool_loop: ToolLoop) -> Self {
        Self { tool_loop }
    }
}

impl AgentExecutor for ToolLoopExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let prompt = ctx.message.texts().collect::<Vec<_>>().join("\n");
            if prompt.trim().is_empty() {
                return Err(A2aError::invalid_params(
                    "the message has no text to answer",
                ));
            }
            let cancel = &ctx.cancellation_token;
            if cancel.is_cancelled() {
                return Ok(());
            }
            let emit = EventEmitter::new(ctx, queue);
            emit.status(TaskState::Working).await?;

            match self
                .tool_loop
                .run_within(&prompt, cancel, ctx.budget())
                .await
            {
                Ok(answer) => {
                    emit.artifact(
                        ANSWER_ARTIFACT_ID,
                        vec![Part::text(answer.text)],
                        None,
                        Some(true),
                    )
                    .await?;
                    if !answer.trace.is_empty() {
                        emit.artifact(
                            TRACE_ARTIFACT_ID,
                            vec![Part::text(answer.trace.join("\n"))],
                            None,
                            Some(true),
                        )
                        .await?;
                    }
                    emit.status(TaskState::Completed).await
                }
                Err(LoopError::Cancelled) => Ok(()),
                Err(e @ (LoopError::TurnLimit(_) | LoopError::Budget(_))) => {
                    emit.fail(FailureClass::BudgetExhausted, e.to_string())
                        .await
                }
                Err(e) => emit.fail(FailureClass::Internal, e.to_string()).await,
            }
        })
    }
}
