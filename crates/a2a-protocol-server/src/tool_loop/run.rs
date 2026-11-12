// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoop`]: model turns and tool calls until the model answers.

use std::collections::HashMap;
use std::fmt;

use a2a_protocol_types::budget::BudgetHandle;
use tokio_util::sync::CancellationToken;

use super::answer::Answer;
use super::idempotency::{self, Secret};
pub use super::loop_error::LoopError;
use super::model::{ChatMessage, ChatRequest, ToolCall, ToolModel};
use super::policy::{
    CallStatus, PolicyDecision, PolicyRequest, TextCallRecovery, ToolPolicy, TraceContent,
    TrustedIdentity,
};
use super::recover;
use super::retry::ModelRetry;
use super::run_support::{charge_usage, give_ids, limit_calls, parse_arguments};
use super::steering::{Side, before_turn};
use super::tool::{Tool, ToolError, ToolInvocation, ToolSet};
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

/// Where a call came from, for the policy.
#[derive(Clone, Copy)]
struct Origin<'a> {
    from_text: bool,
    identity: Option<&'a TrustedIdentity>,
}

impl<'a> Origin<'a> {
    const fn of(from_text: bool, side: &Side<'a>) -> Self {
        Self {
            from_text,
            identity: side.identity,
        }
    }
}

/// A model, the tools it may call, and the bounded loop between them.
///
/// Each turn sends the preamble, the whole conversation and the whole
/// catalogue; a reply with calls has them run and their results appended,
/// and a reply without is the answer. What a call returns goes back to the
/// model, errors included: an unknown tool, arguments that are not a JSON
/// object and [`ToolError::Failed`] are each that call's result, because
/// that is how a model recovers. Only the provider failing, a
/// [`ToolError::Fatal`], the turn bound and cancellation end a run.
///
/// Each call carries an idempotency key ([`ToolInvocation::idempotency_key`],
/// ADR 0026), and a call identical to one that already succeeded in the run
/// is answered with the earlier result instead of being run again, unless
/// its tool is [`read_only`](super::Tool::read_only).
pub struct ToolLoop {
    model: Box<dyn ToolModel>,
    pub(super) tools: ToolSet,
    pub(super) preamble: String,
    max_turns: usize,
    max_calls_per_turn: usize,
    text_recovery: TextCallRecovery,
    trace_content: TraceContent,
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
        let mut messages = vec![ChatMessage::User(user_text.to_owned())];
        let mut trace = Vec::new();
        // What each call that succeeded returned, by key, for tools that
        // are not read-only.
        let mut done = HashMap::new();

        for turn in 0..self.max_turns {
            if cancel.is_cancelled() {
                return Err(LoopError::Cancelled);
            }
            before_turn(budget, side.inbox, &mut messages)?;
            run.inference_call();
            let sink = side.sink(turn + 1);
            let request = ChatRequest::new(&self.preamble, &messages, &specs);
            let reply = self
                .retried_turn(request, cancel, sink.as_ref(), budget)
                .await?;
            charge_usage(budget, &reply)?;

            let mut text = reply.text;
            let mut calls = reply.tool_calls;
            let mut from_text = false;
            limit_calls(&mut calls, self.max_calls_per_turn, &mut trace, side.trace);
            if calls.is_empty() {
                let recovered = self.recovered(&text, &names);
                from_text = recovered.is_some();
                let Some((name, arguments)) = recovered else {
                    return Answer::from_reply(text, trace, reply.finish_reason);
                };
                // The model wrote the call as text: run it as the call it
                // is, and keep it in the conversation as one.
                calls.push(ToolCall::new(
                    format!("recovered-{turn}"),
                    name,
                    arguments.to_string(),
                ));
                text.clear();
            }
            give_ids(&mut calls, turn);
            messages.push(ChatMessage::Assistant {
                text,
                tool_calls: calls.clone(),
            });

            for call in calls {
                if cancel.is_cancelled() {
                    return Err(LoopError::Cancelled);
                }
                let origin = Origin::of(from_text, &side);
                let (content, status) = self
                    .call(run, &call, cancel, scope, &mut done, origin)
                    .await?;
                let line = self.trace_line(&call, &content, status);
                trace::record(&mut trace, side.trace, line);
                messages.push(ChatMessage::ToolResult {
                    call_id: call.id,
                    name: call.name,
                    content,
                });
            }
        }
        Err(LoopError::TurnLimit(self.max_turns))
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

