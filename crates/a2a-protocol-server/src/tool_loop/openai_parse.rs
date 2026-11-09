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
    #[serde(default, deserialize_with = "count")]
    prompt_tokens: Option<u64>,
    #[serde(default, deserialize_with = "count")]
    completion_tokens: Option<u64>,
}

/// A token count as a whole, non-negative number, or `None` for anything else
/// (a float with a fraction, a negative number, a string, `null`, an object).
///
/// The counts are the provider's report of its own work, and only a budget
/// reads them. A provider that sends `1.5`, `-3` or `"12"` for one used to have
/// its whole completion refused as unreadable, an answer thrown away over a
/// number nothing needed; it now reports no usage, which charges nothing, as a
/// provider that sends none does (hostile campaign, wave 2, W2-LIVE-10). A float
/// with no fraction (`12.0`) is the number it says.
pub fn count<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().and_then(whole)),
        _ => None,
    })
}

/// 2^64: the first float no `u64` holds.
const TWO_POW_64: f64 = 18_446_744_073_709_551_616.0;

/// `f` as a `u64` when it is finite, non-negative, has no fraction and is
/// below 2^64 (at 2^64 the cast would saturate to `u64::MAX`, a count the
/// provider did not report).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // each excluded by the test
fn whole(f: f64) -> Option<u64> {
    (f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < TWO_POW_64).then_some(f as u64)
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
