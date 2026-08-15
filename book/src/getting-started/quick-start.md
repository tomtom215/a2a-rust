# Quick Start

Two examples, in order. `hello-agent` is the whole SDK in one screen — start there. `echo-agent` is the full tour: every A2A method over every binding.

## The Smallest Agent

```bash
git clone https://github.com/tomtom215/a2a-rust.git
cd a2a-rust
cargo run -p hello-agent
```

It listens on `http://127.0.0.1:3000`. Send it a message:

```bash
curl -X POST http://127.0.0.1:3000 \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Tom"}]}}}'
```

```json
{"jsonrpc":"2.0","id":1,"result":{"task":{
  "artifacts":[{"artifactId":"greeting","parts":[{"text":"Hello, Tom!"}]}],
  "status":{"state":"TASK_STATE_COMPLETED"}}}}
```

The `A2A-Version: 1.0` header is required — the server refuses requests that
omit it with `VERSION_NOT_SUPPORTED`, rather than guessing which spec revision
you meant.

That is the entire agent, from `examples/hello-agent/src/main.rs`:

```rust,ignore
use a2a_protocol_sdk::prelude::*;

struct HelloAgent;

agent_executor!(HelloAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    let who = ctx.message.text().unwrap_or("world");
    emit.artifact("greeting", vec![Part::text(format!("Hello, {who}!"))], None, Some(true))
        .await?;

    emit.status(TaskState::Completed).await?;
    Ok(())
});

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let handler = std::sync::Arc::new(
        RequestHandlerBuilder::new(HelloAgent).build().expect("build handler"),
    );
    serve("127.0.0.1:3000", JsonRpcDispatcher::new(handler)).await
}
```

Three things carry the weight:

- **`agent_executor!`** writes the `AgentExecutor` impl for you. The trait
  returns `Pin<Box<dyn Future>>` for object safety; the macro hides that.
- **`EventEmitter`** caches `task_id` and `context_id` off the context, turning
  each event into a one-liner instead of a struct literal.
- **`ctx.message.text()`** returns the first text part, skipping any file or
  URL parts before it. No `PartContent` match needed for the common case.

`a2a-protocol-sdk` is the example's only dependency — everything above comes
from `prelude::*`.

## Running the Echo Agent

`echo-agent` goes the other direction: it serves all four bindings, drives all
eleven methods over each, and prints the resulting coverage matrix.

```bash
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

The echo executor is the hello agent plus one wrinkle — a deliberate pause, so
a caller can observe a task that is still running (`examples/echo-agent/src/agent.rs`):

```rust,ignore
agent_executor!(EchoExecutor, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    let input_text = ctx.message.text().unwrap_or("<no text>");

    // Without a slow path, every echo task is terminal by the time its id is
    // known, and `SubscribeToTask` could only ever be observed being refused.
    if input_text.starts_with(SLOW_PREFIX) {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    let echo_text = format!("Echo: {input_text}");
    emit.artifact("echo-artifact", vec![Part::text(&echo_text)], None, Some(true))
        .await?;

    emit.status(TaskState::Completed).await?;
    Ok(())
});
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
