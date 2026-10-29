<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Model clients

The loop talks to its model through the `ToolModel` trait: one completion
turn, provider-neutral. This page covers `OpenAiChatModel`, the bundled client
for the chat-completions API, and writing a `ToolModel` for any other
provider.

## `OpenAiChatModel`

`OpenAiChatModel` sends each turn as `POST {base}/chat/completions`, the API
that OpenAI, llama.cpp's `llama-server`, vLLM and Ollama all serve:

```rust,no_run
use std::time::Duration;

use a2a_protocol_sdk::server::tool_loop::OpenAiChatModel;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
// llama.cpp's llama-server, with Qwen3's thinking turned off.
let mut extra = serde_json::Map::new();
extra.insert("chat_template_kwargs".into(), serde_json::json!({"enable_thinking": false}));

let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?
    .with_timeout(Duration::from_secs(60))
    .with_extra_body(extra);
# Ok(())
# }
```

`new(base_url, model)` takes the API's base URL including its version segment
(`https://api.openai.com/v1`, `http://127.0.0.1:8080/v1`) and the model name.
It returns `ModelError::Config` at once, rather than at the first turn, for a
URL that is not `http://` or `https://`, or an `https://` one in a build
without `tls-rustls`.

| Method | Default | What it is for |
|---|---|---|
| `with_api_key(key)` | none | Sent as `Authorization: Bearer {key}`. Local servers need none |
| `with_timeout(Duration)` | 120 s (`DEFAULT_TIMEOUT`) | Bounds each turn: connecting, sending and reading the whole response |
| `with_max_response_bytes(usize)` | 8 MiB (`DEFAULT_MAX_RESPONSE_BYTES`) | Caps the response body read, so an endpoint does not choose how much memory a turn takes |
| `with_extra_body(Map)` | none | Fields added to every request body: `temperature`, `chat_template_kwargs`, `tool_choice` and the like. The fields the client writes itself (`model`, `messages`, and `tools` when there are tools) win over an extra of the same name |
| `with_tls_config(rustls::ClientConfig)` | Mozilla's roots | A private CA, or a client certificate for mutual TLS (`tls-rustls`) |

Tool-call arguments are read whether the server sends them as a JSON string,
as OpenAI does, or as an object, as some compatible servers do. A reply's
`usage`, `model`, `id` and first choice's `finish_reason` are kept for the
budget and the spans. A non-2xx answer becomes `ModelError::Status` with the
status and at most 512 bytes of the body (`ERROR_BODY_LIMIT`).

Providers with another wire format (Anthropic's messages API, Gemini) are
reached through their OpenAI-compatible endpoints, or through a `ToolModel`
you write.

## Writing a `ToolModel`

A `ToolModel` sends one request and reports what came back. It does not run
tools, retry or loop; `ToolLoop` does.

| Method | What it is for |
|---|---|
| `model_name()` | The model asked for: `gen_ai.request.model`, and the name of each turn's span (`chat {model}`) |
| `provider_name()` | The provider, as `gen_ai.provider.name`: one of the `GenAI` conventions' well-known values (`openai`, `anthropic`, `gcp.gemini`, …) when one applies |
| `server()` | Where the provider is served, as a host and port, for `server.address` and `server.port`. The default, `None`, records neither: a model running in this process has no server |
| `complete(request)` | Runs one turn |

`complete` is given a `ChatRequest`: the `preamble` (empty for none), the
`messages` so far, oldest first, and the `tools` as `ToolSpec`s. It returns a
`ModelTurn`: the turn's text and the `ToolCall`s the model asked for (each an
id, a name and the arguments as JSON text), plus what the provider reported:

| `ModelTurn` method | Recorded as |
|---|---|
| `with_usage(TokenUsage::new(input, output))` | `gen_ai.usage.input_tokens` and `gen_ai.usage.output_tokens`; charged to the caller's `tokens` budget |
| `with_response_model(model)` | `gen_ai.response.model` |
| `with_response_id(id)` | `gen_ai.response.id` |
| `with_finish_reason(reason)` | `gen_ai.response.finish_reasons` |

A scripted model, as a test might use:

```rust,no_run
use std::future::Future;
use std::pin::Pin;

use a2a_protocol_sdk::server::tool_loop::{
    ChatRequest, ModelError, ModelTurn, TokenUsage, ToolModel,
};

/// Answers every turn with the same text and no tool calls.
struct Scripted;

impl ToolModel for Scripted {
    fn model_name(&self) -> &str { "scripted" }
    fn provider_name(&self) -> &str { "scripted" }
    fn complete<'a>(
        &'a self,
        _request: ChatRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ModelTurn, ModelError>> + Send + 'a>> {
        Box::pin(async {
            Ok(ModelTurn::new("All services are healthy.", Vec::new())
                .with_usage(TokenUsage::new(Some(42), Some(6)))
                .with_finish_reason("stop"))
        })
    }
}
```

A `ModelError` is the provider's failure, never the model's: the loop feeds a
model's mistakes back to it, and ends the run on a `ModelError` without
retrying. The variants are `Status`, `Timeout`, `Transport`,
`InvalidResponse`, `ResponseTooLarge`, `Config`, and `Other` for a failure of
a `ToolModel` you wrote. What the caller is told for each is on
[Running the loop](./running.md#how-a-run-fails).
