// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoopExecutor`]: a [`ToolLoop`] as an A2A task.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::approval::ApprovalRequest;
use a2a_protocol_types::artifact::Artifact;
use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::events::{StreamResponse, TaskArtifactUpdateEvent, TaskStatusUpdateEvent};
use a2a_protocol_types::message::{Message, Part};
use a2a_protocol_types::task::{TaskState, TaskStatus};

use super::answer::{Answer, FINISH_REASON_LENGTH, FINISH_REASON_METADATA_KEY};
use super::idempotency;
use super::run::{LoopError, ToolLoop};
use super::steering::Side;
use super::trace::Sink;
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
/// extension (`a2a_protocol_types::failure`): `transient` when the model
/// provider may come back (HTTP 429, 502, 503 or 504, a timeout, no
/// connection), `internal` for any other provider failure or a tool that
/// returned [`ToolError::Fatal`], `budget-exhausted` when the model was
/// still calling tools at the turn bound, the caller's budget
/// (`RequestContext::budget`, ADR 0023) ran out (its token usage charged to
/// the `tokens` meter), or the model hit its output limit before writing
/// any text, and `policy-refusal` when the provider's content filter
/// withheld the answer. A final reply with no text is such a run
/// ([`LoopError::EmptyAnswer`]). The status message says which, in words.
/// Cancellation ends the run at the next turn or call, with nothing
/// written: the cancel path writes `Canceled`.
///
/// A reply that has text but stopped at the output limit completes the task:
/// the answer artifact's metadata and the `Completed` status message both
/// carry [`FINISH_REASON_METADATA_KEY`], so a caller can tell a cut-off
/// answer from one that ended by itself.
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
    pub(super) tool_loop: ToolLoop,
    drafts: bool,
    steering: bool,
}

impl ToolLoopExecutor {
    /// An executor running `tool_loop` for each message.
    #[must_use]
    pub const fn new(tool_loop: ToolLoop) -> Self {
        Self {
            tool_loop,
            drafts: false,
            steering: false,
        }
    }

    /// Reads the turn's steering inbox
    /// ([`RequestContext::inbox`](crate::RequestContext::inbox)) before each
    /// model turn, and adds each message's text to the conversation as a
    /// user turn, so a caller can correct course while the run works (ADR
    /// 0036). Off by default. Needs a handler built with
    /// [`with_steering`](crate::RequestHandlerBuilder::with_steering); without
    /// one there is no inbox and this does nothing.
    ///
    /// The text is the sender's: it is user text like the prompt, with the
    /// sender's authority over the task and no more. It cannot cancel the run
    /// (`CancelTask` does), approve an action, or lift a budget. A message
    /// that arrives during the turn that answers is not read: the run is
    /// over, and the message is counted as unread.
    #[must_use]
    pub const fn with_steering(mut self) -> Self {
        self.steering = true;
        self
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
        trace: &Sink,
    ) -> A2aResult<Result<super::Answer, LoopError>> {
        // A decision on a call the run is parked on finds that run by the id of the
        // request, which is the run's journal key; any other message is new work,
        // keyed by the task and the message it answers.
        let approval = ctx.approval().filter(|_| self.tool_loop.has_journal());
        let scope = approval.map_or_else(
            || idempotency::task_scope(&ctx.task_id.0, &ctx.message.id.0),
            |a| a.request.request_id.clone(),
        );
        let verdict = approval.map(|a| super::journaling::Verdict {
            digest: a.request.digest.clone(),
            approved: a.is_approved(),
            by: a.approver.clone(),
        });
        let cancel = &ctx.cancellation_token;
        let inbox = if self.steering { ctx.inbox() } else { None };
        // From the authenticated request, never from what the model wrote.
        let identity = super::policy::TrustedIdentity {
            tenant: ctx.tenant().map(str::to_owned),
            principal: ctx.caller_identity().map(str::to_owned),
            task: Some(ctx.task_id.0.clone()),
        };
        if !self.drafts {
            let side = Side {
                drafts: None,
                inbox,
                trace: Some(trace),
                identity: Some(&identity),
                approval: verdict.as_ref(),
            };
            return Ok(self
                .tool_loop
                .run_drafting(&scope, prompt, cancel, ctx.budget(), side)
                .await);
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let side = Side {
            drafts: Some(&tx),
            inbox,
            trace: Some(trace),
            identity: Some(&identity),
            approval: verdict.as_ref(),
        };
        let run = self
            .tool_loop
            .run_drafting(&scope, prompt, cancel, ctx.budget(), side);
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
            if prompt.trim().is_empty() && ctx.approval().is_none() {
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

            let trace = Sink::default();
            match self.run(ctx, &emit, &prompt, &trace).await? {
                Ok(answer) => complete(ctx, queue, &emit, answer).await,
                Err(LoopError::Cancelled) => Ok(()),
                Err(LoopError::AwaitingApproval(needed)) => {
                    // The calls made so far have run; the caller sees them, and
                    // then the question. The decision for this digest, from an
                    // approver the gate admits, is what resumes the run.
                    send_trace(&emit, &trace).await?;
                    let question = format!(
                        "Approval needed before calling '{}': {}",
                        needed.tool, needed.summary
                    );
                    let request =
                        ApprovalRequest::new(needed.request_id, needed.summary, needed.digest);
                    emit.request_approval(request, question).await.map(|_| ())
                }
                Err(e @ (LoopError::TurnLimit(_) | LoopError::Budget(_))) => {
                    send_trace(&emit, &trace).await?;
                    emit.fail(e.failure_class(), e.to_string()).await
                }
                Err(e) => {
                    // The whole error is the operator's; the caller gets
                    // `caller_message`, without the provider's body or URL.
                    trace_warn!(error = %e, "the run failed");
                    send_trace(&emit, &trace).await?;
                    emit.fail(e.failure_class(), e.caller_message()).await
                }
            }
        })
    }
}

