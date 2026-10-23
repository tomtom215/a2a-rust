// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The spans of the tool loop (`tool_loop`): one per model turn and one per
//! tool call, as the OpenTelemetry `GenAI` conventions name them —
//! `docs/gen-ai/client-inference.md` and `gen-ai-spans.md` in
//! `open-telemetry/semantic-conventions-genai` at `9ffc7e0`, all
//! `development`:
//!
//! * a model turn is an inference span: kind `CLIENT`, named
//!   `chat {gen_ai.request.model}`, with `gen_ai.operation.name` = `chat`,
//!   `gen_ai.provider.name` and `gen_ai.request.model`; `server.address` and
//!   `server.port` when the model says where it is served
//!   ([`ToolModel::server`]); and what the provider reported: the token
//!   counts (`gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`),
//!   `gen_ai.response.model`, `gen_ai.response.id` and
//!   `gen_ai.response.finish_reasons`;
//! * a tool call is an execute-tool span: kind `INTERNAL`, named
//!   `execute_tool {gen_ai.tool.name}`, with `gen_ai.operation.name` =
//!   `execute_tool`, `gen_ai.tool.name`, `gen_ai.tool.call.id`,
//!   `gen_ai.tool.type` = `function` and `gen_ai.tool.description`.
//!
//! Both get `error.type` and `Error` status when they fail, and when they are
//! abandoned unfinished — the loop's cancellation token fired or its future
//! was dropped — `error.type` = `cancelled`; a turn that ends without a reply
//! records `gen_ai.response.finish_reasons` = `["error"]`, as the conventions
//! ask for "a finish reason [that] was expected but not received". Both open
//! inside whatever span is current, which for `ToolLoopExecutor` is the
//! executor's `invoke_agent` span (`agent.rs`). With the `otel` feature each
//! also records its metrics (`tool_metrics.rs`).
//!
//! **No content.** The conventions make the prompt, the tool arguments and
//! the tool result (`gen_ai.input.messages`, `gen_ai.tool.call.arguments`,
//! `gen_ai.tool.call.result`) Opt-In. The opt-in this crate has,
//! `RequestHandlerBuilder::with_span_content_capture`, is the handler's and
//! reaches no executor, so these spans record none of it.
//!
//! [`ToolModel::server`]: crate::tool_loop::ToolModel::server

use std::future::Future;
#[cfg(feature = "tracing")]
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "otel")]
use super::tool_metrics::Measure;
#[cfg(feature = "otel")]
pub use super::tool_metrics::{AgentRun, GenAiMeter};

/// What one model turn's reply reported, for its span and metrics.
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(not(feature = "tracing"), allow(dead_code))]
pub struct Reply<'a> {
    /// Tokens in the request.
    pub input_tokens: Option<u64>,
    /// Tokens in the reply.
    pub output_tokens: Option<u64>,
    /// The model that answered.
    pub response_model: Option<&'a str>,
    /// The provider's id for the completion.
    pub response_id: Option<&'a str>,
    /// Why the model stopped, as the provider spelled it.
    pub finish_reason: Option<&'a str>,
}

/// The span of one model turn or one tool call.
#[derive(Debug)]
pub struct GenAiSpan {
    #[cfg(feature = "tracing")]
    span: tracing::Span,
    /// Whether [`answered`](Self::answered), [`succeeded`](Self::succeeded)
    /// or [`failed`](Self::failed) ended it; one dropped before that was
    /// abandoned.
    #[cfg(feature = "tracing")]
    ended: AtomicBool,
    /// Whether it is a model turn, which owes a finish reason.
    #[cfg(feature = "tracing")]
    chat: bool,
    #[cfg(feature = "otel")]
    measure: Measure,
}

