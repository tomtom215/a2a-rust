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
  'https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf'
llama-server -m model.gguf --port 11434 --alias qwen2.5-0.5b-instruct &
GENAI_MODEL=qwen2.5-0.5b-instruct cargo run -p genai-a2a-agent
```

(Ollama works too: `ollama pull qwen2.5:0.5b`, then
`GENAI_MODEL=qwen2.5:0.5b cargo run -p genai-a2a-agent`.)

Set `A2A_BIND_ADDR=127.0.0.1:8080` for a fixed port instead of a random one.

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
curl -X POST http://127.0.0.1:<port> -H 'Content-Type: application/json' -d '{
  "jsonrpc": "2.0", "id": 1, "method": "message/send",
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
