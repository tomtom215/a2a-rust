// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`OpenAiChatModel`]: a [`ToolModel`] for the chat-completions API that
//! `OpenAI`, llama.cpp's llama-server, vLLM and Ollama all serve.
//!
//! It runs on the hyper client the bundled push sender uses, so it adds no
//! dependency: plaintext HTTP always, HTTPS with the `tls-rustls` feature.
//! `deny.toml` keeps reqwest out of this crate.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt as _, Full, LengthLimitError, Limited};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::model::{
    ChatMessage, ChatRequest, ModelError, ModelTurn, TokenUsage, ToolCall, ToolModel,
};
use crate::push::sender::PushHttpClient;

/// How long one turn may take, request and whole response, by default.
///
/// A small local model on a CPU can take a minute over a long conversation;
/// two is the bound past which the caller is better served by a failed task
/// than by waiting.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

/// The most of a response body read when no cap is set. A chat completion
/// is kilobytes; the cap exists so an endpoint does not get to choose how
/// much memory a turn takes.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// The most of an error response's body kept in [`ModelError::Status`].
pub const ERROR_BODY_LIMIT: usize = 512;

/// A [`ToolModel`] that sends each turn as `POST {base}/chat/completions`.
///
/// ```rust,no_run
/// use a2a_protocol_server::tool_loop::OpenAiChatModel;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // llama.cpp's llama-server, with Qwen3's thinking turned off.
/// let mut extra = serde_json::Map::new();
/// extra.insert("chat_template_kwargs".into(), serde_json::json!({"enable_thinking": false}));
/// let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?
///     .with_extra_body(extra);
/// # Ok(()) }
/// ```
///
/// Tool-call arguments are read whether the server sends them as a JSON
/// string, as `OpenAI` does, or as an object, as some compatible servers do.
pub struct OpenAiChatModel {
    client: PushHttpClient,
    endpoint: hyper::Uri,
    model: String,
    api_key: Option<String>,
    timeout: Duration,
    max_response_bytes: usize,
    extra: Map<String, Value>,
}

impl fmt::Debug for OpenAiChatModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatModel")
            .field("endpoint", &self.endpoint)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("timeout", &self.timeout)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl OpenAiChatModel {
    /// A client for `model` at `base_url`, the API's base including its
    /// version segment (`https://api.openai.com/v1`,
    /// `http://127.0.0.1:8080/v1`).
    ///
    /// # Errors
    ///
    /// [`ModelError::Config`] when `base_url` is not an `http://` or
    /// `https://` URL, or is `https://` in a build without `tls-rustls` —
    /// refused here rather than at the first turn.
    pub fn new(base_url: &str, model: impl Into<String>) -> Result<Self, ModelError> {
        let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
        let endpoint: hyper::Uri = endpoint
            .parse()
            .map_err(|e| ModelError::Config(format!("'{base_url}' is not a URL: {e}")))?;
        match endpoint.scheme_str() {
            Some("http") => {}
            #[cfg(feature = "tls-rustls")]
            Some("https") => {}
            #[cfg(not(feature = "tls-rustls"))]
            Some("https") => {
                return Err(ModelError::Config(
                    "this build of OpenAiChatModel speaks HTTP only and cannot reach an \
                     https:// endpoint; enable the `tls-rustls` feature (on by default in \
                     a2a-protocol-sdk)"
                        .to_owned(),
                ));
            }
            _ => {
                return Err(ModelError::Config(format!(
                    "'{base_url}' is not an http:// or https:// URL"
                )));
            }
        }
        Ok(Self {
            client: crate::push::sender::build_push_http_client(),
            endpoint,
            model: model.into(),
            api_key: None,
            timeout: DEFAULT_TIMEOUT,
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
            extra: Map::new(),
        })
    }

    /// Verifies the server against `tls_config` instead of Mozilla's roots:
    /// a private CA, or a client certificate for mutual TLS.
    #[cfg(feature = "tls-rustls")]
    #[must_use]
    pub fn with_tls_config(mut self, tls_config: rustls::ClientConfig) -> Self {
        self.client = crate::push::sender::build_push_https_client(tls_config);
        self
    }

    /// Sends `key` as `Authorization: Bearer {key}`. Local servers need
    /// none.
    #[must_use]
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Bounds each turn — connecting, sending and reading the whole
    /// response. Default [`DEFAULT_TIMEOUT`].
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Caps the response body read, in bytes. Default
    /// [`DEFAULT_MAX_RESPONSE_BYTES`].
    #[must_use]
    pub const fn with_max_response_bytes(mut self, max: usize) -> Self {
        self.max_response_bytes = max;
        self
    }

    /// Adds `fields` to every request body: `temperature`,
    /// `chat_template_kwargs`, `tool_choice` and the like. The fields this
    /// client writes itself — `model`, `messages`, and `tools` when there
    /// are tools — win over an extra of the same name.
    #[must_use]
    pub fn with_extra_body(mut self, fields: Map<String, Value>) -> Self {
        self.extra.extend(fields);
        self
    }

    /// The request body for one turn.
    fn request_body(&self, request: ChatRequest<'_>) -> Value {
        let mut messages = Vec::with_capacity(request.messages.len() + 1);
        if !request.preamble.is_empty() {
            messages.push(json!({"role": "system", "content": request.preamble}));
        }
        messages.extend(request.messages.iter().map(wire_message));
        let mut body = self.extra.clone();
        body.insert("model".to_owned(), Value::from(self.model.as_str()));
        body.insert("messages".to_owned(), Value::Array(messages));
        if !request.tools.is_empty() {
            let tools = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters,
                        }
                    })
                })
                .collect();
            body.insert("tools".to_owned(), Value::Array(tools));
        }
        Value::Object(body)
    }

    /// Sends one turn and reads its answer, without the timeout.
    async fn exchange(&self, body: Bytes) -> Result<ModelTurn, ModelError> {
        let mut builder = hyper::Request::post(self.endpoint.clone())
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .header(hyper::header::ACCEPT, "application/json");
        if let Some(key) = &self.api_key {
            let mut value = hyper::header::HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|_| {
                    ModelError::Config(
                        "the API key holds characters an HTTP header cannot carry".to_owned(),
                    )
                })?;
            value.set_sensitive(true);
            builder = builder.header(hyper::header::AUTHORIZATION, value);
        }
        let request = builder
            .body(Full::new(body))
            .map_err(|e| ModelError::Config(format!("the request could not be built: {e}")))?;
        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| ModelError::Transport(e.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let body = read_prefix(response.into_body(), ERROR_BODY_LIMIT).await;
            return Err(ModelError::Status {
                status: status.as_u16(),
                body: truncated(&body, ERROR_BODY_LIMIT),
            });
        }
        let body = Limited::new(response.into_body(), self.max_response_bytes)
            .collect()
            .await
            .map_err(|e| {
                if e.downcast_ref::<LengthLimitError>().is_some() {
                    ModelError::ResponseTooLarge(self.max_response_bytes)
                } else {
                    ModelError::Transport(e.to_string())
                }
            })?
            .to_bytes();
        parse_completion(&body)
    }
}

