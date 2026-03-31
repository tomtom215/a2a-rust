<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Genai Agent — LLM-Powered A2A Agent

Demonstrates how to wrap a [genai](https://crates.io/crates/genai) multi-provider LLM client behind the A2A protocol. Incoming A2A messages are forwarded to the LLM, and the response is returned as an A2A artifact.

## Supported LLM providers

`genai` supports these providers out of the box:

| Provider | Model example | Env var |
|----------|--------------|---------|
| OpenAI | `gpt-4o`, `gpt-4o-mini` | `OPENAI_API_KEY` |
| Anthropic | `claude-sonnet-4-20250514` | `ANTHROPIC_API_KEY` |
| Google Gemini | `gemini-1.5-flash` | `GEMINI_API_KEY` |
| Ollama | `llama3` | (local, no key) |
| Groq | `llama3-70b-8192` | `GROQ_API_KEY` |
| Cohere | `command-r-plus` | `COHERE_API_KEY` |

## Running

```bash
# Set API key for your provider:
export OPENAI_API_KEY=sk-...

# Start the agent (defaults to gpt-4o-mini):
cargo run -p genai-a2a-agent

# Use a different model:
GENAI_MODEL=claude-sonnet-4-20250514 cargo run -p genai-a2a-agent
```

## How it works

```
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              GenaiAgentExecutor
                    │
              1. Extract user text from A2A message
              2. Transition to Working
              3. Call genai::Client for LLM completion
              4. Package response as A2A artifact
              5. Transition to Completed
```

## Testing

Once running, test with the TCK or any A2A client:

```bash
# With the TCK:
cargo run -p a2a-tck -- --url http://127.0.0.1:<port>

# Or send a request with curl:
curl -X POST http://127.0.0.1:<port> \
  -H 'Content-Type: application/json' \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "SendMessage",
    "params": {
      "message": {
        "role": "ROLE_USER",
        "parts": [{"text": "What is the A2A protocol?"}]
      }
    }
  }'
```

## Key integration point

The `GenaiAgentExecutor` struct implements `AgentExecutor` — the only trait you need to bridge any LLM framework with A2A:

```rust
impl AgentExecutor for GenaiAgentExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,    // incoming A2A message
        queue: &'a dyn EventQueueWriter,  // emit status + artifacts
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        // Extract text → call LLM → emit artifact → done
    }
}
```

## Prerequisites

- Rust 1.93+ (MSRV)
- API key for at least one supported LLM provider

## License

Apache-2.0
