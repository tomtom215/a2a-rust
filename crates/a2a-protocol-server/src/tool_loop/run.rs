// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoop`]: model turns and tool calls until the model answers.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::Arc;

use a2a_protocol_types::budget::BudgetHandle;
use tokio_util::sync::CancellationToken;

use super::answer::Answer;
use super::idempotency::{self, Secret};
use super::journal::{ReceiptState, StepJournal};
use super::journaling::{RunJournal, Verdict};
pub use super::loop_error::LoopError;
use super::model::{ChatMessage, ChatRequest, ToolCall, ToolModel};
use super::policy::{CallStatus, TextCallRecovery, ToolPolicy, TraceContent, TrustedIdentity};
use super::recover;
use super::retry::ModelRetry;
use super::run_support::{charge_usage, give_ids, limit_calls};
use super::schema::ArgumentCheck;
use super::steering::{Side, before_turn};
use super::tool::ToolSet;
use super::trace;
use crate::rpc_span::{AgentRun, GenAiMeter, GenAiSpan, Reply};

/// The turn bound when none is set.
///
/// Four more than the two rounds a discover-then-query catalogue needs
/// (`examples/rig-agent` measured six as enough on 2026-10-20), and small
/// enough that a model which never stops calling tools fails in seconds
/// rather than at the executor timeout.
pub const DEFAULT_MAX_TURNS: usize = 8;

/// The tool calls one model reply may make when none is set.
///
/// The first this many run and the rest are dropped. A model that writes a
/// hundred calls in one reply is broken or hostile, and with no bound a reply
/// of 90,000 of them took a loop from 21 MB to 1.4 GB (hostile campaign, wave
/// 2, W2-LIVE-5).
pub const DEFAULT_MAX_CALLS_PER_TURN: usize = 64;

/// Where a call came from, for the policy, and what a person has decided.
#[derive(Clone, Copy)]
struct Origin<'a> {
    from_text: bool,
    identity: Option<&'a TrustedIdentity>,
    /// The decision on the call the run was parked on, when it is resumed.
    approval: Option<&'a Verdict>,
    /// Whether a call that needs approval has a journal to wait in.
    can_park: bool,
}

/// A run's conversation, trace and record of answered calls, which a journal
/// checkpoints.
pub(super) struct Progress {
    pub(super) messages: Vec<ChatMessage>,
    pub(super) trace: Vec<String>,
    pub(super) done: HashMap<String, String>,
}

/// What one model reply is.
enum Turn {
    /// The answer, and why the model stopped.
    Answer(String, Option<String>),
    /// Calls to run, and whether the model wrote them as text.
    Calls(Vec<ToolCall>, bool),
}

/// A model, the tools it may call, and the bounded loop between them.
///
/// Each turn sends the preamble, the whole conversation and the whole
/// catalogue; a reply with calls has them run and their results appended,
/// and a reply without is the answer. What a call returns goes back to the
/// model, errors included: an unknown tool, arguments that are not a JSON
/// object and [`ToolError::Failed`](super::ToolError::Failed) are each that call's result, because
/// that is how a model recovers. Only the provider failing, a
/// [`ToolError::Fatal`](super::ToolError::Fatal), the turn bound and cancellation end a run.
///
/// Each call carries an idempotency key ([`ToolInvocation::idempotency_key`](super::ToolInvocation::idempotency_key),
/// ADR 0026), and a call identical to one that already succeeded in the run
/// is answered with the earlier result instead of being run again, unless
/// its tool is [`read_only`](super::Tool::read_only).
pub struct ToolLoop {
    pub(super) model: Box<dyn ToolModel>,
    pub(super) tools: ToolSet,
    pub(super) preamble: String,
    pub(super) max_turns: usize,
    pub(super) max_calls_per_turn: usize,
    pub(super) text_recovery: TextCallRecovery,
    pub(super) trace_content: TraceContent,
    pub(super) argument_check: ArgumentCheck,
    pub(super) journal: Option<Arc<dyn StepJournal>>,
    /// The deployment's own pins, checked on resume.
    pub(super) pin_labels: BTreeMap<String, String>,
    /// Whether the idempotency secret was set, so keys agree across replicas.
    pub(super) secret_set: bool,
    policy: Option<Box<dyn ToolPolicy>>,
    secret: Secret,
    agent_name: Option<String>,
    meter: GenAiMeter,
    model_retry: Option<ModelRetry>,
}