impl ToolModel for OpenAiChatModel {
    fn model_name(&self) -> &str {
        &self.model
    }

    /// `openai`: the conventions name the API a client speaks, not the
    /// server behind it ("multiple systems … are accessible by `OpenAI`
    /// client libraries. In such cases, `gen_ai.provider.name` is set to
    /// `openai`").
    fn provider_name(&self) -> &'static str {
        "openai"
    }

    fn complete<'a>(
        &'a self,
        request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        let body = serde_json::to_vec(&self.request_body(request)).map(Bytes::from);
        Box::pin(async move {
            let body = body.map_err(|e| ModelError::Config(format!("request encoding: {e}")))?;
            tokio::time::timeout(self.timeout, self.exchange(body))
                .await
                .unwrap_or(Err(ModelError::Timeout(self.timeout)))
        })
    }
}

/// One conversation message on the wire.
fn wire_message(message: &ChatMessage) -> Value {
    match message {
        ChatMessage::System(text) => json!({"role": "system", "content": text}),
        ChatMessage::User(text) => json!({"role": "user", "content": text}),
        ChatMessage::Assistant { text, tool_calls } if tool_calls.is_empty() => {
            json!({"role": "assistant", "content": text})
        }
        ChatMessage::Assistant { text, tool_calls } => {
            let calls: Vec<Value> = tool_calls
                .iter()
                .map(|c| {
                    json!({
                        "id": c.id,
                        "type": "function",
                        "function": {"name": c.name, "arguments": c.arguments},
                    })
                })
                .collect();
            let content = if text.is_empty() {
                Value::Null
            } else {
                Value::from(text.as_str())
            };
            json!({"role": "assistant", "content": content, "tool_calls": calls})
        }
        ChatMessage::ToolResult {
            call_id, content, ..
        } => json!({"role": "tool", "tool_call_id": call_id, "content": content}),
    }
}

/// Reads up to `limit` bytes of `body` and drops the rest unread.
async fn read_prefix(mut body: hyper::body::Incoming, limit: usize) -> Vec<u8> {
    let mut out = Vec::new();
    while out.len() < limit {
        match body.frame().await {
            Some(Ok(frame)) => {
                if let Ok(data) = frame.into_data() {
                    out.extend_from_slice(&data);
                }
            }
            Some(Err(_)) | None => break,
        }
    }
    out
}

