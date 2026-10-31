# Examples Overview

The `examples/` directory holds sixteen crates: fifteen runnable examples
that show the a2a-rust SDK doing real work, and `harness`, the library several
of them share to score their coverage. Each is a workspace member that
depends on the SDK by path, so run it from the repository root. Use this page
to pick one: the table says what each shows and what it needs, and the
learning path below it says what order to read them in.

## Choose an example

"Nothing" means a plain `cargo run` needs no model, key, service or other
language. The package name is not always the directory name; each page gives
the exact command.

| Example | What it shows | Needs | Level |
|---------|---------------|-------|-------|
| [Hello Agent](./hello-agent.md) | The smallest complete agent: an executor, a card and a JSON-RPC listener in one file, using only the SDK's prelude | Nothing | Beginner |
| [Echo Agent](./echo-agent.md) | Every A2A method over all four bindings from one handler, with a coverage matrix that fails the run on a gap, and counter-tests for the calls that must be refused | Nothing | Beginner |
| [Deploy Agent](./deploy-agent.md) | The hello agent made shippable: environment configuration, `/health` and `/ready`, `SIGTERM` draining, a container and a Kubernetes manifest | Nothing locally; Docker or Kubernetes to deploy; PostgreSQL for more than one replica | Beginner |
| [Incident-Response Team](./incident-response.md) | Three agents handling an incident: multi-turn `INPUT_REQUIRED`, delegation, streaming progress, cancellation; then the coverage matrix and sixteen production-hardening checks | Nothing; a local model or an API key, optionally; PostgreSQL for one check | Intermediate |
| [Agent Team](./agent-team.md) | The SDK's dogfood suite: five agents over JSON-RPC, REST and gRPC plus a sixth serving all four bindings, 100 end-to-end tests and a 2-result method × binding sweep, 102 on the default feature set | Nothing | Advanced |
| [Resilient Agent](./resilient-agent.md) | What survives a restart, a failure and a second replica, each asserted, with what the SDK does *not* do asserted too | Nothing; PostgreSQL for two of its eight checks | Advanced |
| [Genai Agent](./genai-agent.md) | An LLM-backed agent through genai (OpenAI, Anthropic, Gemini, local servers and others), on all four bindings with the coverage matrix | Nothing for the self-check, which labels its answers as fallbacks; a local model server or an API key for real answers | Intermediate |
| [Rig Agent](./rig-agent.md) | A rig-core agent with a bounded tool loop behind A2A, showing where tool calling sits relative to the protocol | Nothing for the conformance run; a tool-capable model, local or hosted, to see a tool call | Intermediate |
| [Tool-Loop Agent](./tool-loop-agent.md) | Tool calling from the SDK alone: `ToolLoopExecutor` over `OpenAiChatModel`, no loop written | An OpenAI-compatible model server (`cargo test` needs none) | Intermediate |
| [Live Model](./live-model.md) | The SDK end to end against a real model: ten checks across four bindings, provenance, idempotency, a budget, a halt and a two-approver quorum | An OpenAI-compatible model server | Advanced |
| [MCP Agent](./mcp-agent.md) | An agent whose tools come from an MCP server it discovers at startup | Nothing; a tool-capable model to exercise the MCP leg | Intermediate |
| [A2A → MCP Bridge](./mcp-bridge.md) | A remote A2A agent exposed as an MCP server, callable as a tool | Nothing | Intermediate |
| [Langfuse Agent](./langfuse-agent.md) | Two agents in one Langfuse trace, exported by the SDK's own `Telemetry` with no tracing code in either agent | A Langfuse: self-hosted with Docker Compose, or a Langfuse Cloud project's keys | Intermediate |
| [Multi-Language Team](./multi-lang-team.md) | A Rust coordinator fanning out to Python, JavaScript, Go, Java and Rust workers | Nothing with the Rust worker; Python, Node.js, Go or Java for the others, optionally | Advanced |
| [Swarm](./swarm.md) | A coordinator, supervisors and workers, every hop a real A2A call: fan-out, retry by failure class, cancellation through two hops | Nothing; a model for one optional scenario | Advanced |
| [Example Harness and Go Interop](./harness.md) | The library that scores the coverage matrix for six of the examples, and a check of this SDK against the official Go SDK in both directions | Nothing for its tests; Go for `scripts/go_sdk_interop.sh` | Reference |

## A learning path

1. **[Hello Agent](./hello-agent.md).** The whole agent fits on one screen:
   the executor, the card, the listener.
2. **[Echo Agent](./echo-agent.md).** The same idea over all four bindings
   and every method, and how this project checks a claim of coverage.
3. **[Incident-Response Team](./incident-response.md).** The recommended
   first multi-agent example: a task that pauses for input, delegates, streams
   and can be cancelled.
4. Then branch by what you are building:
   - **Shipping one agent:** [Deploy Agent](./deploy-agent.md), then
     [Resilient Agent](./resilient-agent.md) for what a restart and a second
     replica do to a task.
   - **Putting a model behind it:** [Genai Agent](./genai-agent.md) for one
     model call; [Tool-Loop Agent](./tool-loop-agent.md) or
     [Rig Agent](./rig-agent.md) for tool calling;
     [Live Model](./live-model.md) for the whole SDK against a real model.
   - **Using MCP:** [MCP Agent](./mcp-agent.md) for tools from an MCP server,
     [A2A → MCP Bridge](./mcp-bridge.md) for an agent as an MCP tool.
   - **Operating many agents:** [Swarm](./swarm.md) for fan-out and
     cancellation through two hops, [Langfuse Agent](./langfuse-agent.md) for
     two agents in one trace.
   - **Talking to other SDKs:** [Multi-Language Team](./multi-lang-team.md)
     for the delegation shape, [Example Harness and Go Interop](./harness.md)
     for the measured check.
   - **Evaluating the SDK as a whole:** [Agent Team](./agent-team.md), which
     runs 100 end-to-end tests and the method × binding sweep and prints a
     pass/fail report.

## Telemetry in every example

Every example binary that runs A2A code starts the SDK's `Telemetry` from the
`OTEL_*` environment: with `OTEL_EXPORTER_OTLP_ENDPOINT` set it exports
traces, metrics and logs over OTLP, and without it nothing is built, so a
plain `cargo run` needs no collector. `OTEL_TRACES_EXPORTER=console` (and the
metrics and logs equivalents) prints the signal instead.
[`examples/README.md`](https://github.com/tomtom215/a2a-rust/blob/main/examples/README.md)
describes every example from the repository's side.

## Common pattern

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
