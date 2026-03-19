<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Examples

End-to-end examples demonstrating the [a2a-rust](https://github.com/tomtom215/a2a-rust) SDK. Each example is a standalone binary crate.

## Quick start

```bash
# Run the simplest example (no external dependencies):
cargo run -p echo-agent

# Run the full SDK test suite:
cargo run -p agent-team
```

## Examples

| Example | Description | External deps | Difficulty |
|---------|-------------|--------------|------------|
| [**echo-agent**](echo-agent/) | Minimal echo agent with JSON-RPC + REST servers and 6 client demos | None | Beginner |
| [**agent-team**](agent-team/) | 4-agent team with 81+ E2E tests exercising every SDK feature | None | Advanced |
| [**genai-agent**](genai-agent/) | LLM-powered agent using [genai](https://crates.io/crates/genai) (OpenAI, Anthropic, Gemini, Ollama, etc.) | API key | Intermediate |
| [**rig-agent**](rig-agent/) | AI agent using [rig](https://github.com/0xPlaygrounds/rig) framework (mock by default, drop-in LLM support) | Optional API key | Intermediate |
| [**multi-lang-team**](multi-lang-team/) | Rust coordinator delegating to Python, JS, Go, and Java A2A agents | Worker agents | Advanced |

## What to start with

- **New to A2A?** Start with [`echo-agent`](echo-agent/) — it demonstrates the complete request lifecycle (send, stream, discover, retrieve) with no external dependencies.

- **Evaluating the SDK?** Run [`agent-team`](agent-team/) — it exercises every SDK feature with 81+ automated tests and prints a pass/fail report.

- **Integrating an LLM?** See [`genai-agent`](genai-agent/) or [`rig-agent`](rig-agent/) for patterns that bridge LLM frameworks with A2A's `AgentExecutor` trait.

- **Cross-language interop?** See [`multi-lang-team`](multi-lang-team/) for a Rust coordinator that talks to agents in 4 other languages.

## Common patterns

All examples follow the same integration pattern:

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

## Prerequisites

- Rust 1.93+ (MSRV)
- `protoc` only when using `--features grpc`
- See each example's README for additional requirements
