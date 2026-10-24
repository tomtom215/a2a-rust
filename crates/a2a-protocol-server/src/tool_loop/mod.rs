// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tool calling: a model, the tools it may call, and the bounded loop
//! between them, run as an A2A task (ADR 0022; feature `tool-loop`).
//!
//! A2A has no tool concept. It carries messages, tasks and artifacts
//! between agents; tool calling happens inside one task, between the
//! executor and its model. This module is that layer:
//!
//! ```text
//!  A2A client ──SendMessage──→ ToolLoopExecutor           one A2A task
//!                                   │
//!                                   ▼
//!                              ToolLoop::run               many model turns
//!                            ┌──────┴───────┐
//!                    model turn N   ──→  tool calls? ──no──→ answer + trace
//!                            ▲              │ yes
//!                            └── results ── ToolSet
//! ```
//!
//! - [`ToolModel`]: one completion turn, provider-neutral.
//!   [`OpenAiChatModel`] is the bundled one, for the chat-completions API
//!   (`OpenAI`, llama.cpp's llama-server, vLLM, Ollama).
//! - [`Tool`] and [`ToolSet`]: what the model may call. Each call carries
//!   an idempotency key, and a call repeated within a run to a tool that is
//!   not read-only is answered from the first (ADR 0026).
//! - [`ToolLoop`]: turns until the model answers without a call, bounded by
//!   [`with_max_turns`](ToolLoop::with_max_turns), with a tool's error sent
//!   back to the model as that call's result, and a call written as text
//!   recovered as a call.
//! - [`ToolLoopExecutor`]: the loop as an [`AgentExecutor`](crate::AgentExecutor),
//!   with the answer and a `tool-trace` artifact.
//!
//! Each model turn and each tool call has its own span, named as the
//! OpenTelemetry `GenAI` conventions name them (`chat {model}`,
//! `execute_tool {name}`) and opened inside the executor's `invoke_agent`
//! span. They record no prompt, argument or result.
//!
//! ```rust,no_run
//! use std::future::Future;
//! use std::pin::Pin;
//! use a2a_protocol_server::tool_loop::{
//!     OpenAiChatModel, Tool, ToolError, ToolInvocation, ToolLoop, ToolLoopExecutor, ToolSet,
//! };
//! use a2a_protocol_server::RequestHandlerBuilder;
//!
//! struct Clock;
//! impl Tool for Clock {
//!     fn name(&self) -> &str { "utc_now" }
//!     fn description(&self) -> &str { "The current UTC time, RFC 3339." }
//!     fn parameters(&self) -> serde_json::Value {
//!         serde_json::json!({"type": "object", "properties": {}})
//!     }
//!     fn read_only(&self) -> bool { true }
//!     fn call<'a>(
//!         &'a self,
//!         _call: ToolInvocation,
//!     ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
//!         Box::pin(async { Ok("2026-10-20T12:00:00Z".to_owned()) })
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
//! let tools = ToolSet::new().with(Clock)?;
//! let executor = ToolLoopExecutor::new(
//!     ToolLoop::new(model, tools).with_preamble("Call the tools yourself to answer."),
//! );
//! let handler = RequestHandlerBuilder::new(executor).without_agent_card().build()?;
//! # drop(handler); Ok(()) }
//! ```

mod executor;
mod idempotency;
mod loop_error;
mod model;
mod openai;
mod recover;
mod run;
mod tool;

pub use executor::{ANSWER_ARTIFACT_ID, TRACE_ARTIFACT_ID, ToolLoopExecutor};
pub use model::{
    ChatMessage, ChatRequest, ModelError, ModelTurn, TokenUsage, ToolCall, ToolModel, ToolSpec,
};
pub use openai::{DEFAULT_MAX_RESPONSE_BYTES, DEFAULT_TIMEOUT, ERROR_BODY_LIMIT, OpenAiChatModel};
pub use run::{Answer, DEFAULT_MAX_TURNS, LoopError, ToolLoop};
pub use tool::{DuplicateTool, Tool, ToolError, ToolInvocation, ToolSet};
