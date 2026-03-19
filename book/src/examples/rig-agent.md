# Rig Agent

Demonstrates how to wrap a [rig](https://github.com/0xPlaygrounds/rig) AI agent behind the A2A protocol. Shows the integration pattern for bridging rig's completion-based model with A2A's event-based streaming model.

**Source:** [`examples/rig-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/rig-agent)

## Running

```bash
# With mock completion (no API key needed):
cargo run -p rig-a2a-agent

# With a real provider (after uncommenting in source):
export OPENAI_API_KEY=sk-...
cargo run -p rig-a2a-agent
```

## How to connect a real rig agent

The example ships with a mock completion for zero-dependency demos. To connect a real LLM, replace the body of `RigAgentExecutor::run_rig_completion()`:

```rust
// OpenAI via rig:
use rig::providers::openai;
use rig::completion::Prompt;

let client = openai::Client::from_env();
let model = client.agent("gpt-4o")
    .preamble("You are a helpful assistant.")
    .build();
let response = model.prompt(user_text).await?;
```

This works with any rig agent type: completion, chat, RAG, tool-using, etc.

## Architecture

```text
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              RigAgentExecutor
                    │
                    ▼
              rig::Agent (LLM-powered)
```

## Key takeaway

The integration point is always `AgentExecutor` — the same ~60 line pattern as the [Genai Agent](./genai-agent.md). The only difference is which LLM framework you call inside `execute()`.
