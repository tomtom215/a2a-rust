# Echo Agent

Every A2A method over every binding, from one echo executor, with a coverage
matrix that fails the run if a cell is missing. Read it after the
[Hello Agent](./hello-agent.md), which is the smaller starting point. It is
also the server that the in-repo TCK and the [Go interop check](./harness.md)
drive.

**Source:** [`examples/echo-agent/`](https://github.com/tomtom215/a2a-rust/tree/main/examples/echo-agent)

## What it shows

| Feature | How |
|---------|-----|
| **AgentExecutor** | Implements the trait with Working → Artifact → Completed lifecycle |
| **Four bindings, one handler** | `JsonRpcDispatcher`, `RestDispatcher`, `GrpcDispatcher` and `WebSocketDispatcher` all serve the same `RequestHandler` |
| **Clients per binding** | `ClientBuilder` for JSON-RPC, `with_protocol_binding("HTTP+JSON")` for HTTP+JSON, and `with_custom_transport` with `GrpcTransport` / `WebSocketTransport` |
| **Every method** | The eleven A2A methods, driven over each binding |
| **Push notifications** | The card declares them, and a webhook sink is started so the push configs point at a URL that answers |
| **Agent card discovery** | `resolve_agent_card()` fetches `/.well-known/agent-card.json` at runtime |
| **Counter-tests** | A second agent advertising no optional capabilities, whose five refusals are checked |
| **Pre-bind pattern** | Listeners are bound before the handler is built so agent card URLs are correct |

## Run it

No services, keys or other languages: everything runs in-process.

```bash
cargo run -p echo-agent

# With structured logging:
RUST_LOG=debug cargo run -p echo-agent --features tracing
```

Exit codes: `0` every cell ran and every counter-test was refused, `1` a call
or counter-test failed, `2` the matrix has a gap. The matrix rows come from
the ratified `a2a.proto` (see `a2a_protocol_types::method`), not from this
example.

## What you'll see

From `cargo run -p echo-agent` on 2026-10-15, as recorded in the example's
README. Ports and ids are the ones that run was assigned; the HTTP+JSON, gRPC
and WebSocket blocks, which have the same eleven lines as the JSON-RPC one,
are left out:

```text
=== A2A Echo Agent — full-surface demo ===

JSON-RPC  http://127.0.0.1:38669
HTTP+JSON http://127.0.0.1:41789
gRPC      127.0.0.1:38293
WebSocket ws://127.0.0.1:35907
webhook   http://127.0.0.1:36525/webhook

Discovered 'Echo Agent' v1.0.0 — 4 interface(s), streaming=Some(true) push=Some(true) extended=Some(true)

--- JSONRPC ---
  [ok]   SendMessage                        task c0f6fe8c-183f-4512-b902-a6f901fb90f0
  [ok]   SendStreamingMessage               4 event(s), 0 lag signal(s)
  [ok]   GetTask                            state Completed
  [ok]   ListTasks                          2 task(s)
  [ok]   CancelTask                         refused as expected: protocol error: [-32002] Task cannot be canceled: f29ccb61-bf1a-4358-949e-d4b02a5c2252
  [ok]   SubscribeToTask                    3 event(s)
  [ok]   CreateTaskPushNotificationConfig   config Some("8242a563-d272-45c9-af05-cdc721bf2010")
  [ok]   ListTaskPushNotificationConfigs    1 config(s)
  [ok]   GetTaskPushNotificationConfig      url http://127.0.0.1:36525/webhook
  [ok]   DeleteTaskPushNotificationConfig   deleted
  [ok]   GetExtendedAgentCard               card 'Echo Agent'

--- counter-tests (calls that must be refused) ---
  [ok]   GetTask on an unknown id                     refused: TaskNotFound
  [ok]   CancelTask on an unknown id                  refused: TaskNotFound
  [ok]   push config against a card without pushNotifications refused: PushNotificationNotSupported
  [ok]   extended card against a card without extendedAgentCard refused: UnsupportedOperation
  [ok]   streaming against a card without streaming   refused: protocol error: [-32004] agent does not support streaming (AgentCard.capabilities.streaming is not true)

=== Coverage: every A2A method over every binding ===

METHOD                             JSONRPC    HTTP+JSON     GRPC      WEBSOCKET
SendMessage                          ok          ok          ok          ok
SendStreamingMessage                 ok          ok          ok          ok
GetTask                              ok          ok          ok          ok
ListTasks                            ok          ok          ok          ok
CancelTask                           ok          ok          ok          ok
SubscribeToTask                      ok          ok          ok          ok
CreateTaskPushNotificationConfig     ok          ok          ok          ok
GetTaskPushNotificationConfig        ok          ok          ok          ok
ListTaskPushNotificationConfigs      ok          ok          ok          ok
DeleteTaskPushNotificationConfig     ok          ok          ok          ok
GetExtendedAgentCard                 ok          ok          ok          ok

44 exercised, 0 not applicable, 0 missing, of 44 cells
Rows: the 11 methods `service A2AService` declares in the ratified proto (see a2a_protocol_types::method).
Columns: 3 spec-named bindings (§9 §10 §11) plus WEBSOCKET, a §12 custom binding this SDK adds.

Every A2A method was exercised over every binding, and every
counter-test was refused as the specification requires.
```

`CancelTask` is refused because the echo task has already completed when the
cancel arrives. That refusal is the answer §3.1.5 requires for a task in a
terminal state, and the example counts it as exercised. The matrix and the
counter-tests come from the shared [example harness](./harness.md).

## Key code

The executor is about twenty lines: extract the text, emit events.

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

The rest of `src/main.rs` is wiring, in this order: bind a listener per
binding, build the card from the bound addresses, build one handler, start the
four dispatchers over it, then build one client per binding and sweep it. The
handler gets a push-config store, an `HttpPushSender` and an extended card
because the card declares all three, and the server refuses the four
push-config methods and `GetExtendedAgentCard` unless the card declares them
(§3.1.11).

## Server-only mode

Set `A2A_BIND_ADDR` to run as a standalone server instead, until it is killed:

```bash
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p echo-agent
```

It serves JSON-RPC and HTTP+JSON on that one address, the shape this
repository's TCK (`tck/`, `cargo run -p a2a-tck`) drives. Optional:

| Variable | Effect |
|---|---|
| `A2A_GRPC_BIND_ADDR` | Also serve gRPC on that `host:port`, and advertise it on the card |
| `A2A_WS_BIND_ADDR` | Also serve WebSocket there, and advertise it on the card |
| `A2A_INTEROP_CARD=1` | Advertise (not enforce) API-key security requirements on the card, for `scripts/go_sdk_interop.sh` |

The card names only the bindings actually listening. This mode permits push
webhooks on loopback and private addresses, so it is a test server, not a
deployment template. `genai-agent` names the gRPC and WebSocket variables
`A2A_GRPC_ADDR` and `A2A_WS_ADDR`.

## What it does not cover

No model, no authentication (the extended card is served unauthenticated by
an explicit `allow_unauthenticated_extended_card()`), no persistence and no
multi-turn tasks. For those:

- [Incident-Response Team](./incident-response.md): multi-turn
  `INPUT_REQUIRED`, delegation and cancellation, then the same matrix and
  sixteen production-hardening checks.
- [Deploy Agent](./deploy-agent.md): configuration, health checks and
  graceful shutdown for a container.
- [Example Harness and Go Interop](./harness.md): how the matrix is scored,
  and this agent as the server an a2a-go client drives.
