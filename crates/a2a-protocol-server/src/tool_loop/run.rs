// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolLoop`]: model turns and tool calls until the model answers.

use std::collections::HashMap;
use std::fmt;

use a2a_protocol_types::budget::{BudgetHandle, Exhausted, TOKENS_METER};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::idempotency::{self, Secret};
pub use super::loop_error::LoopError;
use super::model::{ChatMessage, ChatRequest, ToolCall, ToolModel};
use super::recover;
use super::tool::{ToolError, ToolInvocation, ToolSet};
use crate::rpc_span::{AgentRun, GenAiMeter, GenAiSpan, Reply};

/// The turn bound when none is set.
///
/// Four more than the two rounds a discover-then-query catalogue needs
/// (`examples/rig-agent` measured six as enough on 2026-10-20), and small
/// enough that a model which never stops calling tools fails in seconds
/// rather than at the executor timeout.
pub const DEFAULT_MAX_TURNS: usize = 8;

/// What a run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Answer {
    /// The model's final text.
    pub text: String,
    /// One line per tool call, in the order they ran: `name(arguments) ->
    /// result`. Empty when the model answered without calling anything.
    pub trace: Vec<String>,
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
    tools: ToolSet,
    preamble: String,
    max_turns: usize,
    recover_text_calls: bool,
    secret: Secret,
    agent_name: Option<String>,
    meter: GenAiMeter,
}

