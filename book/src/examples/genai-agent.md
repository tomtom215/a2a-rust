# Genai Agent

Demonstrates how to wrap a [genai](https://crates.io/crates/genai) multi-provider LLM client behind the A2A protocol. Incoming A2A messages are forwarded to the LLM, and the response is returned as an A2A artifact.

**Source:** [`examples/genai-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/genai-agent)

## Supported providers

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
export OPENAI_API_KEY=sk-...
cargo run -p genai-a2a-agent

# Use a different model:
GENAI_MODEL=claude-sonnet-4-20250514 cargo run -p genai-a2a-agent
```

## How it works

```text
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

## Key integration point

The `GenaiAgentExecutor` implements `AgentExecutor` — the single trait that bridges any LLM framework with A2A. The integration is ~60 lines of code: extract text from the A2A message, call the genai client, and emit the result as an artifact.

This pattern is the same for any LLM integration — see also the [Rig Agent](./rig-agent.md) for an alternative framework.
