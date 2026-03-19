# Echo Agent

The simplest possible A2A agent — echoes incoming messages back as artifacts. This is the best starting point for understanding the SDK.

**Source:** [`examples/echo-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/echo-agent)

## What it demonstrates

| Feature | How |
|---------|-----|
| **AgentExecutor** | Implements the trait with Working → Artifact → Completed lifecycle |
| **JSON-RPC transport** | Server via `JsonRpcDispatcher`, client via `ClientBuilder` |
| **REST transport** | Server via `RestDispatcher`, client via `ClientBuilder::with_protocol_binding("REST")` |
| **Streaming (SSE)** | `stream_message()` consumes `StatusUpdate` and `ArtifactUpdate` events |
| **Agent card discovery** | `resolve_agent_card()` fetches `/.well-known/agent.json` at runtime |
| **Task retrieval** | `get_task()` retrieves a completed task by ID |
| **Pre-bind pattern** | Listeners are bound before the handler is built so agent card URLs are correct |

## Running

```bash
cargo run -p echo-agent

# With structured logging:
RUST_LOG=debug cargo run -p echo-agent --features tracing
```

## Demo walkthrough

The example runs 6 demos in sequence:

1. **Synchronous SendMessage (JSON-RPC)** — sends a message, waits for the complete task
2. **Streaming SendMessage (JSON-RPC)** — receives real-time SSE events as the agent works
3. **Synchronous SendMessage (REST)** — same operation over the REST transport
4. **Streaming SendMessage (REST)** — SSE streaming over REST
5. **Agent Card Discovery** — fetches `/.well-known/agent.json` and inspects capabilities
6. **GetTask** — retrieves the task created in Demo 1 by its ID

## Server-only mode

Set `A2A_BIND_ADDR` to run as a standalone server (used by the TCK):

```bash
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p echo-agent
```

## Key code

The executor is ~30 lines — extract text, emit events:

```rust
impl AgentExecutor for EchoExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            queue.write(StatusUpdate { state: Working }).await?;

            let input = extract_text(&ctx.message.parts);
            queue.write(ArtifactUpdate {
                artifact: Artifact::new("echo", vec![Part::text(&format!("Echo: {input}"))]),
                ..
            }).await?;

            queue.write(StatusUpdate { state: Completed }).await?;
            Ok(())
        })
    }
}
```
