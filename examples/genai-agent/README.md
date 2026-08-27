<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Genai Agent — LLM-Powered A2A Agent

Wraps the [genai](https://crates.io/crates/genai) multi-provider LLM client
behind the A2A protocol. Incoming A2A messages are forwarded to the LLM, and
the response is returned as an A2A artifact.

## Supported LLM providers

| Provider | Model example | Env var |
|----------|--------------|---------|
| OpenAI | `gpt-4o`, `gpt-4o-mini` | `OPENAI_API_KEY` |
| Anthropic | `claude-sonnet-4-20250514` | `ANTHROPIC_API_KEY` |
| Google Gemini | `gemini-1.5-flash` | `GEMINI_API_KEY` |
| Ollama / any local server on `:11434` | any other model name | (local, no key) |
| Groq | `llama3-70b-8192` | `GROQ_API_KEY` |
| Cohere | `command-r-plus` | `COHERE_API_KEY` |

Model names that don't match a hosted provider's prefix route to genai's
Ollama adapter at `http://localhost:11434/v1/` — which is also the API that
llama.cpp's `llama-server` speaks.

## Running

```bash
# Hosted provider:
export OPENAI_API_KEY=sk-...
cargo run -p genai-a2a-agent             # defaults to gpt-4o-mini

# Fully local, no API key — verified with llama.cpp's llama-server:
curl -L -o model.gguf \
  'https://huggingface.co/ggml-org/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_0.gguf'
llama-server -m model.gguf --port 11434 --alias qwen3.5:0.8b \
  --chat-template-kwargs '{"enable_thinking":false}' &   # direct answers, no thinking preamble
GENAI_MODEL=qwen3.5:0.8b cargo run -p genai-a2a-agent
```

### vLLM

vLLM serves the same OpenAI-compatible API, from the unquantized weights
rather than a GGUF. Use it when you want throughput or a GPU; llama.cpp is the
lighter option for a laptop CPU.

```bash
pip install vllm
vllm serve Qwen/Qwen3.5-0.8B --port 11434 --served-model-name qwen3.5:0.8b
```

`--served-model-name` matters: the examples send the model name in the request
body, and vLLM rejects a name it was not started with. Setting it to the same
string llama.cpp's `--alias` uses means the same env var works against either
server, which is the point of both being OpenAI-compatible.

(Ollama works too: `ollama pull qwen3.5:0.8b`, then
`GENAI_MODEL=qwen3.5:0.8b cargo run -p genai-a2a-agent`.)

Set `A2A_BIND_ADDR=127.0.0.1:8080` for a fixed port instead of a random one.
This "server mode" is the deployment shape, and its webhook SSRF guard is **on
by default**: a push-notification URL that resolves to a loopback, private, or
link-local address is rejected. For local testing where the webhook itself
lives on localhost, set `A2A_ALLOW_PRIVATE_WEBHOOKS=1` to permit them; the
active posture is printed at startup.

Server mode serves JSON-RPC on `A2A_BIND_ADDR`; to expose the other bindings on
the same handler, set `A2A_GRPC_ADDR` and/or `A2A_WS_ADDR` (e.g.
`A2A_GRPC_ADDR=127.0.0.1:8081 A2A_WS_ADDR=127.0.0.1:8082`). Each is printed at
startup when set.

## How it works

```
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              GenaiAgentExecutor
                    │
              1. Extract user text (no text part → TASK_STATE_FAILED, InvalidParams)
              2. Transition to Working
              3. Call genai::Client for LLM completion
              4. Success → artifact "llm-response" + TASK_STATE_COMPLETED
                 LLM error → TASK_STATE_FAILED
```

Errors surface through the task state, never disguised as successful
artifacts — clients can trust `status.state`.

## Key integration point

`GenaiAgentExecutor` implements `AgentExecutor` — the only trait needed to
bridge any LLM library with A2A:

```rust
impl AgentExecutor for GenaiAgentExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,          // incoming A2A message
        queue: &'a dyn EventQueueWriter,  // emit status + artifacts
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        // Extract text → call LLM → emit artifact → done
        // Returning Err(...) marks the task TASK_STATE_FAILED.
    }
}
```

## Testing

```bash
# Agent discovery
curl http://127.0.0.1:<port>/.well-known/agent-card.json

# Send a message
curl -X POST http://127.0.0.1:<port> -H 'Content-Type: application/json' -H 'A2A-Version: 1.0' -d '{
  "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
  "params": {"message": {"messageId": "m1", "role": "ROLE_USER",
             "parts": [{"text": "What is 2+2?"}]}}
}'

# Full conformance suite (passes 20/20 against this agent)
cargo run -p a2a-tck -- --url http://127.0.0.1:<port> --binding jsonrpc
```

## Prerequisites

- Rust 1.93+ (MSRV)
- An API key for a hosted provider, **or** any local OpenAI-compatible
  server on port 11434 (llama-server, Ollama)

## License

Apache-2.0
