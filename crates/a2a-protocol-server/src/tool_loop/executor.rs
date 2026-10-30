// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoopExecutor`]: a [`ToolLoop`] as an A2A task.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::failure::FailureClass;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::TaskState;

use super::idempotency;
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
/// Each run's call keys are scoped to the task and the message it answers
/// (ADR 0026): another message to the same task is new work, with new keys.
///
/// [`with_drafts`](Self::with_drafts) also streams each turn's text as it
/// arrives, to an artifact of its own.
///
/// [`ToolError::Fatal`]: super::ToolError::Fatal
#[derive(Debug)]
pub struct ToolLoopExecutor {
    tool_loop: ToolLoop,
    drafts: bool,
}

impl ToolLoopExecutor {
    /// An executor running `tool_loop` for each message.
    #[must_use]
    pub const fn new(tool_loop: ToolLoop) -> Self {
        Self {
            tool_loop,
            drafts: false,
        }
    }

    /// Streams each turn's text, as the model sends it, to a draft artifact
    /// of the turn's own, `draft-1`, `draft-2` and so on
    /// ([`draft_artifact_id`](super::draft_artifact_id)), each closed with
    /// `lastChunk` when the next turn starts or the run ends; the answer
    /// artifact is sent at the end, as without (audit L5). A turn's text is
    /// known to be the answer only when the turn ends, which is why it is
    /// not streamed into the answer itself. Needs a model that streams
    /// ([`ToolModel::complete_streaming`](super::ToolModel::complete_streaming);
    /// [`OpenAiChatModel`](super::OpenAiChatModel) does); with one that
    /// does not, no draft is sent. Off by default, since the drafts are
    /// artifacts a client reading every artifact would now see.
    #[must_use]
    pub const fn with_drafts(mut self) -> Self {
        self.drafts = true;
        self
    }

    /// The run, its turns' text streamed to drafts when [`Self::drafts`].
    async fn run(
        &self,
        ctx: &RequestContext,
        emit: &EventEmitter<'_>,
        prompt: &str,
    ) -> A2aResult<Result<super::Answer, LoopError>> {
        let scope = idempotency::task_scope(&ctx.task_id.0, &ctx.message.id.0);
        let cancel = &ctx.cancellation_token;
        if !self.drafts {
            return Ok(self
                .tool_loop
                .run_scoped(&scope, prompt, cancel, ctx.budget())
                .await);
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let run = self
            .tool_loop
            .run_drafting(&scope, prompt, cancel, ctx.budget(), Some(&tx));
        tokio::pin!(run);
        let mut drafts = super::drafts::Drafts::default();
        let result = loop {
            tokio::select! {
                biased;
                Some((turn, piece)) = rx.recv() => drafts.push(emit, turn, piece).await?,
                result = &mut run => break result,
            }
        };
        // Cancelled: the cancel path writes `Canceled`, and nothing after.
        if matches!(result, Err(LoopError::Cancelled)) {
            return Ok(result);
        }
        while let Ok((turn, piece)) = rx.try_recv() {
            drafts.push(emit, turn, piece).await?;
        }
        drafts.close(emit).await?;
        Ok(result)
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

            match self.run(ctx, &emit, &prompt).await? {
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
                Err(e) => {
                    // The whole error is the operator's; the caller gets
                    // `caller_message`, without the provider's body or URL.
                    trace_warn!(error = %e, "the run failed");
                    let class = if e.is_transient() {
                        FailureClass::Transient
                    } else {
                        FailureClass::Internal
                    };
                    emit.fail(class, e.caller_message()).await
                }
            }
        })
    }
}