impl fmt::Debug for ToolLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolLoop")
            .field("model", &self.model.model_name())
            .field("tools", &self.tools)
            .field("max_turns", &self.max_turns)
            .field("max_calls_per_turn", &self.max_calls_per_turn)
            .field("text_recovery", &self.text_recovery)
            .field("trace_content", &self.trace_content)
            .field("argument_check", &self.argument_check)
            .field("journal", &self.journal.is_some())
            .field("policy", &self.policy.is_some())
            .field("agent_name", &self.agent_name)
            .field("model_retry", &self.model_retry)
            .finish_non_exhaustive()
    }
}

impl ToolLoop {
    /// A loop over `model` and `tools`, with no preamble,
    /// [`DEFAULT_MAX_TURNS`] and text-call recovery on.
    #[must_use]
    pub fn new(model: impl ToolModel, tools: ToolSet) -> Self {
        Self {
            model: Box::new(model),
            tools,
            preamble: String::new(),
            max_turns: DEFAULT_MAX_TURNS,
            max_calls_per_turn: DEFAULT_MAX_CALLS_PER_TURN,
            text_recovery: TextCallRecovery::All,
            trace_content: TraceContent::Full,
            argument_check: ArgumentCheck::Off,
            journal: None,
            pin_labels: BTreeMap::new(),
            secret_set: false,
            policy: None,
            secret: idempotency::fresh_secret(),
            agent_name: None,
            meter: GenAiMeter::default(),
            model_retry: None,
        }
    }

    /// The turn bound.
    #[must_use]
    pub const fn max_turns(&self) -> usize {
        self.max_turns
    }

    /// The bound on the tool calls of one reply.
    #[must_use]
    pub const fn max_calls_per_turn(&self) -> usize {
        self.max_calls_per_turn
    }

    /// Sends `user_text` and runs turns until the model answers without a
    /// call. `cancel` is checked before each turn and each call, and a turn
    /// or call in progress is abandoned when it fires. An answer is a final
    /// reply with text: an empty or blank one is
    /// [`LoopError::EmptyAnswer`], and one cut off at the model's output
    /// limit is an [`Answer`] whose [`truncated`](Answer::truncated) is true.
    ///
    /// # Errors
    ///
    /// A [`LoopError`] naming why there is no answer.
    pub async fn run(
        &self,
        user_text: &str,
        cancel: &CancellationToken,
    ) -> Result<Answer, LoopError> {
        self.run_within(user_text, cancel, None).await
    }

    /// [`run`](Self::run), held to `budget` when there is one: no turn
    /// starts once its deadline has passed, and each reply's reported token
    /// usage (input plus output) is charged to its [`TOKENS_METER`](a2a_protocol_types::budget::TOKENS_METER), the run
    /// ending once that is spent. A provider that reports no usage charges
    /// nothing.
    ///
    /// # Errors
    ///
    /// As [`run`](Self::run), and [`LoopError::Budget`] when the budget runs
    /// out.
    pub async fn run_within(
        &self,
        user_text: &str,
        cancel: &CancellationToken,
        budget: Option<&BudgetHandle>,
    ) -> Result<Answer, LoopError> {
        self.run_scoped(&idempotency::fresh_scope(), user_text, cancel, budget)
            .await
    }

    /// [`run_within`](Self::run_within), with the calls' idempotency keys
    /// derived in `scope`. [`run`](Self::run) and `run_within` draw a fresh
    /// scope for each run; [`ToolLoopExecutor`](super::ToolLoopExecutor)
    /// uses the task and message ids, so that running the same message of
    /// the same task again gives each call the key it had before. Give
    /// different work different scopes: the same call in the same scope has
    /// the same key.
    ///
    /// With a [journal](Self::with_journal) the scope is also the journal's
    /// key: running the same scope again resumes the run it left, and two
    /// pieces of work that share a scope share a checkpoint.
    ///
    /// # Errors
    ///
    /// As [`run_within`](Self::run_within).
    pub async fn run_scoped(
        &self,
        scope: &str,
        user_text: &str,
        cancel: &CancellationToken,
        budget: Option<&BudgetHandle>,
    ) -> Result<Answer, LoopError> {
        self.run_drafting(scope, user_text, cancel, budget, Side::default())
            .await
    }

