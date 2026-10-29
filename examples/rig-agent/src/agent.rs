// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The agent this example puts behind A2A: a preamble, a model, and the tool
//! loop that runs between them.
//!
//! rig-core 0.41 moved its `Agent` run loop into the separate `rig-agent`
//! crate. This example stays on `rig-core` alone, so the loop is written out
//! here rather than delegated — and that is deliberate: the loop is the part
//! an adopter has to get right, and hiding it behind a dependency would teach
//! nothing about where it sits relative to A2A.
//!
//! # Where the loop sits
//!
//! ```text
//!  A2A client ──SendMessage──→ RigAgentExecutor          ← one A2A task
//!                                   │
//!                                   ▼
//!                              RigAgent::prompt          ← many model turns
//!                            ┌──────┴───────┐
//!                            │              │
//!                    model turn N   ──→  tool calls? ──no──→ answer
//!                            ▲              │
//!                            │             yes
//!                            │              ▼
//!                            └── tool results ── crate::tools::invoke
//! ```
//!
//! The whole loop is *inside one A2A task*. A2A never sees the tool calls;
//! it sees a task that goes `Working` and then `Completed` with an artifact.
//! That is the layering to copy — the protocol boundary is the task, not the
//! turn.
//!
//! # Three things the loop has to get right
//!
//! 1. **The catalogue goes on every request.** A provider holds no state
//!    between turns, so a follow-up request that omits the tools is one in
//!    which the model cannot call anything.
//! 2. **A tool error is a result, not a failure.** `crate::tools::invoke`
//!    errors are handed back to the model as that call's result. An unknown
//!    service is something the model recovers from by calling
//!    `list_services`; a failed A2A task is not.
//! 3. **The loop is bounded.** See [`MAX_TURNS`].

use std::fmt;

use rig_core::completion::message::{ToolCall, ToolResultContent, UserContent};
use rig_core::completion::{AssistantContent, CompletionError, CompletionModel, Message};

use crate::tools;

/// How many model turns one prompt may take before the loop gives up.
///
/// Load-bearing, not a tidiness knob. Without it a model that calls tools
/// forever holds the A2A task open until the server's executor timeout —
/// [one hour by default][timeout] — with the caller blocked and no way to
/// tell a slow agent from a stuck one. Six is four more than the two rounds
/// this example's own catalogue needs.
///
/// [timeout]: a2a_protocol_server::builder::RequestHandlerBuilder::with_executor_timeout
pub const MAX_TURNS: usize = 6;

/// What the agent is told before every prompt.
///
/// Small models otherwise tell the caller to call a tool rather than call it
/// (1 of 20 billing runs on Qwen3-1.7B, 2026-10-20), and restate a tool's
/// numbers from memory (2 of 10 checkout runs reported an uptime the tool
/// did not return).
pub const PREAMBLE: &str = "You are an operations agent with tools for checking services. \
Call the tools yourself to answer; never ask the user to call a tool. \
If a tool returns an error that suggests another tool, call that tool, then answer. \
Report values exactly as the tools returned them.";

/// What one prompt produced.
pub struct Answer {
    /// The model's final text, with no tool calls left outstanding.
    pub text: String,
    /// One line per tool call, in the order they ran. Empty when the model
    /// answered without calling anything.
    pub trace: Vec<String>,
}

/// Why a prompt produced no answer.
#[derive(Debug)]
pub enum AgentError {
    /// The provider failed, or was unreachable.
    Completion(CompletionError),
    /// The model asked for tools on every turn and never answered.
    ///
    /// Its own variant rather than a borrowed [`CompletionError`] one: the
    /// provider did nothing wrong here, and reporting this as a provider
    /// error would send whoever reads the failed task to the wrong place.
    TurnLimit(usize),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completion(e) => write!(f, "{e}"),
            Self::TurnLimit(turns) => write!(
                f,
                "the model asked for tools on all {turns} turns without answering"
            ),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<CompletionError> for AgentError {
    fn from(e: CompletionError) -> Self {
        Self::Completion(e)
    }
}

/// A preamble, the model that answers under it, and the tools it may call.
///
/// This is what `rig_core::agent::Agent` was for this example before the run
/// loop moved to the `rig-agent` crate, plus the tool loop that makes it an
/// agent rather than a completion call.
pub struct RigAgent<M> {
    model: M,
    preamble: String,
}

impl<M: CompletionModel + Clone> RigAgent<M> {
    /// Builds an agent over `model`, answering under `preamble`.
    pub fn new(model: M, preamble: &str) -> Self {
        Self {
            model,
            preamble: preamble.to_owned(),
        }
    }

