# Examples Overview

The `examples/` directory contains standalone binary crates that demonstrate real-world usage of the a2a-rust SDK. Each example builds and runs independently.

## At a Glance

| Example | Description | External deps | Difficulty |
|---------|-------------|--------------|------------|
| [Hello Agent](./hello-agent.md) | **Smallest complete agent** — 28 lines of code above its tests, the SDK plus `tokio`, no SDK feature flags | None | Beginner |
| [Deploy Agent](./deploy-agent.md) | The hello agent made shippable — env config, `/healthz` and `/readyz`, `SIGTERM` draining, a container and a Kubernetes manifest | None | Beginner |
| [Incident-Response Team](./incident-response.md) | Multi-turn input-required, delegation, streaming, cancellation | None (optional local model) | Intermediate |
| [Echo Agent](./echo-agent.md) | Every A2A method over all four bindings, with a coverage matrix that fails the run on a gap | None | Beginner |
| [Agent Team](./agent-team.md) | 4-agent team with 102 E2E tests on its default feature set (the dogfood CI job runs them) | None | Advanced |
| [Genai Agent](./genai-agent.md) | LLM-powered agent using genai (OpenAI, Anthropic, Gemini, Ollama, etc.) | A local model server, or an API key | Intermediate |
| [Rig Agent](./rig-agent.md) | Real rig-core agent behind A2A — hosted or fully local, no mock | None (local server works keyless) | Intermediate |
| [MCP Agent](./mcp-agent.md) | An agent whose tools come from an MCP server it discovers at startup | None (optional tool-capable local model) | Intermediate |
| [A2A → MCP Bridge](./mcp-bridge.md) | Exposes a remote A2A agent as an MCP server | None | Intermediate |
| [Multi-Language Team](./multi-lang-team.md) | Rust coordinator delegating to Python, JS, Go, Java and Rust A2A agents | Worker agents | Advanced |

## Where to Start

- **New to A2A?** Start with the [Hello Agent](./hello-agent.md) — the entire agent fits on one screen. Then read the [Echo Agent](./echo-agent.md) for the complete request lifecycle across four bindings.

- **Evaluating the SDK?** Run the [Agent Team](./agent-team.md) — it runs 102 automated end-to-end tests across the SDK's features and prints a pass/fail report.

- **Integrating an LLM?** See the [Genai Agent](./genai-agent.md) or [Rig Agent](./rig-agent.md) for patterns that bridge LLM frameworks with A2A's `AgentExecutor` trait.

- **Cross-language interop?** See the [Multi-Language Team](./multi-lang-team.md) for a Rust coordinator that talks to agents in 4 other languages.

## Common Pattern

Every example that serves A2A follows the same three-step integration pattern:

```rust,no_run
use a2a_protocol_sdk::prelude::*;
use std::sync::Arc;

// 1. Implement AgentExecutor. The macro writes the impl; the trait's real
//    signature returns Pin<Box<dyn Future>> to stay object-safe.
struct MyExecutor;

agent_executor!(MyExecutor, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    // ... do work ...
    emit.artifact("result", vec![Part::text("done")], None, Some(true)).await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});

# fn main() -> Result<(), Box<dyn std::error::Error>> {
// 2. Build a handler
let handler = Arc::new(RequestHandlerBuilder::new(MyExecutor).build()?);

// 3. Serve via any dispatcher
let dispatcher = JsonRpcDispatcher::new(handler);
// or: RestDispatcher, A2aRouter (Axum), gRPC
# let _ = dispatcher;
# Ok(())
# }
```

For details on each step, see [The AgentExecutor Trait](../building-agents/executor.md), [Request Handler & Builder](../building-agents/handler.md), and [Dispatchers](../building-agents/dispatchers.md).