/// What the status message of a task that completed with a cut-off answer
/// says, for a person reading it; the machine-readable form is
/// [`FINISH_REASON_METADATA_KEY`].
const TRUNCATED_NOTE: &str =
    "The answer was cut off at the model's output limit and may be incomplete.";

/// Writes the answer artifact, the trace artifact when a tool ran, and
/// `Completed`. A reply cut off at the output limit
/// ([`Answer::truncated`]) is still a completed task, since the text is real
/// and may be enough, but the answer artifact and the status message both
/// carry [`FINISH_REASON_METADATA_KEY`], so that a caller can tell it from
/// an answer that ended on its own (audit M-1).
async fn complete(
    ctx: &RequestContext,
    queue: &dyn EventQueueWriter,
    emit: &EventEmitter<'_>,
    answer: Answer,
) -> A2aResult<()> {
    let marker = answer
        .truncated()
        .then(|| serde_json::json!({ FINISH_REASON_METADATA_KEY: FINISH_REASON_LENGTH }));
    let mut artifact = Artifact::new(ANSWER_ARTIFACT_ID, vec![Part::text(answer.text)]);
    artifact.metadata.clone_from(&marker);
    let mut update =
        TaskArtifactUpdateEvent::new(ctx.task_id.clone(), ctx.context_id.clone(), artifact);
    update.last_chunk = Some(true);
    queue.write(StreamResponse::ArtifactUpdate(update)).await?;
    if !answer.trace.is_empty() {
        emit.artifact(
            TRACE_ARTIFACT_ID,
            vec![Part::text(answer.trace.join("\n"))],
            None,
            Some(true),
        )
        .await?;
    }
    let Some(marker) = marker else {
        return emit.status(TaskState::Completed).await;
    };
    let mut note = Message::agent(
        uuid::Uuid::new_v4().to_string(),
        vec![Part::text(TRUNCATED_NOTE)],
    );
    note.task_id = Some(ctx.task_id.clone());
    note.context_id = Some(ctx.context_id.clone());
    note.metadata = Some(marker);
    let mut status = TaskStatus::new(TaskState::Completed);
    status.message = Some(note);
    queue
        .write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
            ctx.task_id.clone(),
            ctx.context_id.clone(),
            status,
        )))
        .await
}

/// Sends the trace of a run that failed, before the failure: the calls it made
/// have run whatever became of the answer, and a caller that cannot see them
/// retries with new idempotency keys and runs them again. Since M-1 an empty
/// final reply after tools fails the task, where it used to complete it with
/// this trace, and a run that hit the turn bound or its budget never had it
/// (hostile campaign, wave 2, W2-LIVE-3). Nothing is sent when no call ran.
async fn send_trace(emit: &EventEmitter<'_>, trace: &Sink) -> A2aResult<()> {
    let lines = trace.take();
    if lines.is_empty() {
        return Ok(());
    }
    emit.artifact(
        TRACE_ARTIFACT_ID,
        vec![Part::text(lines.join("\n"))],
        None,
        Some(true),
    )
    .await
}