/// `bytes` as text, cut to at most `limit` bytes on a character boundary.
fn truncated(bytes: &[u8], limit: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= limit {
        return text.into_owned();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[derive(Deserialize)]
struct Completion {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<Usage>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
}

#[derive(Deserialize)]
struct AssistantMessage {
    content: Option<Value>,
    #[serde(default)]
    tool_calls: Option<Vec<WireCall>>,
}

#[derive(Deserialize)]
struct WireCall {
    id: Option<String>,
    function: WireFunction,
}

#[derive(Deserialize)]
struct WireFunction {
    name: String,
    arguments: Option<Value>,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

/// A chat completion's first choice as a [`ModelTurn`].
fn parse_completion(body: &[u8]) -> Result<ModelTurn, ModelError> {
    let completion: Completion = serde_json::from_slice(body)
        .map_err(|e| ModelError::InvalidResponse(format!("not a chat completion: {e}")))?;
    let Some(choice) = completion.choices.into_iter().next() else {
        return Err(ModelError::InvalidResponse(
            "the completion has no choices".to_owned(),
        ));
    };
    let text = match choice.message.content {
        Some(Value::String(text)) => text,
        // The content-parts form: the text parts, in order.
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect(),
        _ => String::new(),
    };
    let calls = choice
        .message
        .tool_calls
        .unwrap_or_default()
        .into_iter()
        .map(|c| {
            let arguments = match c.function.arguments {
                Some(Value::String(s)) => s,
                None | Some(Value::Null) => String::new(),
                Some(other) => other.to_string(),
            };
            ToolCall::new(c.id.unwrap_or_default(), c.function.name, arguments)
        })
        .collect();
    let mut turn = ModelTurn::new(text, calls);
    if let Some(usage) = completion.usage {
        turn = turn.with_usage(TokenUsage::new(
            usage.prompt_tokens,
            usage.completion_tokens,
        ));
    }
    if let Some(model) = completion.model {
        turn = turn.with_response_model(model);
    }
    Ok(turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_body_is_cut_on_a_character_boundary() {
        assert_eq!(truncated(b"short", 512), "short");
        let long = "é".repeat(400); // 800 bytes
        let cut = truncated(long.as_bytes(), 512);
        assert_eq!(cut.len(), 512);
        assert_eq!(truncated("aé".as_bytes(), 2), "a");
    }

    #[test]
    fn content_parts_and_missing_fields_are_read() {
        let turn = parse_completion(
            br#"{"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]}"#,
        )
        .expect("parses");
        assert_eq!(turn.text, "ab");
        assert!(turn.usage.is_none());
        let turn = parse_completion(
            br#"{"choices":[{"message":{"content":null,"tool_calls":[{"function":{"name":"t"}}]}}]}"#,
        )
        .expect("parses");
        assert_eq!(turn.tool_calls, vec![ToolCall::new("", "t", "")]);
        assert!(matches!(
            parse_completion(br#"{"choices":[]}"#),
            Err(ModelError::InvalidResponse(_))
        ));
        assert!(matches!(
            parse_completion(b"<html>"),
            Err(ModelError::InvalidResponse(_))
        ));
    }

    #[test]
    fn a_base_url_that_cannot_work_is_refused_at_construction() {
        assert!(matches!(
            OpenAiChatModel::new("ftp://x/v1", "m"),
            Err(ModelError::Config(_))
        ));
        assert!(matches!(
            OpenAiChatModel::new("not a url", "m"),
            Err(ModelError::Config(_))
        ));
        let ok = OpenAiChatModel::new("http://127.0.0.1:1/v1/", "m").expect("http is fine");
        assert_eq!(
            ok.endpoint.to_string(),
            "http://127.0.0.1:1/v1/chat/completions"
        );
        #[cfg(not(feature = "tls-rustls"))]
        assert!(matches!(
            OpenAiChatModel::new("https://api.example/v1", "m"),
            Err(ModelError::Config(e)) if e.contains("tls-rustls")
        ));
        #[cfg(feature = "tls-rustls")]
        assert!(OpenAiChatModel::new("https://api.example/v1", "m").is_ok());
    }

    #[test]
    fn the_api_key_is_not_in_debug_output() {
        let model = OpenAiChatModel::new("http://127.0.0.1:1/v1", "m")
            .expect("valid")
            .with_api_key("sk-secret");
        let debug = format!("{model:?}");
        assert!(!debug.contains("sk-secret"), "{debug}");
        assert!(debug.contains("[redacted]"), "{debug}");
    }
}
