// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tool loop, with the tools on the far side of an MCP session.
//!
//! Compare this file with `examples/rig-agent/src/agent.rs`. The loop is the
//! same — same three rules, same bound, same shape — and the diff is entirely
//! in where a tool call goes: there, a `match` on a compiled-in name; here, a
//! `tools/call` to another process. **That is the lesson.** The loop does not
//! care where tools come from, so an agent written against one source can be
//! repointed at the other without touching the part that was hard to get
//! right.
//!
//! # Three rules the loop keeps, unchanged from the in-process version
//!
//! 1. The catalogue goes on **every** request — a provider holds no state
//!    between turns.
//! 2. A tool that **ran and refused** is a result, not a failure: the reason
//!    goes back to the model, which can act on it.
//! 3. The loop is **bounded** ([`MAX_TURNS`]), or a model that calls tools
//!    forever holds the A2A task open until the server's executor timeout.
//!
//! # And one the in-process version did not need
//!
//! 4. A **broken session** is not a tool result. When the MCP server dies
//!    mid-loop, re-prompting cannot help, and answering anyway would hand the
//!    caller a guess dressed as a researched answer. The task fails, and it
//!    says why.
//!
//! Rules 2 and 4 look similar and are opposites. `crate::mcp` is where they
//! are told apart.

use std::fmt;

use rig_core::completion::message::{ToolCall, ToolResultContent, UserContent};
use rig_core::completion::{AssistantContent, CompletionError, CompletionModel, Message};

use crate::mcp::{McpError, McpTools, ToolOutcome};

/// How many model turns one prompt may take before the loop gives up.
///
/// Without a bound, a model that keeps calling tools holds the A2A task open
/// until the server's executor timeout — an hour by default — with the caller
/// unable to tell a slow agent from a stuck one.
pub const MAX_TURNS: usize = 6;

/// What one prompt produced.
#[derive(Debug)]
pub struct Answer {
    /// The model's final text.
    pub text: String,
    /// One line per tool call, in the order they ran.
    pub trace: Vec<String>,
}

/// Why a prompt produced no answer.
#[derive(Debug)]
pub enum AgentError {
    /// The model provider failed, or was unreachable.
    Completion(CompletionError),
    /// The MCP session broke. Distinct from a tool refusing, which is not an
    /// error at all — see [`crate::mcp`].
    Mcp(McpError),
    /// The model asked for tools on every turn and never answered.
    TurnLimit(usize),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completion(e) => write!(f, "{e}"),
            Self::Mcp(e) => write!(f, "{e}"),
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

impl From<McpError> for AgentError {
    fn from(e: McpError) -> Self {
        Self::Mcp(e)
    }
}

/// A preamble, the model that answers under it, and an MCP session to call.
pub struct McpAgent<M> {
    model: M,
    preamble: String,
    tools: McpTools,
}

impl<M: CompletionModel + Clone> McpAgent<M> {
    /// Builds an agent over `model`, answering under `preamble`, with `tools`
    /// as its only source of tools.
    pub fn new(model: M, preamble: &str, tools: McpTools) -> Self {
        Self {
            model,
            preamble: preamble.to_owned(),
            tools,
        }
    }

    /// Sends `user_text` as the user turn and runs the tool loop until the
    /// model answers without asking for a tool.
    ///
    /// # Errors
    ///
    /// [`AgentError::Completion`] if the provider fails, [`AgentError::Mcp`]
    /// if the MCP session breaks, and [`AgentError::TurnLimit`] if the model
    /// never stops calling tools.
    pub async fn prompt(&self, user_text: &str) -> Result<Answer, AgentError> {
        let catalogue = self.tools.definitions().to_vec();
        let mut history: Vec<Message> = Vec::new();
        let mut turn = Message::user(user_text);
        let mut trace = Vec::new();

        for _ in 0..MAX_TURNS {
            let response = self
                .model
                .completion_request(turn.clone())
                .preamble(self.preamble.clone())
                .messages(history.clone())
                .tools(catalogue.clone())
                .send()
                .await?;

            let calls: Vec<ToolCall> = response
                .choice
                .iter()
                .filter_map(|content| match content {
                    AssistantContent::ToolCall(call) => Some(call.clone()),
                    _ => None,
                })
                .collect();

            if calls.is_empty() {
                return Ok(Answer {
                    text: text_of(&response.choice),
                    trace,
                });
            }

            history.push(turn);
            history.push(Message::Assistant {
                id: response.message_id.clone(),
                content: response.choice.clone(),
            });

            let mut results = Vec::with_capacity(calls.len());
            for call in &calls {
                // `?` on the transport failure is rule 4: a dead session ends
                // the task rather than becoming a line the model reads.
                let outcome = self
                    .tools
                    .invoke(&call.function.name, &call.function.arguments)
                    .await?;
                let rendered = match outcome {
                    ToolOutcome::Output(text) => text,
                    ToolOutcome::Refused(why) => format!("error: {why}"),
                };
                trace.push(format!(
                    "{}({}) -> {rendered}",
                    call.function.name, call.function.arguments
                ));
                results.push(UserContent::tool_result_for(
                    call.id.clone(),
                    call.provider.clone(),
                    call.function.name.clone(),
                    vec![ToolResultContent::text(rendered)],
                ));
            }
            turn = Message::User { content: results };
        }

        Err(AgentError::TurnLimit(MAX_TURNS))
    }
}

/// Concatenates one assistant turn's text blocks, in order.
fn text_of(choice: &[AssistantContent]) -> String {
    choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}
