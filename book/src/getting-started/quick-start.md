# Quick Start

This guide gets you running the built-in echo agent example in under 5 minutes. The example demonstrates the full A2A stack: agent executor, dual-transport server, and client — all in a single binary.

## Running the Echo Agent

Clone the repository and run the example:

```bash
git clone https://github.com/tomtom215/a2a-rust.git
cd a2a-rust
cargo run -p echo-agent
```

You'll see output like (ports are randomly assigned):

```
=== A2A Echo Agent Example ===

JSON-RPC server listening on http://127.0.0.1:<port>
REST server listening on http://127.0.0.1:<port>

--- Demo 1: Synchronous SendMessage (JSON-RPC) ---
  Task ID:    550e8400-e29b-41d4-a716-446655440000
  Status:     Completed
  Artifact:   echo-artifact
  Content:    Echo: Hello from JSON-RPC client!

--- Demo 2: Streaming SendMessage (JSON-RPC) ---
  Status update: Working
  Artifact update: echo-artifact
  Content:    Echo: Hello from streaming client!
  Status update: Completed

--- Demo 3: Synchronous SendMessage (REST) ---
  Task ID:    ...
  Status:     Completed
  Content:    Echo: Hello from REST client!

--- Demo 4: Streaming SendMessage (REST) ---
  Status update: Working
  Content:    Echo: Hello from REST streaming!
  Status update: Completed

--- Demo 5: Agent Card Discovery ---
  Agent:      Echo Agent
  Version:    1.0.0
  Skills:     ["Echo"]
  Streaming:  true
  Interfaces: 2

--- Demo 6: GetTask ---
  Fetched task: ... (Completed)

=== All demos completed successfully! ===
```

## What Just Happened?

The example exercised all major protocol operations:

1. **Synchronous send** (JSON-RPC) — Client sends a message, waits for the complete task
2. **Streaming send** (JSON-RPC) — Client receives real-time SSE events as the agent works
3. **Synchronous send** (REST) — Same operation over the REST transport
4. **Streaming send** (REST) — SSE streaming over REST
5. **Agent card discovery** — Fetches `/.well-known/agent-card.json` to discover the agent's capabilities
6. **GetTask** — Retrieves a previously completed task by ID

## The Code in Brief

The echo agent's core executor is straightforward. Here's the key part:

```rust
use a2a_protocol_sdk::prelude::*;
use std::future::Future;
use std::pin::Pin;

struct EchoExecutor;

impl AgentExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            // 1. Transition to Working
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus::new(TaskState::Working),
                metadata: None,
            })).await?;

            // 2. Extract text from incoming message
            let input = ctx.message.parts.iter()
                .find_map(|p| match &p.content {
                    a2a_protocol_types::message::PartContent::Text(text) => Some(text.as_str()),
                    _ => None,
                })
                .unwrap_or("<no text>");

            // 3. Echo back as an artifact
            queue.write(StreamResponse::ArtifactUpdate(TaskArtifactUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                artifact: Artifact::new("echo-artifact", vec![Part::text(
                    &format!("Echo: {input}")
                )]),
                append: None,
                last_chunk: Some(true),
                metadata: None,
            })).await?;

            // 4. Transition to Completed
            queue.write(StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
                task_id: ctx.task_id.clone(),
                context_id: ContextId::new(ctx.context_id.clone()),
                status: TaskStatus::new(TaskState::Completed),
                metadata: None,
            })).await?;

            Ok(())
        })
    }
}
```

The pattern is always: write status updates and artifacts to the event queue, then return `Ok(())`.

## With Tracing

Enable structured logging to see the protocol internals:

```bash
cargo run -p echo-agent --features echo-agent/tracing
RUST_LOG=debug cargo run -p echo-agent --features echo-agent/tracing
```

## Next Steps

- **[Your First Agent](./first-agent.md)** — Build your own agent from scratch
- **[Project Structure](./project-structure.md)** — Understand how the crates fit together
- **[Examples](../examples/overview.md)** — Browse all examples with LLM integrations, multi-agent teams, and more
- **[The AgentExecutor Trait](../building-agents/executor.md)** — Deep dive into the executor API
