// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The spans of the tool loop (`tool_loop`): one per model turn and one per
//! tool call, as the OpenTelemetry `GenAI` conventions name them —
//! `docs/gen-ai/gen-ai-spans.md` in `open-telemetry/semantic-conventions-genai`
//! at `4f85037` (2026-10-06), all `development`:
//!
//! * a model turn is an inference span: kind `CLIENT`, named
//!   `chat {gen_ai.request.model}`, with `gen_ai.operation.name` = `chat`,
//!   `gen_ai.provider.name` and `gen_ai.request.model`; the token counts
//!   (`gen_ai.usage.input_tokens`, `gen_ai.usage.output_tokens`) and
//!   `gen_ai.response.model` when the provider reported them;
//! * a tool call is an execute-tool span: kind `INTERNAL`, named
//!   `execute_tool {gen_ai.tool.name}`, with `gen_ai.operation.name` =
//!   `execute_tool`, `gen_ai.tool.name`, `gen_ai.tool.call.id`,
//!   `gen_ai.tool.type` = `function` and `gen_ai.tool.description`.
//!
//! Both get `error.type` and `Error` status when they fail. Both open inside
//! whatever span is current, which for `ToolLoopExecutor` is the executor's
//! `invoke_agent` span (`agent.rs`).
//!
//! **No content.** The conventions make the prompt, the tool arguments and
//! the tool result (`gen_ai.input.messages`, `gen_ai.tool.call.arguments`,
//! `gen_ai.tool.call.result`) Opt-In. The opt-in this crate has,
//! `RequestHandlerBuilder::with_span_content_capture`, is the handler's and
//! reaches no executor, so these spans record none of it.

use std::future::Future;

/// The span of one model turn or one tool call.
#[derive(Debug)]
pub struct GenAiSpan {
    #[cfg(feature = "tracing")]
    span: tracing::Span,
}

impl GenAiSpan {
    /// Opens the span of one turn of `model` at `provider`.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::missing_const_for_fn)
    )]
    pub fn chat(provider: &str, model: &str) -> Self {
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
                    gen_ai.response.model = tracing::field::Empty,
                    gen_ai.usage.input_tokens = tracing::field::Empty,
                    gen_ai.usage.output_tokens = tracing::field::Empty,
                )
            };
            // Only for a span someone will see; recorded before it is first
            // entered, which is when `tracing-opentelemetry` takes the name.
            if !span.is_disabled() {
                span.record("otel.name", format!("chat {model}").as_str());
            }
            Self { span }
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
    pub fn tool(name: &str, call_id: &str, description: Option<&str>) -> Self {
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
            Self { span }
        }
        #[cfg(not(feature = "tracing"))]
        {
            Self {}
        }
    }

    /// Runs `fut` inside the span.
    #[cfg_attr(not(feature = "tracing"), allow(clippy::unused_self))]
    pub fn instrument<F: Future>(&self, fut: F) -> impl Future<Output = F::Output> {
        #[cfg(feature = "tracing")]
        {
            tracing::Instrument::instrument(fut, self.span.clone())
        }
        #[cfg(not(feature = "tracing"))]
        {
            fut
        }
    }

    /// Records the token counts the provider reported, and the model it
    /// says answered.
    #[cfg_attr(
        not(feature = "tracing"),
        allow(unused_variables, clippy::unused_self, clippy::missing_const_for_fn)
    )]
    pub fn answered(
        &self,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        response_model: Option<&str>,
    ) {
        #[cfg(feature = "tracing")]
        {
            if let Some(n) = input_tokens {
                self.span.record(
                    "gen_ai.usage.input_tokens",
                    i64::try_from(n).unwrap_or(i64::MAX),
                );
            }
            if let Some(n) = output_tokens {
                self.span.record(
                    "gen_ai.usage.output_tokens",
                    i64::try_from(n).unwrap_or(i64::MAX),
                );
            }
            if let Some(model) = response_model {
                self.span.record("gen_ai.response.model", model);
            }
        }
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
            self.span.record("error.type", error_type);
            self.span.record("otel.status_code", "ERROR");
        }
    }
}

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
