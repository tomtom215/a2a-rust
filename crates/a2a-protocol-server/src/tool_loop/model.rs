// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`ToolModel`], one completion turn of a model that can call tools, and
//! the conversation it is given.
//!
//! The types here are provider-neutral: they say what a turn needs (the
//! preamble, the conversation, the tool catalogue) and what it returns (text
//! and calls), not how any one API spells them.
//! [`OpenAiChatModel`](super::OpenAiChatModel) is the bundled translation
//! to the chat-completions wire format; any other provider is a
//! [`ToolModel`] the integrator writes.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde_json::Value;

/// One message of the conversation a [`ToolModel`] is given.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChatMessage {
    /// An instruction from the operator, beside the preamble.
    System(String),
    /// What the caller asked.
    User(String),
    /// A turn the model took: its text and the calls it asked for.
    Assistant {
        /// The text of the turn; empty when the model only called tools.
        text: String,
        /// The calls it asked for, in order.
        tool_calls: Vec<ToolCall>,
    },
    /// The result of one call, sent back to the model.
    ToolResult {
        /// The [`ToolCall::id`] this answers.
        call_id: String,
        /// The tool that ran.
        name: String,
        /// What the tool returned, or the error the loop reports for it.
        content: String,
    },
}

/// One call a model asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolCall {
    /// The provider's id for the call, which the result must carry back.
    pub id: String,
    /// The tool's name, as the model wrote it.
    pub name: String,
    /// The arguments as JSON text, as the model wrote them. Not parsed here:
    /// arguments that are not a JSON object are the model's mistake, and the
    /// loop tells the model so rather than failing the turn.
    pub arguments: String,
}

impl ToolCall {
    /// A call to `name` with `arguments` as JSON text.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        arguments: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }
}

/// What the model is told about one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToolSpec {
    /// The name the model calls it by.
    pub name: String,
    /// What it does, for the model to choose by.
    pub description: String,
    /// A JSON Schema for its arguments.
    pub parameters: Value,
}

impl ToolSpec {
    /// A specification from its three parts.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }
}

/// Everything one turn is asked with.
///
/// A provider holds no state between turns, so every request carries the
/// whole conversation and the whole catalogue.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ChatRequest<'a> {
    /// The system preamble; empty for none.
    pub preamble: &'a str,
    /// The conversation so far, oldest first.
    pub messages: &'a [ChatMessage],
    /// The tools the model may call.
    pub tools: &'a [ToolSpec],
}

impl<'a> ChatRequest<'a> {
    /// A request from its three parts.
    #[must_use]
    pub const fn new(
        preamble: &'a str,
        messages: &'a [ChatMessage],
        tools: &'a [ToolSpec],
    ) -> Self {
        Self {
            preamble,
            messages,
            tools,
        }
    }
}

/// The tokens one turn used, as the provider reported them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TokenUsage {
    /// Tokens in the request.
    pub input_tokens: Option<u64>,
    /// Tokens in the reply.
    pub output_tokens: Option<u64>,
}

impl TokenUsage {
    /// Usage from the two counts, either of which a provider may omit.
    #[must_use]
    pub const fn new(input_tokens: Option<u64>, output_tokens: Option<u64>) -> Self {
        Self {
            input_tokens,
            output_tokens,
        }
    }
}

/// What one turn produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ModelTurn {
    /// The turn's text.
    pub text: String,
    /// The calls the model asked for; empty when it answered.
    pub tool_calls: Vec<ToolCall>,
    /// Token usage, when the provider reported it.
    pub usage: Option<TokenUsage>,
    /// The model that answered, when the provider named it.
    pub response_model: Option<String>,
    /// The provider's id for the completion, when it gave one; recorded as
    /// `gen_ai.response.id`.
    pub response_id: Option<String>,
    /// Why the model stopped, as the provider spelled it (`stop`,
    /// `tool_calls`, `length`, …); recorded as
    /// `gen_ai.response.finish_reasons`.
    pub finish_reason: Option<String>,
}

impl ModelTurn {
    /// A turn of `text` and `tool_calls`.
    #[must_use]
    pub fn new(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            text: text.into(),
            tool_calls,
            usage: None,
            response_model: None,
            response_id: None,
            finish_reason: None,
        }
    }

    /// Sets the token usage.
    #[must_use]
    pub const fn with_usage(mut self, usage: TokenUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Sets the model the provider says answered.
    #[must_use]
    pub fn with_response_model(mut self, model: impl Into<String>) -> Self {
        self.response_model = Some(model.into());
        self
    }

    /// Sets the provider's id for the completion.
    #[must_use]
    pub fn with_response_id(mut self, id: impl Into<String>) -> Self {
        self.response_id = Some(id.into());
        self
    }

    /// Sets why the model stopped, as the provider spelled it.
    #[must_use]
    pub fn with_finish_reason(mut self, reason: impl Into<String>) -> Self {
        self.finish_reason = Some(reason.into());
        self
    }
}

/// Why a turn produced nothing. Each is the provider's failure, not the
/// model's: a model's mistakes are fed back to it by the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelError {
    /// The provider answered with a status outside 2xx.
    Status {
        /// The HTTP status.
        status: u16,
        /// At most 512 bytes of the response body, cut on a character
        /// boundary.
        body: String,
    },
    /// No complete answer within the request timeout.
    Timeout(Duration),
    /// The request could not be sent, or the connection failed.
    Transport(String),
    /// The answer was not a chat completion this client can read.
    InvalidResponse(String),
    /// The answer's body was larger than the configured cap, in bytes.
    ResponseTooLarge(usize),
    /// The client is configured in a way that cannot work, such as an
    /// `https://` endpoint in a build without TLS.
    Config(String),
    /// Any other failure, from a [`ToolModel`] the integrator wrote.
    Other(String),
}