    /// [`run_scoped`](Self::run_scoped), each turn's text sent to `drafts`
    /// as it arrives, with the turn's number, when given (audit L5).
    pub(super) async fn run_drafting(
        &self,
        scope: &str,
        user_text: &str,
        cancel: &CancellationToken,
        budget: Option<&BudgetHandle>,
        side: Side<'_>,
    ) -> Result<Answer, LoopError> {
        let run = AgentRun::start(
            &self.meter,
            self.agent_name.as_deref(),
            self.model.model_name(),
        );
        let result = self
            .run_counted(&run, scope, user_text, cancel, budget, side)
            .await;
        run.ended(result.as_ref().err().map(LoopError::error_type).as_deref());
        result
    }

    /// [`run_scoped`](Self::run_scoped), counting turns and calls in `run`.
    async fn run_counted(
        &self,
        run: &AgentRun,
        scope: &str,
        user_text: &str,
        cancel: &CancellationToken,
        budget: Option<&BudgetHandle>,
        side: Side<'_>,
    ) -> Result<Answer, LoopError> {
        let specs = self.tools.specs();
        let names = self.tools.names();
        let (mut journal, restored) =
            RunJournal::open(self, scope, user_text, &specs, side.approval, side.identity).await?;
        let (mut progress, first_turn, mut resume, identity) = restored.begin(side.trace);
        let identity = identity.as_ref().or(side.identity);

        for turn in first_turn..self.max_turns {
            if cancel.is_cancelled() {
                return Err(LoopError::Cancelled);
            }
            let (calls, from_text) = if let Some(r) = resume.take() {
                (r.calls, r.recovered)
            } else {
                let reply = self
                    .ask_model(run, turn, cancel, budget, &side, &specs, &mut progress)
                    .await?;
                match self.turn_of(reply, turn, &names, &side, &mut progress) {
                    Turn::Answer(text, finish_reason) => {
                        let answer = Answer::from_reply(text, progress.trace, finish_reason)?;
                        journal.clear().await;
                        return Ok(answer);
                    }
                    Turn::Calls(calls, from_text) => {
                        journal.save((turn, from_text), &progress).await?;
                        (calls, from_text)
                    }
                }
            };
            let origin = Origin {
                from_text,
                identity,
                approval: side.approval,
                can_park: journal.enabled(),
            };
            let step = (turn, from_text);
            self.run_calls(
                run,
                calls,
                step,
                (cancel, scope, origin),
                &side,
                &mut progress,
                &mut journal,
            )
            .await?;
        }
        Err(LoopError::TurnLimit(self.max_turns))
    }

    /// One model turn: what the caller said since the last is added to the
    /// conversation, the model is asked, and its usage charged.
    #[allow(clippy::too_many_arguments)]
    async fn ask_model(
        &self,
        run: &AgentRun,
        turn: usize,
        cancel: &CancellationToken,
        budget: Option<&BudgetHandle>,
        side: &Side<'_>,
        specs: &[super::model::ToolSpec],
        progress: &mut Progress,
    ) -> Result<super::model::ModelTurn, LoopError> {
        before_turn(budget, side.inbox, &mut progress.messages)?;
        run.inference_call();
        let sink = side.sink(turn + 1);
        let request = ChatRequest::new(&self.preamble, &progress.messages, specs);
        let reply = self
            .retried_turn(request, cancel, sink.as_ref(), budget)
            .await?;
        charge_usage(budget, &reply)?;
        Ok(reply)
    }