    /// Sends `user_text` as the user turn and runs the tool loop until the
    /// model answers without asking for a tool.
    ///
    /// # Errors
    ///
    /// [`AgentError::Completion`] if the provider fails on any turn, and
    /// [`AgentError::TurnLimit`] if the model never stops calling tools.
    pub async fn prompt(&self, user_text: &str) -> Result<Answer, AgentError> {
        let catalogue = tools::definitions();
        let names: Vec<String> = catalogue.iter().map(|d| d.name.clone()).collect();
        // Everything before the current turn. The request builder appends
        // the prompt *after* `messages`, so `history` holds the settled
        // conversation and `turn` holds what we are asking about now.
        let mut history: Vec<Message> = Vec::new();
        let mut turn = Message::user(user_text);
        let mut trace = Vec::new();

        for round in 0..MAX_TURNS {
            let response = self
                .model
                .completion_request(turn.clone())
                .preamble(self.preamble.clone())
                .messages(history.clone())
                .tools(catalogue.clone())
                .max_tokens(max_tokens())
                .additional_params_opt(additional_params())
                .send()
                .await?;

            let mut choice = response.choice.clone();
            let mut calls: Vec<ToolCall> = choice
                .iter()
                .filter_map(|content| match content {
                    AssistantContent::ToolCall(call) => Some(call.clone()),
                    _ => None,
                })
                .collect();

            if calls.is_empty() {
                let text = text_of(&response.choice);
                let Some(function) = crate::leaked::leaked_call(&text, &names) else {
                    return Ok(Answer { text, trace });
                };
                // The model wrote the call as text: run it as the call it is,
                // and keep it in history as one (see `leaked`).
                let call = ToolCall::from_wire(format!("recovered-{round}"), function);
                choice = vec![AssistantContent::ToolCall(call.clone())];
                calls.push(call);
            }

            // The asking turn and the model's reply both settle into history;
            // the results become the next turn's prompt.
            history.push(turn);
            history.push(Message::Assistant {
                id: response.message_id.clone(),
                content: choice,
            });

            let mut results = Vec::with_capacity(calls.len());
            for call in &calls {
                let outcome = match tools::invoke(&call.function.name, &call.function.arguments) {
                    Ok(output) => output,
                    // Handed to the model, not raised. See the module docs.
                    Err(e) => format!("error: {e}"),
                };
                trace.push(format!(
                    "{}({}) -> {outcome}",
                    call.function.name, call.function.arguments
                ));
                results.push(UserContent::tool_result_for(
                    call.id.clone(),
                    call.provider.clone(),
                    call.function.name.clone(),
                    vec![ToolResultContent::text(outcome)],
                ));
            }
            turn = Message::User { content: results };
        }

        Err(AgentError::TurnLimit(MAX_TURNS))
    }
}

/// Concatenates one assistant turn's text blocks, in order.
///
/// A turn can carry reasoning and images beside its text; only the text is
/// the answer.
fn text_of(choice: &[AssistantContent]) -> String {
    choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// Bounds every model turn. A reasoning model left to think without a limit
/// on a CPU spends minutes on one turn, past any caller's timeout, and keeps
/// the model busy after the caller has gone. `RIG_MAX_TOKENS` sets the
/// cap (default 1024).
fn max_tokens() -> u64 {
    std::env::var("RIG_MAX_TOKENS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1024)
}

/// Unless `RIG_THINKING=1`, asks the server not to reason before
/// answering (`chat_template_kwargs`, which llama.cpp's `llama-server` and
/// vLLM read; a hosted provider may refuse it, so set `RIG_THINKING=1`
/// there).
fn additional_params() -> Option<serde_json::Value> {
    (std::env::var("RIG_THINKING").as_deref() != Ok("1"))
        .then(|| serde_json::json!({"chat_template_kwargs": {"enable_thinking": false}}))
}
