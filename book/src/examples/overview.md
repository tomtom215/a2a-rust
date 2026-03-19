# Examples Overview

The `examples/` directory contains standalone binary crates that demonstrate real-world usage of the a2a-rust SDK. Each example builds and runs independently.

## At a Glance

| Example | Description | External deps | Difficulty |
|---------|-------------|--------------|------------|
| [Echo Agent](./echo-agent.md) | Minimal echo agent with JSON-RPC + REST servers and 6 client demos | None | Beginner |
| [Agent Team](./agent-team.md) | 4-agent team with 81+ E2E tests exercising every SDK feature | None | Advanced |
| [Genai Agent](./genai-agent.md) | LLM-powered agent using genai (OpenAI, Anthropic, Gemini, Ollama, etc.) | API key | Intermediate |
| [Rig Agent](./rig-agent.md) | AI agent using the rig framework (mock by default, drop-in LLM support) | Optional API key | Intermediate |
| [Multi-Language Team](./multi-lang-team.md) | Rust coordinator delegating to Python, JS, Go, and Java A2A agents | Worker agents | Advanced |

## Where to Start

- **New to A2A?** Start with the [Echo Agent](./echo-agent.md) — it demonstrates the complete request lifecycle with no external dependencies.

- **Evaluating the SDK?** Run the [Agent Team](./agent-team.md) — it exercises every SDK feature with 81+ automated tests and prints a pass/fail report.

- **Integrating an LLM?** See the [Genai Agent](./genai-agent.md) or [Rig Agent](./rig-agent.md) for patterns that bridge LLM frameworks with A2A's `AgentExecutor` trait.

- **Cross-language interop?** See the [Multi-Language Team](./multi-lang-team.md) for a Rust coordinator that talks to agents in 4 other languages.

## Common Pattern

All examples follow the same three-step integration pattern:

```rust
// 1. Implement AgentExecutor
struct MyExecutor;
impl AgentExecutor for MyExecutor {
    fn execute(&self, ctx: &RequestContext, queue: &dyn EventQueueWriter) -> ... {
        queue.write(StatusUpdate { state: Working }).await?;
        // ... do work ...
        queue.write(ArtifactUpdate { artifact }).await?;
        queue.write(StatusUpdate { state: Completed }).await?;
    }
}

// 2. Build a handler
let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_agent_card(card)
    .build()?;

// 3. Serve via any dispatcher
let dispatcher = JsonRpcDispatcher::new(handler);
// or: RestDispatcher, A2aRouter (Axum), gRPC
```

For details on each step, see [The AgentExecutor Trait](../building-agents/executor.md), [Request Handler & Builder](../building-agents/handler.md), and [Dispatchers](../building-agents/dispatchers.md).