impl fmt::Debug for ToolLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolLoop")
            .field("model", &self.model.model_name())
            .field("tools", &self.tools)
            .field("max_turns", &self.max_turns)
            .field("recover_text_calls", &self.recover_text_calls)
            .field("agent_name", &self.agent_name)
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
            recover_text_calls: true,
            secret: idempotency::fresh_secret(),
            agent_name: None,
            meter: GenAiMeter::default(),
        }
    }

    /// Names the agent this loop is, recorded as `gen_ai.agent.name` on
    /// the run's metrics.
    #[must_use]
    pub fn with_agent_name(mut self, name: impl Into<String>) -> Self {
        self.agent_name = Some(name.into());
        self
    }

    /// Records the loop's metrics (`gen_ai.client.inference.duration` and
    /// the rest; ADR 0013) to `meter` rather than to the global provider's
    /// meter, which is looked up each time one is recorded.
    #[cfg(feature = "otel")]
    #[must_use]
    pub fn with_meter(mut self, meter: opentelemetry::metrics::Meter) -> Self {
        self.meter = GenAiMeter::new(meter);
        self
    }

    /// Sets the secret idempotency keys are derived under. Each loop draws
    /// its own by default, so keys differ between processes; replicas that
    /// set the same secret give the same call in the same scope the same
    /// key, so a backend deduplicates a call one replica retries after
    /// another made it. Keep it as secret as a credential: with it, keys
    /// can be computed from task ids and arguments.
    #[must_use]
    pub const fn with_idempotency_secret(mut self, secret: [u8; 32]) -> Self {
        self.secret = secret;
        self
    }

    /// Sets the system preamble every turn is sent with.
    #[must_use]
    pub fn with_preamble(mut self, preamble: impl Into<String>) -> Self {
        self.preamble = preamble.into();
        self
    }

    /// Sets how many model turns a run may take. `0` is taken as `1`: a run
    /// asks the model at least once.
    #[must_use]
    pub const fn with_max_turns(mut self, max_turns: usize) -> Self {
        self.max_turns = if max_turns == 0 { 1 } else { max_turns };
        self
    }

    /// Whether a final reply that is exactly one JSON object naming a tool
    /// in the set — bare, in a code fence, or between `<tool_call>` tags —
    /// is run as that call rather than returned as the answer. On by
    /// default: small models write calls as text, and an answer of raw JSON
    /// helps nobody.
    #[must_use]
    pub const fn with_recover_text_calls(mut self, on: bool) -> Self {
        self.recover_text_calls = on;
        self
    }

    /// The turn bound.
    #[must_use]
    pub const fn max_turns(&self) -> usize {
        self.max_turns
    }

    /// Sends `user_text` and runs turns until the model answers without a
    /// call. `cancel` is checked before each turn and each call, and a turn
    /// or call in progress is abandoned when it fires.
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
    /// usage (input plus output) is charged to its [`TOKENS_METER`], the run
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
        self.run_drafting(scope, user_text, cancel, budget, None)
            .await
    }

    /// [`run_scoped`](Self::run_scoped), each turn's text sent to `drafts`
    /// as it arrives, with the turn's number, when given (audit L5).
    pub(crate) async fn run_drafting(
        &self,
        scope: &str,
        user_text: &str,
        cancel: &CancellationToken,
        budget: Option<&BudgetHandle>,
        drafts: Option<&tokio::sync::mpsc::UnboundedSender<(usize, String)>>,
    ) -> Result<Answer, LoopError> {
        let run = AgentRun::start(
            &self.meter,
            self.agent_name.as_deref(),
            self.model.model_name(),
        );
        let result = self
            .run_counted(&run, scope, user_text, cancel, budget, drafts)
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
        drafts: Option<&tokio::sync::mpsc::UnboundedSender<(usize, String)>>,
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
            out_of_time(budget)?;
            run.inference_call();
            let sink = drafts.map(|tx| super::model::TextSink::new(turn + 1, tx.clone()));
            let request = ChatRequest::new(&self.preamble, &messages, &specs);
            let reply = self.turn(request, cancel, sink.as_ref()).await?;
            charge_usage(budget, &reply)?;

            let mut text = reply.text;
            let mut calls = reply.tool_calls;
            if calls.is_empty() {
                let recovered = if self.recover_text_calls {
                    recover::text_call(&text, &names)
                } else {
                    None
                };
                let Some((name, arguments)) = recovered else {
                    return Ok(Answer { text, trace });
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
            for (i, call) in calls.iter_mut().enumerate() {
                if call.id.is_empty() {
                    call.id = format!("call-{turn}-{i}");
                }
            }
            messages.push(ChatMessage::Assistant {
                text,
                tool_calls: calls.clone(),
            });

            for call in calls {
                if cancel.is_cancelled() {
                    return Err(LoopError::Cancelled);
                }
                let content = self.call(run, &call, cancel, scope, &mut done).await?;
                trace.push(format!("{}({}) -> {content}", call.name, call.arguments));
                messages.push(ChatMessage::ToolResult {
                    call_id: call.id,
                    name: call.name,
                    content,
                });
            }
        }
        Err(LoopError::TurnLimit(self.max_turns))
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
    ) -> Result<String, LoopError> {
        let tool = self.tools.get(&call.name);
        let arguments = parse_arguments(&call.arguments);
        let key = match (&tool, &arguments) {
            (Some(_), Ok(arguments)) => {
                idempotency::call_key(&self.secret, scope, &call.name, arguments)
            }
            _ => String::new(),
        };
        if let Some(earlier) = done.get(&key) {
            return Ok(format!(
                "not run again: an identical call already ran in this task and returned: {earlier}"
            ));
        }
        run.tool_call();
        let span = self.tool_span(call);
        let Some(tool) = tool else {
            span.failed("unknown_tool");
            return Ok(format!(
                "error: no tool named '{}'; the tools are: {}",
                call.name,
                self.tools.names().join(", ")
            ));
        };
        let arguments = match arguments {
            Ok(arguments) => arguments,
            Err(e) => {
                span.failed("invalid_arguments");
                return Ok(format!("error: {e}"));
            }
        };
        let invocation = ToolInvocation::new(arguments, key.clone());
        let result = tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(LoopError::Cancelled),
            result = span.instrument(tool.call(invocation)) => result,
        };
        match result {
            Ok(output) => {
                span.succeeded();
                if !tool.read_only() {
                    done.insert(key, output.clone());
                }
                Ok(output)
            }
            Err(ToolError::Failed(e)) => {
                span.failed("tool_error");
                Ok(format!("error: {e}"))
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

/// [`LoopError::Budget`] once `budget`'s deadline has passed or its
/// [`TOKENS_METER`] has nothing left: a turn would only overdraw it.
fn out_of_time(budget: Option<&BudgetHandle>) -> Result<(), LoopError> {
    let Some(budget) = budget else { return Ok(()) };
    if budget.remaining_time() == Some(std::time::Duration::ZERO) {
        return Err(LoopError::Budget(Exhausted::Time));
    }
    if budget.remaining(TOKENS_METER) == Some(0) {
        return Err(LoopError::Budget(Exhausted::Meter {
            name: TOKENS_METER.to_owned(),
            overdraft: 0,
        }));
    }
    Ok(())
}

/// Charges `reply`'s reported usage, input plus output, to `budget`'s
/// [`TOKENS_METER`]; [`LoopError::Budget`] once that is spent.
fn charge_usage(
    budget: Option<&BudgetHandle>,
    reply: &super::model::ModelTurn,
) -> Result<(), LoopError> {
    if let (Some(budget), Some(usage)) = (budget, reply.usage) {
        let used = usage
            .input_tokens
            .unwrap_or(0)
            .saturating_add(usage.output_tokens.unwrap_or(0));
        budget
            .charge(TOKENS_METER, used)
            .map_err(LoopError::Budget)?;
    }
    Ok(())
}

/// A call's arguments as the object a [`Tool`](super::Tool) is given. Empty
/// text is taken as no arguments, which some providers send for a tool that
/// takes none.
fn parse_arguments(raw: &str) -> Result<Value, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    match serde_json::from_str::<Value>(raw) {
        Ok(object @ Value::Object(_)) => Ok(object),
        Ok(_) => Err("the arguments must be a JSON object".to_owned()),
        Err(e) => Err(format!("the arguments are not valid JSON ({e})")),
    }
}

#[cfg(test)]
#[path = "run_tests/mod.rs"]
mod tests;
