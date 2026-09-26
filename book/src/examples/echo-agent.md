# Echo Agent

Every A2A method over every binding, from one echo executor, with a coverage
matrix that fails the run if a cell is missing. Read it after the
[Hello Agent](./hello-agent.md), which is the smaller starting point.

**Source:** [`examples/echo-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/echo-agent)

## What it demonstrates

| Feature | How |
|---------|-----|
| **AgentExecutor** | Implements the trait with Working → Artifact → Completed lifecycle |
| **Four bindings, one handler** | `JsonRpcDispatcher`, `RestDispatcher`, `GrpcDispatcher` and `WebSocketDispatcher` all serve the same `RequestHandler` |
| **Clients per binding** | `ClientBuilder` for JSON-RPC, `with_protocol_binding("HTTP+JSON")` for HTTP+JSON, and `with_custom_transport` with `GrpcTransport` / `WebSocketTransport` |
| **Every method** | The eleven A2A methods, driven over each binding |
| **Agent card discovery** | `resolve_agent_card()` fetches `/.well-known/agent-card.json` at runtime |
| **Counter-tests** | A second agent advertising no optional capabilities, whose refusals are checked |
| **Pre-bind pattern** | Listeners are bound before the handler is built so agent card URLs are correct |

## Running

```bash
cargo run -p echo-agent

# With structured logging:
RUST_LOG=debug cargo run -p echo-agent --features tracing
```

## Demo walkthrough

The run has four parts, in order:

1. **Discovery** — the card is fetched once; it describes every binding.
2. **A sweep per binding** — JSON-RPC, HTTP+JSON, gRPC and WebSocket in turn,
   each driving every A2A method, push configs pointed at a local webhook sink.
3. **Counter-tests** — calls a second, capability-less agent must refuse.
4. **The coverage matrix** — one row per method, one column per binding.

Exit codes: `0` complete, `1` a call or counter-test failed, `2` the matrix
has a gap.

## Server-only mode

Set `A2A_BIND_ADDR` to run as a standalone server (used by the TCK):

```bash
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p echo-agent
```

## Key code

The executor is ~20 lines — extract text, emit events:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# const SLOW_PREFIX: &str = "slow:";
struct EchoExecutor;

agent_executor!(EchoExecutor, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    let input_text = ctx.message.text().unwrap_or("<no text>");

    // A deliberate pause, so a caller can observe a task that is still
    // running — otherwise `SubscribeToTask` could only ever be seen refused.
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
