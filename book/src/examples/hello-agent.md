# Hello Agent

The smallest complete A2A agent: 35 lines, one dependency, no feature flags.

```bash
cargo run -p hello-agent
```

Source: [`examples/hello-agent/src/main.rs`](https://github.com/tomtom215/a2a-rust/blob/main/examples/hello-agent/src/main.rs).

## The whole thing

```rust,no_run
use a2a_protocol_sdk::prelude::*;

/// The agent. It has no state — the greeting depends only on the message.
struct HelloAgent;

agent_executor!(HelloAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;

    let who = ctx.message.text().unwrap_or("world");
    let greeting = Part::text(format!("Hello, {who}!"));

    emit.artifact("greeting", vec![greeting], None, Some(true)).await?;

    emit.status(TaskState::Completed).await?;
    Ok(())
});

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let handler = std::sync::Arc::new(
        RequestHandlerBuilder::new(HelloAgent).build().expect("build handler"),
    );

    println!("hello-agent listening on http://127.0.0.1:3000");
    serve("127.0.0.1:3000", JsonRpcDispatcher::new(handler)).await
}
```

## Talking to it

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
  "contextId":"6cac14dc-...","id":"82c82725-...",
  "status":{"state":"TASK_STATE_COMPLETED"}}}}
```

`A2A-Version: 1.0` is required. Without it the server answers
`VERSION_NOT_SUPPORTED` (`-32009`) rather than guessing which revision you
meant — a client pinned to an older spec gets a clear error instead of subtly
wrong parsing.

## What each piece does

| Piece | Why it is there |
|---|---|
| `agent_executor!` | Writes the `AgentExecutor` impl. The trait returns `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>` so it stays object-safe; the macro hides that. |
| `EventEmitter` | Caches `task_id` and `context_id` off the `RequestContext`, so each event is one line rather than a seven-field struct literal. |
| `ctx.message.text()` | The first text part, or `None`. Non-text parts are skipped, not treated as the end of the search — a message that leads with a file attachment still yields its text. |
| `serve` | Binds a listener and runs the accept loop, replacing roughly 25 lines of hyper wiring. |
| `RequestHandlerBuilder` | Everything the agent does not configure gets a default: an in-memory task store, no auth, no push notifications. |

## Two rules keep it small

1. **One dependency** — `a2a-protocol-sdk` and nothing else (plus `tokio` for a
   runtime). If saying hello ever needs a second crate or a fully-qualified
   path, that is a gap in the prelude worth closing rather than working around
   here. This is not decoration: it is why the example exists as a workspace
   member. The README Quick Start tells you to depend on exactly these two
   crates, so `cargo build -p hello-agent` fails if that instruction stops
   being true.

2. **No feature flags** — what you read is what `cargo run -p hello-agent`
   runs. The sibling `echo-agent` makes its transports hard dependencies for
   the same reason: a feature-gated demo can quietly shrink and still print a
   full-looking report.

## What it deliberately leaves out

No agent card, no streaming, no push notifications, no auth, no persistence,
and one binding out of four. Those are the subject of the other examples:

- [Echo Agent](./echo-agent.md) — every method over every binding, with a
  coverage matrix that fails the run if a cell is missing.
- [Incident-Response Team](./incident-response.md) — multi-turn
  `INPUT_REQUIRED`, delegation, cancellation.
- [Agent Team](./agent-team.md) — the full dogfood suite.

## Tests

Three tests live under `#[cfg(test)]` in the same file. Each boots the agent on
an ephemeral port and drives it through a real `A2aClient`:

- `greets_the_caller_by_the_text_they_sent` — the positive control. Without it,
  an agent hardcoded to emit `"Hello, world!"` would pass the other two.
- `greets_world_when_there_is_no_text` — a message of only file parts still
  gets a greeting rather than an error.
- `finds_text_that_follows_a_file_part` — a leading non-text part must not hide
  the text behind it. This is the seam `Message::text()` exists to get right,
  exercised through the real server rather than against the type alone.

```bash
cargo test -p hello-agent
```
