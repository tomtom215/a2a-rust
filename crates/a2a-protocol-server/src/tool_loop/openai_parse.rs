// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`OpenAiChatModel`]: a [`ToolModel`] for the chat-completions API that
//! `OpenAI`, llama.cpp's llama-server, vLLM and Ollama all serve.

//! A whole (unstreamed) chat completion's body, as a [`ModelTurn`].

use serde::Deserialize;
use serde_json::Value;

use super::model::{ModelError, ModelTurn, TokenUsage, ToolCall};

#[derive(Deserialize)]
struct Completion {
    id: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<Usage>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct Choice {
    message: AssistantMessage,
    finish_reason: Option<String>,
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
pub fn parse_completion(body: &[u8]) -> Result<ModelTurn, ModelError> {
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
    if let Some(reason) = choice.finish_reason {
        turn = turn.with_finish_reason(reason);
    }
    if let Some(id) = completion.id {
        turn = turn.with_response_id(id);
    }
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
