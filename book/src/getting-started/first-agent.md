# Your First Agent

This guide walks you through building a complete A2A agent from scratch — a calculator that evaluates simple arithmetic expressions.

## Project Setup

Create a new binary crate:

```bash
cargo new my-agent
cd my-agent
```

Add dependencies to `Cargo.toml`:

```toml
[dependencies]
a2a-protocol-sdk = "0.16"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
```

## Step 1: Define Your Executor

The `AgentExecutor` trait is the entry point for all agent logic. It defines what your agent does when it receives a message.

The trait itself returns `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>` — that boxing is what keeps it object-safe, so the handler can hold a `dyn AgentExecutor`. You rarely write it by hand: `agent_executor!` generates the whole impl from a plain `async` block. See [The AgentExecutor Trait](../building-agents/executor.md) for the unabridged form and when you need it.

```rust,no_run
use a2a_protocol_sdk::prelude::*;

struct CalcExecutor;

agent_executor!(CalcExecutor, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);

    // Signal that we're working
    emit.status(TaskState::Working).await?;

    // Extract the expression from the message. `text()` returns the first
    // text part, skipping any file or URL parts that precede it.
    let expr = ctx.message.text().unwrap_or_default();

    // Evaluate (very basic: one `a op b` expression with + - * /)
    let result = evaluate(expr);

    // Send the result as an artifact, then the terminal `Completed` status
    emit.complete_with_text(result).await
});

fn evaluate(expr: &str) -> String {
    // Toy parser: "3 + 5", "10 - 2", etc.
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 3 {
        return format!("Error: expected 'a op b', got '{expr}'");
    }
    let a: f64 = match parts[0].parse() {
        Ok(v) => v,
        Err(_) => return format!("Error: invalid number '{}'", parts[0]),
    };
    let b: f64 = match parts[2].parse() {
        Ok(v) => v,
        Err(_) => return format!("Error: invalid number '{}'", parts[2]),
    };
    match parts[1] {
        "+" => format!("{}", a + b),
        "-" => format!("{}", a - b),
        "*" => format!("{}", a * b),
        "/" if b != 0.0 => format!("{}", a / b),
        "/" => "Error: division by zero".into(),
        op => format!("Error: unknown operator '{op}'"),
    }
}
```

## Step 2: Create the Agent Card

The agent card tells clients what your agent can do. `AgentCard::new` takes the
three things the type cannot invent — a name, a version and where to reach the
agent — and defaults both mode lists to `text/plain`; the `with_*` methods add
the rest. The server's builder refuses a card without a description, or without
a skill that has at least one tag (spec §5.7):

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
fn make_agent_card(url: &str) -> AgentCard {
    AgentCard::new("Calculator Agent", "1.0.0", AgentInterface::jsonrpc(url))
        .with_description("Evaluates simple arithmetic expressions")
        .with_skill(
            AgentSkill::new("calc", "Calculator", "Evaluates expressions like '3 + 5'")
                .with_tags(["math", "calculator"])
                .with_examples(["3 + 5", "10 * 2", "100 / 4"]),
        )
        .with_streaming(true)
}
```

## Step 3: Wire Up the Server

Build the request handler and start an HTTP server:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
use std::sync::Arc;
# struct CalcExecutor;
# agent_executor!(CalcExecutor, |_ctx, _queue| async { Ok(()) });
# fn make_agent_card(_url: &str) -> AgentCard { unimplemented!() }

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Build the handler with our executor and agent card
    let handler = Arc::new(
        RequestHandlerBuilder::new(CalcExecutor)
            .with_agent_card(make_agent_card("http://localhost:3000"))
            .build()
            .expect("build handler"),
    );

    println!("Calculator agent listening on http://127.0.0.1:3000");

    // Serves until SIGTERM or Ctrl-C, then lets the tasks in flight end and
    // drains the connections.
    let report = Server::bind("127.0.0.1:3000")
        .await?
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown_signal())
        .await;
    match report.refused {
        Some(why) => Err(std::io::Error::other(why)),
        None => Ok(()),
    }
}
```

> **Note:** `Server` and `shutdown_signal` are re-exported from the prelude.
> `Server::bind` binds a TCP listener; `serve_with_shutdown` runs the accept
> loop until the signal, with a 30-second header-read timeout, a 75-second idle
> timeout, no connection ceiling and no TLS (`with_config` and `with_tls` change
> those). For port `0` in a test, read `server.local_addr()` before serving. The
> free `serve()` and `serve_with_addr()` this page used to show are deprecated
> since 0.16.0: neither can be stopped.

Steps 1 to 3 go in `src/main.rs`, in that order, under the one
`use a2a_protocol_sdk::prelude::*;` that Step 1 opens with. Start the agent:

```bash
cargo run --bin my-agent
```

## Step 4: Test with a Client

Save the client as `src/bin/client.rs`:

```rust,no_run
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::client::ClientBuilder;

#[tokio::main]
async fn main() {
    let client = ClientBuilder::new("http://127.0.0.1:3000".to_string())
        .build()
        .expect("build client");

    let params = MessageSendParams::new(Message::user_text(
        uuid::Uuid::new_v4().to_string(),
        "42 + 58",
    ));

    match client.send_message(params).await.unwrap() {
        SendMessageResponse::Task(task) => {
            println!("Result: {:?}", task.status.state);
            for art in task.artifacts.iter().flatten() {
                // `texts()` yields every text part in order; `text()` would
                // give just the first.
                for text in art.texts() {
                    println!("Answer: {text}");
                }
            }
        }
        other => println!("Unexpected response: {other:?}"),
    }
}
```

Run it in a second terminal while the agent is running:

```bash
cargo run --bin client
```

Output:
```text
Result: Completed
Answer: 100
```

## The Three-Event Pattern

Almost every executor follows this pattern:

1. **Status → Working** — Signal that processing has started
2. **ArtifactUpdate** — Deliver results (one or more artifacts)
3. **Status → Completed** — Signal that processing is done

For streaming clients, these arrive as individual SSE events. For synchronous clients, the handler collects them into a final `Task` response.

`EventEmitter` exists for exactly this pattern: it caches `task_id` and `context_id` off the `RequestContext` so each of the three steps is one line rather than a struct literal of four to six fields, and `complete_with_text` (or `complete_with`, for parts other than text) is steps 2 and 3 together. The steps one at a time are `status`, `text_artifact` (one complete text artifact) and `artifact_chunk` (one chunk of an artifact streamed in several, placed by `ArtifactChunk::first()`, `append()`, `last()` or `whole()`). Build the `StreamResponse` values yourself only when you need a field `EventEmitter` does not expose — per-event `metadata`, say.

## Next Steps

- **[Project Structure](./project-structure.md)** — Understand how the crates fit together
- **[The AgentExecutor Trait](../building-agents/executor.md)** — Advanced executor patterns
- **[Request Handler & Builder](../building-agents/handler.md)** — Configuration options