impl GenAiSpan {
    /// Opens the span of one turn of `model` at `provider`, served at
    /// `server`.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn chat(
        provider: &str,
        model: &str,
        server: Option<(&str, u16)>,
        meter: &GenAiMeter,
    ) -> Self {
        #[cfg(feature = "tracing")]
        {
            let span = if super::untraced() {
                tracing::Span::none()
            } else {
                tracing::info_span!(
                    target: "a2a_protocol_server::tool_loop",
                    "gen_ai.chat",
                    otel.name = tracing::field::Empty,
                    otel.kind = "client",
                    otel.status_code = tracing::field::Empty,
                    error.type = tracing::field::Empty,
                    gen_ai.operation.name = "chat",
                    gen_ai.provider.name = provider,
                    gen_ai.request.model = model,
                    server.address = server.map(|(address, _)| address),
                    server.port = server.map(|(_, port)| i64::from(port)),
                    gen_ai.response.model = tracing::field::Empty,
                    gen_ai.response.id = tracing::field::Empty,
                    gen_ai.usage.input_tokens = tracing::field::Empty,
                    gen_ai.usage.output_tokens = tracing::field::Empty,
                )
            };
            // Only for a span someone will see; recorded before it is first
            // entered, which is when `tracing-opentelemetry` takes the name.
            if !span.is_disabled() {
                span.record("otel.name", format!("chat {model}").as_str());
            }
            Self {
                span,
                ended: AtomicBool::new(false),
                chat: true,
                #[cfg(feature = "otel")]
                measure: Measure::chat(meter, provider, model, server),
            }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {}
        }
    }

    /// Opens the span of one call to tool `name`, the model's call `call_id`.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn tool(name: &str, call_id: &str, description: Option<&str>, meter: &GenAiMeter) -> Self {
        #[cfg(feature = "tracing")]
        {
            let span = if super::untraced() {
                tracing::Span::none()
            } else {
                tracing::info_span!(
                    target: "a2a_protocol_server::tool_loop",
                    "gen_ai.execute_tool",
                    otel.name = tracing::field::Empty,
                    otel.status_code = tracing::field::Empty,
                    error.type = tracing::field::Empty,
                    gen_ai.operation.name = "execute_tool",
                    gen_ai.tool.name = name,
                    gen_ai.tool.call.id = call_id,
                    gen_ai.tool.type = "function",
                    gen_ai.tool.description = description,
                )
            };
            if !span.is_disabled() {
                span.record("otel.name", format!("execute_tool {name}").as_str());
            }
            Self {
                span,
                ended: AtomicBool::new(false),
                chat: false,
                #[cfg(feature = "otel")]
                measure: Measure::tool(meter, name),
            }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {}
        }
    }

    /// Runs `fut` inside the span.
    #[cfg_attr(not(feature = "tracing"), allow(clippy::unused_self))]
    pub fn instrument<F: Future>(&self, fut: F) -> impl Future<Output = F::Output> + use<F> {
        #[cfg(feature = "tracing")]
        {
            tracing::Instrument::instrument(fut, self.span.clone())
        }
        #[cfg(not(feature = "tracing"))]
        {
            fut
        }
    }

    /// Ends a turn with what the provider reported. Only what it reported is
    /// recorded.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::unused_self, clippy::missing_const_for_fn)
    )]
    pub fn answered(&self, reply: Reply<'_>) {
        #[cfg(feature = "tracing")]
        {
            self.ended.store(true, Ordering::Relaxed);
            if let Some(n) = reply.input_tokens {
                self.span.record(
                    "gen_ai.usage.input_tokens",
                    i64::try_from(n).unwrap_or(i64::MAX),
                );
            }
            if let Some(n) = reply.output_tokens {
                self.span.record(
                    "gen_ai.usage.output_tokens",
                    i64::try_from(n).unwrap_or(i64::MAX),
                );
            }
            if let Some(model) = reply.response_model {
                self.span.record("gen_ai.response.model", model);
            }
            if let Some(id) = reply.response_id {
                self.span.record("gen_ai.response.id", id);
            }
        }
        #[cfg(feature = "otel")]
        {
            if let Some(reason) = reply.finish_reason {
                super::a2a::record_finish_reason(&self.span, reason);
            }
            self.measure.answered(
                reply.input_tokens,
                reply.output_tokens,
                reply.response_model,
            );
        }
    }

    /// Ends a call that returned.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(clippy::unused_self, clippy::missing_const_for_fn)
    )]
    pub fn succeeded(&self) {
        #[cfg(feature = "tracing")]
        self.ended.store(true, Ordering::Relaxed);
    }

    /// Marks the span failed, with `error_type` — a low-cardinality word, as
    /// the conventions ask.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::unused_self, clippy::missing_const_for_fn)
    )]
    pub fn failed(&self, error_type: &str) {
        #[cfg(feature = "tracing")]
        {
            self.ended.store(true, Ordering::Relaxed);
            self.span.record("error.type", error_type);
            self.span.record("otel.status_code", "ERROR");
            #[cfg(feature = "otel")]
            if self.chat {
                super::a2a::record_finish_reason(&self.span, "error");
            }
        }
        #[cfg(feature = "otel")]
        self.measure.failed(error_type);
    }
}

#[cfg(feature = "tracing")]
impl Drop for GenAiSpan {
    /// A turn or call nobody ended was abandoned: its token fired, or the
    /// future running it was dropped.
    fn drop(&mut self) {
        if !*self.ended.get_mut() {
            self.failed("cancelled");
        }
    }
}

/// Without `otel` there is nothing to record to.
#[cfg(not(feature = "otel"))]
#[derive(Clone, Debug, Default)]
pub struct GenAiMeter {}

/// Without `otel`, a run records nothing.
#[cfg(not(feature = "otel"))]
#[derive(Debug)]
pub struct AgentRun;

#[cfg(not(feature = "otel"))]
#[allow(clippy::unused_self, clippy::missing_const_for_fn)]
impl AgentRun {
    pub fn start(_meter: &GenAiMeter, _agent: Option<&str>, _model: &str) -> Self {
        Self
    }
    pub fn inference_call(&self) {}
    pub fn tool_call(&self) {}
    pub fn ended(&self, _error_type: Option<&str>) {}
}

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