    /// Runs `calls`, one after another, each recorded before it runs and after
    /// it ends; a call a person has to approve parks the run.
    #[allow(clippy::too_many_arguments)]
    async fn run_calls(
        &self,
        run: &AgentRun,
        calls: Vec<ToolCall>,
        step: (usize, bool),
        (cancel, scope, origin): (&CancellationToken, &str, Origin<'_>),
        side: &Side<'_>,
        progress: &mut Progress,
        journal: &mut RunJournal<'_>,
    ) -> Result<(), LoopError> {
        for call in calls {
            if cancel.is_cancelled() {
                return Err(LoopError::Cancelled);
            }
            // Written before the call runs: a call the journal cannot record is
            // not run.
            if journal.enabled() {
                journal.started(&call, super::journaling::arguments_digest(&call));
                journal.save(step, progress).await?;
            }
            match self
                .call(run, &call, cancel, scope, &mut progress.done, origin)
                .await
            {
                Ok((content, status)) => {
                    let line = self.trace_line(&call, &content, status);
                    trace::record(&mut progress.trace, side.trace, line);
                    progress.messages.push(ChatMessage::ToolResult {
                        call_id: call.id.clone(),
                        name: call.name.clone(),
                        content,
                    });
                    let state = match status {
                        CallStatus::Ran => ReceiptState::Ran,
                        CallStatus::Failed => ReceiptState::Failed,
                        CallStatus::NotRun => ReceiptState::NotRun,
                    };
                    journal.ended(&call.id, state);
                    journal.save(step, progress).await?;
                }
                Err(LoopError::AwaitingApproval(needed)) => {
                    journal.park(&call, &needed, origin.identity);
                    journal.save(step, progress).await?;
                    return Err(LoopError::AwaitingApproval(needed));
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// What a reply is: the calls it asks for, with the assistant turn pushed
    /// to `messages`, or the answer.
    fn turn_of(
        &self,
        reply: super::model::ModelTurn,
        turn: usize,
        names: &[String],
        side: &Side<'_>,
        progress: &mut Progress,
    ) -> Turn {
        let mut text = reply.text;
        let mut calls = reply.tool_calls;
        let mut from_text = false;
        limit_calls(
            &mut calls,
            self.max_calls_per_turn,
            &mut progress.trace,
            side.trace,
        );
        if calls.is_empty() {
            let recovered = self.recovered(&text, names);
            from_text = recovered.is_some();
            let Some((name, arguments)) = recovered else {
                return Turn::Answer(text, reply.finish_reason);
            };
            // The model wrote the call as text: run it as the call it is, and
            // keep it in the conversation as one.
            calls.push(ToolCall::new(
                format!("recovered-{turn}"),
                name,
                arguments.to_string(),
            ));
            text.clear();
        }
        give_ids(&mut calls, turn);
        progress.messages.push(ChatMessage::Assistant {
            text,
            tool_calls: calls.clone(),
        });
        Turn::Calls(calls, from_text)
    }

    /// The call a reply written as text is, if it is one the loop's
    /// [`TextCallRecovery`] runs.
    fn recovered(&self, text: &str, names: &[String]) -> Option<(String, serde_json::Value)> {
        recover::text_call(text, names).filter(|(name, _)| {
            let read_only = self.tools.get(name).is_some_and(|t| t.read_only());
            self.text_recovery.permits(read_only)
        })
    }

    /// The line `call` leaves in the caller's trace, as [`TraceContent`] says.
    fn trace_line(&self, call: &ToolCall, content: &str, status: CallStatus) -> String {
        match self.trace_content {
            TraceContent::Full => trace::line(&call.name, &call.arguments, content),
            TraceContent::Receipt => status.receipt(&call.name, &call.id),
        }
    }

    /// One model turn, in its span, abandoned if `cancel` fires.
    async fn turn(
        &self,
        request: ChatRequest<'_>,
        cancel: &CancellationToken,
        sink: Option<&super::model::TextSink>,
    ) -> Result<super::model::ModelTurn, LoopError> {
        let span = GenAiSpan::chat(
            self.model.provider_name(),
            self.model.model_name(),
            self.model.server(),
            &self.meter,
        );
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(LoopError::Cancelled),
            result = span.instrument(sink.map_or_else(
                || self.model.complete(request),
                |sink| self.model.complete_streaming(request, sink),
            )) => result,
        };
        match result {
            Ok(reply) => {
                let usage = reply.usage.unwrap_or_default();
                span.answered(Reply {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    response_model: reply.response_model.as_deref(),
                    response_id: reply.response_id.as_deref(),
                    finish_reason: reply.finish_reason.as_deref(),
                });
                Ok(reply)
            }
            Err(e) => {
                span.failed(&e.error_type());
                trace_warn!(error = %e, "tool loop: the model provider failed a turn");
                Err(LoopError::Model(e))
            }
        }
    }
}

#[path = "run_call.rs"]
mod call;
#[path = "run_config.rs"]
mod config;
#[path = "run_retried.rs"]
mod retried;

#[cfg(test)]
#[path = "run_tests/mod.rs"]
mod tests;