impl ModelError {
    /// The low-cardinality `error.type` a span records for this failure:
    /// the status code for [`Status`](Self::Status), as the HTTP
    /// conventions do, and a fixed word for the rest.
    #[must_use]
    pub fn error_type(&self) -> String {
        match self {
            Self::Status { status, .. } => status.to_string(),
            Self::Timeout(_) => "timeout".to_owned(),
            Self::Transport(_) => "transport".to_owned(),
            Self::InvalidResponse(_) => "invalid_response".to_owned(),
            Self::ResponseTooLarge(_) => "response_too_large".to_owned(),
            Self::Config(_) => "configuration".to_owned(),
            Self::Other(_) => "_OTHER".to_owned(),
        }
    }
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status { status, body } if body.is_empty() => {
                write!(f, "the model provider answered HTTP {status}")
            }
            Self::Status { status, body } => {
                write!(f, "the model provider answered HTTP {status}: {body}")
            }
            Self::Timeout(after) => {
                write!(f, "the model provider did not answer within {after:?}")
            }
            Self::Transport(e) => write!(f, "could not reach the model provider: {e}"),
            Self::InvalidResponse(e) => {
                write!(f, "the model provider's answer could not be read: {e}")
            }
            Self::ResponseTooLarge(cap) => write!(
                f,
                "the model provider's answer is larger than the {cap}-byte cap"
            ),
            Self::Config(e) | Self::Other(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for ModelError {}

/// Where a streaming turn sends its text as it arrives: see
/// [`ToolModel::complete_streaming`]. Cheap to call for every piece; a piece
/// sent after the run ended is dropped.
#[derive(Debug, Clone)]
pub struct TextSink {
    turn: usize,
    tx: tokio::sync::mpsc::UnboundedSender<(usize, String)>,
}

impl TextSink {
    /// A sink for `turn`'s pieces, sent on `tx` with the turn number: what
    /// the loop hands a streaming turn, and what a test of a model's
    /// [`complete_streaming`](ToolModel::complete_streaming) can build.
    #[must_use]
    pub const fn new(turn: usize, tx: tokio::sync::mpsc::UnboundedSender<(usize, String)>) -> Self {
        Self { turn, tx }
    }

    /// Hands on `piece`, the next stretch of the turn's answer text. An
    /// empty piece is ignored.
    pub fn push(&self, piece: &str) {
        if !piece.is_empty() {
            let _ = self.tx.send((self.turn, piece.to_owned()));
        }
    }
}

/// One completion turn of a model that can call tools.
///
/// Object-safe, with boxed futures, like [`AgentExecutor`](crate::AgentExecutor):
/// [`ToolLoop`](super::ToolLoop) holds it as a `Box<dyn ToolModel>`.
///
/// An implementation sends the request once and reports what came back. It
/// does not run tools, retry or loop; [`ToolLoop`](super::ToolLoop) does.
pub trait ToolModel: Send + Sync + 'static {
    /// The model asked for, recorded as `gen_ai.request.model` and in the
    /// name of each turn's span (`chat {model}`).
    fn model_name(&self) -> &str;

    /// The provider, recorded as `gen_ai.provider.name`: one of the
    /// `GenAI` conventions' well-known values (`openai`, `anthropic`,
    /// `gcp.gemini`, …) when one applies.
    fn provider_name(&self) -> &str;

    /// Where the provider is served, as a host and port, recorded as
    /// `server.address` and `server.port`. `None`, the default, records
    /// neither: a model running in this process has no server.
    fn server(&self) -> Option<(&str, u16)> {
        None
    }

    /// Runs one turn.
    ///
    /// # Errors
    ///
    /// A [`ModelError`] when the provider failed. The loop ends the task on
    /// it; it never retries.
    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>>;

    /// Runs one turn as [`complete`](Self::complete) does, and hands
    /// `text` each piece of the turn's text as the provider sends it, so a
    /// caller can watch a long answer arrive (audit L5). The turn it
    /// returns is the whole of it, its text the pieces joined.
    ///
    /// The default runs [`complete`](Self::complete) and sends no piece: a
    /// model that cannot stream still works, and its turns arrive whole.
    /// [`OpenAiChatModel`](super::OpenAiChatModel) streams.
    ///
    /// # Errors
    ///
    /// As [`complete`](Self::complete).
    fn complete_streaming<'a>(
        &'a self,
        request: ChatRequest<'a>,
        text: &'a TextSink,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let _ = text;
        self.complete(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A status with no body says only the status; with one, both. The
    /// mutation gate found the empty case untested: printing `: ` after the
    /// status survived.
    #[test]
    fn a_status_error_names_its_body_only_when_it_has_one() {
        let bare = ModelError::Status {
            status: 503,
            body: String::new(),
        };
        assert_eq!(bare.to_string(), "the model provider answered HTTP 503");
        let with_body = ModelError::Status {
            status: 429,
            body: "slow down".to_owned(),
        };
        assert_eq!(
            with_body.to_string(),
            "the model provider answered HTTP 429: slow down"
        );
    }
}
