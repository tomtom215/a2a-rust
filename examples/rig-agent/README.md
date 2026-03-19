<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Rig Agent — AI Framework Integration

Demonstrates how to wrap a [rig](https://github.com/0xPlaygrounds/rig) AI agent behind the A2A protocol. Shows the integration pattern for bridging rig's completion-based model with A2A's event-based streaming model.

## Architecture

```
A2A Client ──→ A2A Server (JSON-RPC)
                    │
                    ▼
              RigAgentExecutor
                    │
                    ▼
              rig::Agent (LLM-powered)
```

## Running

```bash
# With mock completion (no API key needed):
cargo run -p rig-a2a-agent

# With a real rig provider (uncomment the code in run_rig_completion):
export OPENAI_API_KEY=sk-...
cargo run -p rig-a2a-agent
```

## How to connect a real rig agent

The example ships with a mock completion for zero-dependency demo purposes. To connect a real LLM, replace the body of `RigAgentExecutor::run_rig_completion()`:

### OpenAI

```rust
use rig::providers::openai;
use rig::completion::Prompt;

let client = openai::Client::from_env();
let model = client.agent("gpt-4o")
    .preamble("You are a helpful assistant.")
    .build();

let response = model.prompt(user_text).await
    .map_err(|e| format!("rig error: {e}"))?;
Ok(response)
```

### Anthropic

```rust
use rig::providers::anthropic;
use rig::completion::Prompt;

let client = anthropic::Client::from_env();
let model = client.agent("claude-sonnet-4-20250514")
    .preamble("You are a helpful assistant.")
    .build();

let response = model.prompt(user_text).await
    .map_err(|e| format!("rig error: {e}"))?;
Ok(response)
```

## Testing

Once running, test with the TCK or any A2A client:

```bash
cargo run -p a2a-tck -- --url http://127.0.0.1:<port>
```

## Key integration point

The `RigAgentExecutor` implements `AgentExecutor` — the single trait that bridges any AI framework with A2A:

```rust
impl AgentExecutor for RigAgentExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,         // incoming A2A message
        queue: &'a dyn EventQueueWriter, // emit status + artifacts
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        // 1. Extract user text from A2A message
        // 2. Emit Working status
        // 3. Call rig agent
        // 4. Emit artifact with response
        // 5. Emit Completed status
    }
}
```

This pattern works with any rig agent type: completion, chat, RAG, tool-using, etc.

## What it demonstrates

| Feature | How |
|---------|-----|
| **AI framework integration** | Bridges rig's `Prompt` trait with A2A's `AgentExecutor` |
| **Status lifecycle** | Working → Artifact → Completed transitions |
| **Error handling** | LLM errors are returned as artifact text, not server errors |
| **Extensibility** | Drop-in replacement for any rig provider |

## Prerequisites

- Rust 1.93+ (MSRV)
- API key for your chosen rig provider (optional — works with mock by default)

## License

Apache-2.0
