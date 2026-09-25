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
  "status":{"state":"TASK_STATE_COMPLETED","timestamp":"2026-...Z"}, ...}}}
```

(Abridged: the task also carries its `id` and `contextId`, among other fields.)

The `A2A-Version: 1.0` header is required — the server refuses requests that
omit it with `VERSION_NOT_SUPPORTED`, rather than guessing which spec revision
you meant.

That is the agent, from `examples/hello-agent/src/main.rs` — shown here without
the agent card it also publishes; [Hello Agent](../examples/hello-agent.md) has
the whole file:

```rust,no_run
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

What it prints, in order (ports are assigned by the OS, so they differ per run):

1. **The endpoints** — `=== A2A Echo Agent — full-surface demo ===`, then one
   line each for JSON-RPC, HTTP+JSON, gRPC and WebSocket, and the local
   webhook sink that push configs point at.
2. **Discovery, once** — `resolve_agent_card()` fetches
   `/.well-known/agent-card.json` and prints the agent's name, version,
   interface count and its streaming, push and extended-card capabilities.
   One card describes every binding.
3. **A sweep per binding** — under `--- JSONRPC ---`, `--- HTTP+JSON ---`,
   `--- GRPC ---` and `--- WEBSOCKET ---`, every A2A method is driven over
   that binding: send and stream, get, list, cancel and subscribe, the four
   push-config methods, and the extended card.
4. **Counter-tests** — `--- counter-tests (calls that must be refused) ---`:
   calls a second agent, advertising no optional capabilities, must refuse
   as the specification requires.
5. **The matrix** — `=== Coverage: every A2A method over every binding ===`,
   one row per method and one column per binding, each cell `ok`, `n/a`
   (with the reason printed beneath the grid) or `MISSING`.

The exit code is the verdict: `0` no cell is missing and every counter-test was
refused, `1` a call or counter-test failed, `2` a matrix cell never ran.

## What Just Happened?

The matrix is computed, not asserted: each call records itself, and the
rows come from `a2a_protocol_types::method::Method::ALL` — the
specification's eleven methods, not a list this example chose. A gap is an
exit code, so the example cannot quietly shrink and still print a
full-looking report.

## The Code in Brief

The echo executor is the hello agent plus one wrinkle — a deliberate pause, so
a caller can observe a task that is still running (`examples/echo-agent/src/agent.rs`):

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# const SLOW_PREFIX: &str = "slow:";
# struct EchoExecutor;
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
