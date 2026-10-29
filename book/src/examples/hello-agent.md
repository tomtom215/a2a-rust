# Hello Agent

The smallest complete A2A agent: one screen of code above its tests, one
dependency, no feature flags. Start here if you are new to A2A.

**Source:** [`examples/hello-agent/src/main.rs`](https://github.com/tomtom215/a2a-rust/blob/main/examples/hello-agent/src/main.rs)

## What it shows

An executor, an agent card and a JSON-RPC listener in one file, using nothing
but `a2a_protocol_sdk::prelude::*`. The agent greets whoever sends it a
message, and shuts down cleanly on `SIGTERM` or Ctrl-C.

## Run it

Nothing to install beyond Rust: no model, no keys, no services.

```bash
cargo run -p hello-agent
```

It listens on `127.0.0.1:3000`. From another terminal:

```bash
curl -X POST http://127.0.0.1:3000 \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"ROLE_USER","parts":[{"text":"Tom"}]}}}'
```

Or fetch its card with `curl http://127.0.0.1:3000/.well-known/agent-card.json`,
or with the `a2a` CLI: `cargo run -p a2a-cli -- card http://127.0.0.1:3000`.

## What you'll see

The agent prints `hello-agent listening on http://127.0.0.1:3000`. The `curl`
call answers with a completed task carrying a `greeting` artifact (abridged:
ids, timestamps and the history are cut):

```json
{"jsonrpc":"2.0","id":1,"result":{"task":{
  "artifacts":[{"artifactId":"greeting","parts":[{"text":"Hello, Tom!"}]}],
  "contextId":"6cac14dc-...","id":"82c82725-...",
  "status":{"state":"TASK_STATE_COMPLETED","timestamp":"2026-...Z"}, ...}}}
```

Two details the JSON-RPC binding will not let you omit, and both answer with a
structured error rather than misbehaving quietly:

- **`A2A-Version: 1.0`.** It is required by default, as a header or as an
  `A2A-Version` query parameter. Without it, or with any other major.minor
  than `1.0`, the server answers `VERSION_NOT_SUPPORTED` (`-32009`) rather
  than guessing which revision you meant: a client pinned to an older spec
  gets a clear error instead of subtly wrong parsing.
  `DispatchConfig::accept_missing_version_header()` lifts the requirement for
  trusted callers.
- **`parts`, not `content`.** That is the field name on the wire. The wrong
  one parses as a message with no parts and is rejected with
  `invalid params: message must contain at least one part`.

## Key code

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

/// The card a client discovers at `/.well-known/agent-card.json`.
fn card(url: &str) -> AgentCard {
    AgentCard::new("hello-agent", "0.0.0", AgentInterface::jsonrpc(url))
        .with_description("Greets whoever sends it a message")
        // Spec §5.7: a card publishes at least one skill, each with a tag.
        .with_skill(
            AgentSkill::new("hello", "Hello", "Greets whoever sends it a message")
                .with_tags(["greeting"]),
        )
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let url = "http://127.0.0.1:3000";
    let handler = std::sync::Arc::new(
        RequestHandlerBuilder::new(HelloAgent)
            .with_agent_card(card(url))
            .build()
            .expect("build handler"),
    );

    let server = Server::bind("127.0.0.1:3000").await?;
    println!("hello-agent listening on {url}");
    // Returns on SIGTERM or Ctrl-C, once in-flight work has ended.
    server
        .serve_with_shutdown(JsonRpcDispatcher::new(handler), shutdown_signal())
        .await;
    Ok(())
}
```

The file's `main` also starts the SDK's `Telemetry` from the `OTEL_*`
environment (a `Telemetry::builder()` chain and `install_subscriber()`, left
out above): with `OTEL_EXPORTER_OTLP_ENDPOINT` set, traces, metrics and logs go
there; without it nothing is built, and a line on standard error says so
(`OTEL_TRACES_EXPORTER=console` prints spans instead). That is why the server
stops on a signal rather than running forever: `serve(..)` never returns, so
a signal would end the process before `Telemetry` flushed. The file's comments
and its longer `expect` messages are left out too.

## How it fits together

| Piece | Why it is there |
|---|---|
| `agent_executor!` | Writes the `AgentExecutor` impl. The trait returns `Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>` so it stays object-safe; the macro hides that. |
| `EventEmitter` | Caches `task_id` and `context_id` off the `RequestContext`, so each event is one line rather than a struct literal of four to six fields. |
| `ctx.message.text()` | The first text part, or `None`. Non-text parts are skipped, not treated as the end of the search — a message that leads with a file attachment still yields its text. |
| `Server::bind` and `serve_with_shutdown` | Bind the listener, then accept until the shutdown future resolves: stop accepting, end the tasks in flight, drain connections. `shutdown_signal()` resolves on `SIGTERM` or Ctrl-C. |
| `RequestHandlerBuilder` | Everything the agent does not configure gets a default: an in-memory task store, no auth, no push notifications. |
| `AgentCard::new` | The card, from the three things the type cannot invent — name, version, one interface. The other twelve fields default, and `with_agent_card` makes the dispatcher serve it at the well-known path, so `cargo run -p a2a-cli -- card http://127.0.0.1:3000` (the `a2a` CLI) and `ClientBuilder::from_card` work against this agent. |

Two rules keep it small:

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

## Tests

Four tests live under `#[cfg(test)]` in the same file. Each boots the agent on
an ephemeral port and drives it through a real `A2aClient`:

- `greets_the_caller_by_the_text_they_sent` — the positive control. Without it,
  an agent hardcoded to emit `"Hello, world!"` would pass
  `greets_world_when_there_is_no_text`.
- `greets_world_when_there_is_no_text` — a message of only file parts still
  gets a greeting rather than an error.
- `finds_text_that_follows_a_file_part` — a leading non-text part must not hide
  the text behind it. This is the seam `Message::text()` exists to get right,
  exercised through the real server rather than against the type alone.
- `publishes_a_card_a_client_can_discover_and_connect_with` — `resolve_agent_card`
  gets the card from the well-known path, `ClientBuilder::from_card` builds a
  client from it, and that client is greeted — so the interface the card
  advertises is the one the agent is listening on.

```bash
cargo test -p hello-agent
```

## What it does not cover

No streaming, no push notifications, no auth, no persistence, and one
binding out of four. Those are the subject of the other examples:

- [Deploy Agent](./deploy-agent.md) — the same agent made shippable:
  configuration, health checks, graceful shutdown, a container and a
  Kubernetes manifest.
- [Echo Agent](./echo-agent.md) — every method over every binding, with a
  coverage matrix that fails the run if a cell is missing.
- [Incident-Response Team](./incident-response.md) — multi-turn
  `INPUT_REQUIRED`, delegation, cancellation.
- [Agent Team](./agent-team.md) — the full dogfood suite.
