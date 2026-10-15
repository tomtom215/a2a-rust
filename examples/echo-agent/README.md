<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Echo Agent — every A2A method over every binding

An agent that echoes each message back as an artifact, built with the
[a2a-rust](https://github.com/tomtom215/a2a-rust) SDK and used to measure the
SDK's protocol surface against the [A2A protocol](https://github.com/a2aproject/A2A).
It serves all four bindings behind one handler, drives each of the eleven A2A
methods over each binding, prints the coverage matrix, then runs counter-tests
against a second agent that declares no optional capabilities.

## What it demonstrates

| Feature | How |
|---------|-----|
| **One handler, four bindings** | JSON-RPC (§9), HTTP+JSON (§11), gRPC (§10) and WebSocket (a custom binding, §12) dispatchers over one `RequestHandler` |
| **Every method** | Send, streaming send, get, list, cancel, subscribe, the four push-config methods and the extended card, over each binding |
| **Push notifications** | The card declares them, and a webhook sink is started so the configs point at a real URL |
| **Refusals the spec requires** | Five counter-tests against an agent with no push, no streaming and no extended card |
| **Agent card discovery** | `resolve_agent_card()` fetches `/.well-known/agent-card.json` at runtime |
| **Pre-bind pattern** | Listeners are bound before the handler is built, so the card's URLs are the real ones |

## Running

```bash
# From the workspace root:
cargo run -p echo-agent

# With structured logging:
RUST_LOG=debug cargo run -p echo-agent --features tracing
```

Exit codes: `0` every cell ran and every counter-test was refused, `1` a call
or counter-test failed, `2` a matrix cell was never exercised. The matrix rows
come from the ratified `a2a.proto` (see `a2a_protocol_types::method`), not
from this example.

## Expected output

From `cargo run -p echo-agent` on 2026-10-15, with the ports and ids as the
run assigned them, and the HTTP+JSON, gRPC and WebSocket blocks, which have
the same eleven lines as the JSON-RPC one, left out:

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
cancel arrives; the refusal is the answer §3.1.5 requires for a task in a
terminal state, and the example counts it as exercised.

## Server-only mode (in-repo TCK and CI)

Set `A2A_BIND_ADDR` to run as a standalone server instead, until it is
killed:

```bash
A2A_BIND_ADDR=127.0.0.1:8080 cargo run -p echo-agent
```

It serves JSON-RPC and HTTP+JSON on that one address, the shape this
repository's TCK (`tck/`, `cargo run -p a2a-tck`) drives. Optional:

| Variable | Effect |
|---|---|
| `A2A_GRPC_BIND_ADDR` | Also serve gRPC there, and advertise it on the card |
| `A2A_WS_BIND_ADDR` | Also serve WebSocket there, and advertise it on the card |
| `A2A_INTEROP_CARD=1` | Advertise (not enforce) API-key security requirements on the card, for `scripts/go_sdk_interop.sh` |

The card names only the bindings actually listening. This mode permits push
webhooks on loopback and private addresses, so it is a test server, not a
deployment template. `genai-agent` names the gRPC and WebSocket variables
`A2A_GRPC_ADDR` and `A2A_WS_ADDR`.

## Prerequisites

- Rust 1.88+ (MSRV)
- No external services required — everything runs in-process

## License

Apache-2.0
