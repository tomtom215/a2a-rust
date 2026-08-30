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
a2a-protocol-sdk = "0.11"
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

    // Evaluate (very basic: just handle "a + b")
    let result = evaluate(expr);

    // Send the result as an artifact
    emit.artifact("result", vec![Part::text(&result)], None, Some(true))
        .await?;

    // Done
    emit.status(TaskState::Completed).await?;
    Ok(())
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

The agent card tells clients what your agent can do:

```rust,no_run
use a2a_protocol_sdk::types::agent_card::*;

fn make_agent_card(url: &str) -> AgentCard {
    AgentCard {
        url: None,
        name: "Calculator Agent".into(),
        description: "Evaluates simple arithmetic expressions".into(),
        version: "1.0.0".into(),
        supported_interfaces: vec![AgentInterface {
            url: url.into(),
            protocol_binding: "JSONRPC".into(),
            protocol_version: "1.0.0".into(),
            tenant: None,
        }],
        default_input_modes: vec!["text/plain".into()],
        default_output_modes: vec!["text/plain".into()],
        skills: vec![AgentSkill {
            id: "calc".into(),
            name: "Calculator".into(),
            description: "Evaluates expressions like '3 + 5'".into(),
            tags: vec!["math".into(), "calculator".into()],
            examples: Some(vec![
                "3 + 5".into(),
                "10 * 2".into(),
                "100 / 4".into(),
            ]),
            input_modes: None,
            output_modes: None,
            security_requirements: None,
        }],
        capabilities: AgentCapabilities::none()
            .with_streaming(true)
            .with_push_notifications(false),
        provider: None,
        icon_url: None,
        documentation_url: None,
        security_schemes: None,
        security_requirements: None,
        signatures: None,
    }
}
```

## Step 3: Wire Up the Server

Build the request handler and start an HTTP server:

```rust,no_run
use a2a_protocol_sdk::prelude::*;
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

    // One-liner server startup (replaces ~25 lines of hyper boilerplate)
    serve("127.0.0.1:3000", JsonRpcDispatcher::new(handler)).await
}
```

> **Note:** `serve()` is re-exported from the prelude. It binds a TCP listener
> and runs the accept loop internally. For production use cases needing the bound
> address (e.g. port `0`), use `serve_with_addr()` instead.

## Step 4: Test with a Client

In a separate terminal (or in the same binary), create a client:

```rust,no_run
use a2a_protocol_sdk::prelude::*;
use a2a_protocol_sdk::client::ClientBuilder;

#[tokio::main]
async fn main() {
    let client = ClientBuilder::new("http://127.0.0.1:3000".to_string())
        .build()
        .expect("build client");

    let params = MessageSendParams {
        tenant: None,
        message: Message {
            id: MessageId::new(uuid::Uuid::new_v4().to_string()),
            role: MessageRole::User,
            parts: vec![Part::text("42 + 58")],
            task_id: None,
            context_id: None,
            reference_task_ids: None,
            extensions: None,
            metadata: None,
        },
        configuration: None,
        metadata: None,
    };

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

`EventEmitter` exists for exactly this pattern: it caches `task_id` and `context_id` off the `RequestContext` so each of the three steps is one line rather than a seven-field struct literal. Build the `StreamResponse` values yourself only when you need a field `EventEmitter` does not expose — per-event `metadata`, say.

## Next Steps

- **[Project Structure](./project-structure.md)** — Understand how the crates fit together
- **[The AgentExecutor Trait](../building-agents/executor.md)** — Advanced executor patterns
- **[Request Handler & Builder](../building-agents/handler.md)** — Configuration options
