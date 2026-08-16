<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# hello-agent

The smallest complete A2A agent: **23 lines of code**, one dependency, one file.

```sh
cargo run -p hello-agent
```

Then, from another terminal:

```sh
curl -X POST localhost:3000/ \
  -H 'content-type: application/json' \
  -H 'A2A-Version: 1.0' \
  -d '{"jsonrpc":"2.0","id":1,"method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"ROLE_USER",
        "parts":[{"text":"Ada"}]}}}'
```

```json
{"jsonrpc":"2.0","id":1,"result":{"task":{
  "artifacts":[{"artifactId":"greeting","parts":[{"text":"Hello, Ada!"}]}],
  "status":{"state":"TASK_STATE_COMPLETED"}}}}
```

Two details the JSON-RPC binding will not let you omit. Both answer with a
structured error rather than misbehaving quietly, and both are easy to get
wrong from memory:

- **`A2A-Version: 1.0`** — without it the server returns `-32009`
  (`VERSION_NOT_SUPPORTED`): an unversioned request is taken to be from a v0.3
  peer.
- **`parts`, not `content`** — that is the field name on the wire. The wrong
  one parses as a message with no parts and is rejected with
  `invalid params: message must contain at least one part`.

## What it demonstrates

```rust
agent_executor!(HelloAgent, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    let who = ctx.message.text().unwrap_or("world");
    let greeting = Part::text(format!("Hello, {who}!"));
    emit.artifact("greeting", vec![greeting], None, Some(true)).await?;
    emit.status(TaskState::Completed).await?;
    Ok(())
});
```

Three things carry the weight:

- **`agent_executor!`** writes the trait implementation, including the
  `Pin<Box<dyn Future>>` signature you would otherwise spell out by hand.
- **`EventEmitter`** turns the event-queue protocol into `status` and `artifact`
  calls.
- **`ctx.message.text()`** pulls the text out of a message without matching on
  `PartContent` and walking a vector.

Everything comes from `a2a_protocol_sdk::prelude::*`. That is enforced rather
than encouraged: this example depends on the umbrella crate alone, so if saying
hello ever needs a second dependency or a fully-qualified path, the prelude has
a gap and this example stops compiling.

## Where to go next

| | |
|---|---|
| Ship this | [`deploy-agent`](../deploy-agent) — config, health checks, graceful shutdown, container, Kubernetes |
| Every binding at once | [`echo-agent`](../echo-agent) — JSON-RPC, REST, WebSocket and gRPC from one handler |
| Production concerns | [`incident-response`](../incident-response) — tenancy, auth, rate limits, persistence, signing, shutdown |

## Tests

```sh
cargo test -p hello-agent
```

The tests start the agent on an ephemeral port and drive it with a real client,
including the empty-message case that exercises the `unwrap_or("world")` path.
