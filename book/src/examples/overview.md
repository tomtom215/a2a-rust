# Examples Overview

The `examples/` directory contains standalone binary crates that demonstrate real-world usage of the a2a-rust SDK. Each example builds and runs independently.

The directory also holds `harness`, a library the examples share for their coverage matrix, which has no page; [`examples/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/README.md) describes every example. Every example binary that runs A2A code starts the SDK's `Telemetry` from the `OTEL_*` environment: with `OTEL_EXPORTER_OTLP_ENDPOINT` set it exports traces, metrics and logs over OTLP, and without it nothing is built.

## At a Glance

| Example | Description | External deps | Difficulty |
|---------|-------------|--------------|------------|
| [Hello Agent](./hello-agent.md) | **Smallest complete agent** — one screen of code above its tests, the SDK plus `tokio`, no SDK feature flags | None | Beginner |
| [Deploy Agent](./deploy-agent.md) | The hello agent made shippable — env config, `/healthz` and `/readyz`, `SIGTERM` draining, a container and a Kubernetes manifest | None | Beginner |
| [Incident-Response Team](./incident-response.md) | Multi-turn input-required, delegation, streaming, cancellation | None (optional local model) | Intermediate |
| [Echo Agent](./echo-agent.md) | Every A2A method over all four bindings, with a coverage matrix that fails the run on a gap | None | Beginner |
| [Agent Team](./agent-team.md) | Five agents over JSON-RPC, REST and gRPC plus a sixth serving all four bindings: 100 end-to-end tests and a 2-result method × binding sweep, 102 on the default feature set (the dogfood CI job runs them) | None | Advanced |
| [Resilient Agent](./resilient-agent.md) | What survives a restart, a failure and a second replica, each asserted, with what the SDK does *not* do asserted too | None (PostgreSQL for two of its eight checks) | Advanced |
| [Genai Agent](./genai-agent.md) | LLM-powered agent using genai (OpenAI, Anthropic, Gemini, Ollama, etc.) | A local model server, or an API key | Intermediate |
| [Rig Agent](./rig-agent.md) | Real rig-core agent behind A2A — hosted or fully local, no mock | None (local server works keyless) | Intermediate |
| [Tool-Loop Agent](./tool-loop-agent.md) | Tool calling from the SDK alone: `ToolLoopExecutor` over `OpenAiChatModel`, no loop written | A model server | Intermediate |
| [Live Model](./live-model.md) | The SDK end to end against a real model: ten checks across four bindings, provenance, idempotency, a budget, a halt and a two-approver quorum | A model server | Advanced |
| [MCP Agent](./mcp-agent.md) | An agent whose tools come from an MCP server it discovers at startup | None (optional tool-capable local model) | Intermediate |
| [A2A → MCP Bridge](./mcp-bridge.md) | Exposes a remote A2A agent as an MCP server | None | Intermediate |
| [Langfuse Agent](./langfuse-agent.md) | Two agents in one Langfuse trace, exported by the SDK's own `Telemetry` | A Langfuse, self-hosted or Cloud | Intermediate |
| [Multi-Language Team](./multi-lang-team.md) | Rust coordinator delegating to Python, JS, Go, Java and Rust A2A agents | Worker agents | Advanced |
| [Swarm](./swarm.md) | A coordinator, supervisors and workers, every hop a real A2A call: fan-out, retry by failure class, cancel through two hops | None (a model for one optional scenario) | Advanced |

## Where to Start

- **New to A2A?** Start with the [Hello Agent](./hello-agent.md) — the entire agent fits on one screen. Then read the [Echo Agent](./echo-agent.md) for the complete request lifecycle across four bindings.

- **Evaluating the SDK?** Run the [Agent Team](./agent-team.md) — it runs 100 automated end-to-end tests across the SDK's features, then sweeps every method over every binding (102 results in all), and prints a pass/fail report.

- **Integrating an LLM?** See the [Genai Agent](./genai-agent.md) or [Rig Agent](./rig-agent.md) for patterns that bridge LLM frameworks with A2A's `AgentExecutor` trait, or the [Tool-Loop Agent](./tool-loop-agent.md) for tool calling with no framework at all.

- **Operating an agent?** The [Resilient Agent](./resilient-agent.md) shows what a restart, a failure and a second replica do to a task, and the [Swarm](./swarm.md) what fan-out and cancellation cost through two A2A hops. The [Langfuse Agent](./langfuse-agent.md) shows two agents in one trace.

- **Cross-language interop?** See the [Multi-Language Team](./multi-lang-team.md) for a Rust coordinator that talks to agents in 4 other languages.

## Common Pattern

Every example that serves A2A follows the same three-step integration pattern:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
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
let handler = Arc::new(RequestHandlerBuilder::new(MyExecutor).with_agent_card(card()).build()?);

// 3. Serve via any dispatcher
let dispatcher = JsonRpcDispatcher::new(handler);
// or: RestDispatcher, A2aRouter (Axum), gRPC
# let _ = dispatcher;
# Ok(())
# }
```

For details on each step, see [The AgentExecutor Trait](../building-agents/executor.md), [Request Handler & Builder](../building-agents/handler.md), and [Dispatchers](../building-agents/dispatchers.md).