    /// Why the policy refuses `call`, if there is one and it does.
    async fn denied(
        &self,
        call: &ToolCall,
        read_only: bool,
        arguments: &serde_json::Value,
        origin: Origin<'_>,
    ) -> Option<String> {
        let policy = self.policy.as_ref()?;
        let request = PolicyRequest {
            tool: &call.name,
            arguments,
            call_id: &call.id,
            read_only,
            recovered_from_text: origin.from_text,
            tenant: origin.identity.and_then(|i| i.tenant.as_deref()),
            principal: origin.identity.and_then(|i| i.principal.as_deref()),
            task: origin.identity.and_then(|i| i.task.as_deref()),
        };
        match policy.authorize(&request).await {
            PolicyDecision::Deny(why) => Some(why),
            PolicyDecision::Allow => None,
        }
    }

    /// The span of `call`, opened as it starts to run.
    fn tool_span(&self, call: &ToolCall) -> GenAiSpan {
        let tool = self.tools.get(&call.name);
        let description = tool.map(|t| t.description());
        GenAiSpan::tool(
            &call.name,
            tool.is_some(),
            &call.id,
            description,
            &self.meter,
        )
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

    /// Runs one call in its span and returns what goes back to the model.
    /// Only [`ToolError::Fatal`] and cancellation are errors here. A call
    /// whose key is in `done` is not run: the earlier result is returned,
    /// marked, and no span is opened and nothing counted in `run`, since
    /// nothing ran.
    async fn call(
        &self,
        run: &AgentRun,
        call: &ToolCall,
        cancel: &CancellationToken,
        scope: &str,
        done: &mut HashMap<String, String>,
        origin: Origin<'_>,
    ) -> Result<(String, CallStatus), LoopError> {
        let tool = self.tools.get(&call.name);
        let arguments = parse_arguments(&call.arguments);
        let key = match (&tool, &arguments) {
            (Some(_), Ok(arguments)) => {
                idempotency::call_key(&self.secret, scope, &call.name, arguments)
            }
            _ => String::new(),
        };
        if let Some(earlier) = done.get(&key) {
            let content = format!(
                "not run again: an identical call already ran in this task and returned: {earlier}"
            );
            return Ok((content, CallStatus::NotRun));
        }
        run.tool_call();
        let span = self.tool_span(call);
        let Some(tool) = tool else {
            span.failed("unknown_tool");
            let content = format!(
                "error: no tool named '{}'; the tools are: {}",
                call.name,
                self.tools.names().join(", ")
            );
            return Ok((content, CallStatus::Failed));
        };
        let arguments = match arguments {
            Ok(arguments) => arguments,
            Err(e) => {
                span.failed("invalid_arguments");
                return Ok((format!("error: {e}"), CallStatus::Failed));
            }
        };
        if let Some(why) = self
            .denied(call, tool.read_only(), &arguments, origin)
            .await
        {
            span.failed("policy_denied");
            let content = format!("error: this call was not permitted: {why}");
            return Ok((content, CallStatus::Failed));
        }
        let invocation = ToolInvocation::new(arguments, key.clone());
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(LoopError::Cancelled),
            result = span.instrument(tool.call(invocation)) => result,
        };
        Self::settled(result, &span, (&**tool, call, key), done)
    }

    /// What a call that ran returns to the model and records.
    fn settled(
        result: Result<String, ToolError>,
        span: &GenAiSpan,
        (tool, call, key): (&dyn Tool, &ToolCall, String),
        done: &mut HashMap<String, String>,
    ) -> Result<(String, CallStatus), LoopError> {
        match result {
            Ok(output) => {
                span.succeeded();
                if !tool.read_only() {
                    done.insert(key, output.clone());
                }
                Ok((output, CallStatus::Ran))
            }
            Err(ToolError::Failed(e)) => {
                span.failed("tool_error");
                Ok((format!("error: {e}"), CallStatus::Failed))
            }
            Err(ToolError::Fatal(message)) => {
                span.failed("tool_fatal");
                Err(LoopError::Fatal {
                    tool: call.name.clone(),
                    message,
                })
            }
        }
    }
}

#[path = "run_config.rs"]
mod config;
#[path = "run_retried.rs"]
mod retried;

#[cfg(test)]
#[path = "run_tests/mod.rs"]
mod tests;
