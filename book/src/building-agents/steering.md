<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Steering a Running Task

A task that is still working can be told something: use the cheaper index,
stop at a hundred rows, the customer changed their mind. A2A defines a
follow-up message for a task that has stopped to ask for input
(`input-required`, `auth-required`) and says nothing about one sent while the
task works, so without more a running task can only be cancelled and started
again, losing what it has done.

This SDK adds that as an extension, `https://a2a-rust.com/extensions/steering/v1`
(ADR 0036), off by default. A `SendMessage` that names a running task and lists
the extension is not refused: it is put in the **inbox** of the turn that runs
the task, where the executor reads it when it can act on it. A peer that does
not list the extension gets the refusal it always got.

## Turning it on

```rust
use a2a_protocol_server::{RequestHandlerBuilder, SteeringConfig, agent_executor};
# struct Researcher;
# agent_executor!(Researcher, |_ctx, _queue| async { Ok(()) });

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let handler = RequestHandlerBuilder::new(Researcher)
    .without_agent_card()
    .with_steering(
        SteeringConfig::new()
            .with_inbox_capacity(16)          // messages waiting per running turn (the default)
            .with_max_message_size(64 * 1024), // bytes of the message's JSON (the default)
    )
    .build()?;
# drop(handler);
# Ok(())
# }
```

With a card, the card declares the extension exactly when steering is on, so a
client can read it there. A signed card must already declare it: the server
refuses to edit a card it would have to re-sign.

## Reading the inbox in an executor

`RequestContext::inbox()` is `Some` when steering is on. An executor reads it
where it can change what it does next: between steps of a loop, or beside the
work it is waiting for. One that never reads it is unaffected; the messages are
dropped when the turn ends and counted as `unread`.

```rust
use a2a_protocol_server::{EventEmitter, agent_executor};
use a2a_protocol_types::task::TaskState;

struct Researcher;

agent_executor!(Researcher, |ctx, queue| async {
    let emit = EventEmitter::new(ctx, queue);
    emit.status(TaskState::Working).await?;
    let mut instructions = vec![ctx.message.text().unwrap_or_default().to_owned()];
    for _step in 0..10 {
        // Between steps: take what was said meanwhile, oldest first.
        if let Some(inbox) = ctx.inbox() {
            while let Some(steered) = inbox.try_recv() {
                instructions.extend(steered.message.texts().map(str::to_owned));
            }
        }
        if ctx.cancellation_token.is_cancelled() {
            return Ok(());
        }
        // ... one step of the work, shaped by `instructions` ...
    }
    emit.status(TaskState::Completed).await
});
```

`inbox.recv().await` waits for the next message; it is safe to cancel, and
returns `None` once the turn has ended. Each `Steered` has the `message` and
`from`, the caller's authenticated identity when there is one. The text is the
sender's, with the sender's authority over the task and no more (see
*Security*).

The tool loop reads it for you when asked: `ToolLoopExecutor::with_steering()`
adds each message's text to the model's conversation as a user turn before the
next model turn, after the tool results the last turn produced. A message that
arrives during the turn that answers is not read; the run is over.

```rust,no_run
use a2a_protocol_server::tool_loop::{OpenAiChatModel, ToolLoop, ToolLoopExecutor, ToolSet};
use a2a_protocol_server::{RequestHandlerBuilder, SteeringConfig};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let model = OpenAiChatModel::new("http://127.0.0.1:8080/v1", "qwen3:1.7b")?;
let executor = ToolLoopExecutor::new(ToolLoop::new(model, ToolSet::new())).with_steering();
let handler = RequestHandlerBuilder::new(executor)
    .without_agent_card()
    .with_steering(SteeringConfig::new())
    .build()?;
# drop(handler);
# Ok(())
# }
```

## Steering a child from a client

A parent that delegated work ([Delegating to Another Agent](../client/delegation.md))
steers its child with the delegation:

```rust,no_run
use std::sync::Arc;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::delegation::{Delegation, Steering, steer_all};
use a2a_protocol_types::{Message, MessageSendParams};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let worker = Arc::new(ClientBuilder::new("http://worker:8080").build()?);
let mut child = Delegation::start(
    Arc::clone(&worker),
    MessageSendParams::new(Message::user_text("m-1", "survey the market")),
)
.await?;
// The child is named by its first event.
while child.task_id().is_none() {
    child.next_event().await;
}

match child.steer(Message::user_text("m-2", "skip the retail segment")).await? {
    Steering::Delivered { delivery, .. } => println!("delivered: {delivery:?}"),
    Steering::NotSteered(answer) => println!("not running; answered {answer:?}"),
    _ => {}
}

// Several children at once: one outcome each, in order, none stopping another.
let children = [child];
for outcome in steer_all(&children, || Message::user_text("m-3", "wrap up")).await {
    println!("{:?}: {}", outcome.task_id, outcome.result.is_ok());
}
# Ok(())
# }
```

`steer` names the child's task and context, lists the extension, and gives the
message an idempotency key when the agent is known to honour one, so a client
with a retry policy can retry a delivery whose answer was lost without
delivering it twice. `Delegation::steer_handle` returns an owned handle for
steering from one task while another waits on the delegation. A child that no
event has named yet is not steered and nothing is sent.

## On the wire

A steering message is an ordinary `SendMessage`; nothing else marks it:

```json
{
  "message": {
    "messageId": "m-2",
    "role": "ROLE_USER",
    "taskId": "t-1",
    "parts": [{ "text": "skip the retail segment" }],
    "extensions": ["https://a2a-rust.com/extensions/steering/v1"]
  }
}
```

The response is the task as it stands, with a receipt under
`metadata["a2a-rust.com/steering"]`:

| `delivery` | Means |
|---|---|
| `inbox` | In the inbox of the turn running on the replica that answered |
| `handed-off` | Stored for the replica that holds the turn's claim, which moves it into the inbox at its next look (every claim renewal interval) or at once when notifications are on |
| `replayed` | Not delivered again: the message's idempotency key was held by this same message |

It says where the message went, never that the executor read or acted on it. A
response with no receipt was not a delivery: the task was not running, and the
message was handled as any other message to it. A task parked at
`input-required` or `auth-required` is **continued** by the message, as the
specification says; a finished task refuses it.

A task counts as running when its state is `working`, or `submitted` (the
executor starts when the task is created and the stored state catches up with
its first write) and a turn runs it.

| Refusal | Status | When |
|---|---|---|
| Inbox full | `503` with `Retry-After` (JSON-RPC: internal error with `RetryInfo`) | The turn's inbox, or the store's queue for another replica's turn, holds its capacity. Nothing is queued; the call does not wait |
| Turn ended | `503` with `Retry-After` | The executor returned, or the claim lapsed, before delivery. Nothing was delivered: retry, and see what the task became |
| Too large | `413` | The message's JSON is over the configured size |
| Halted | `503` | The job, tenant or server is halted (as for any send) |
| Not allowed | `403`/`404` | An interceptor refused the named task, or another caller's or tenant's task: not found |

## Several replicas

The turn runs on the replica that holds its claim
([Continuations and cancellation](../deployment/horizontal-scaling/turn-claims.md)).
A steering message that reaches another replica is stored for that claim, in
the task store's database, in the same atomic step that checks the claim is
live, and the replica that holds it moves it into the turn's inbox.

- **When.** At that replica's next look at the store, every claim renewal
  interval (one second by default), which delivers every message. On
  PostgreSQL, `SteeringConfig::with_notifications(true)` also announces it with
  `NOTIFY`, so it arrives at once; leave it off behind a connection pooler in
  transaction mode, where a `LISTEN` session breaks other clients' statements.
- **A full inbox delays, never loses.** The message waits in the store until
  the inbox has room, in order. The store's queue is bounded by the same
  capacity, so a task holds at most twice its inbox's capacity, and the next
  message is refused with `503`.
- **A lapsed claim refuses, never loses.** A task whose claim is gone (its
  replica died; its turn ended) takes no message: the sender is told, and the
  message was not stored. A message stored for a claim that ends before it is
  read is deleted with the claim.
- **A replica that has just died is a window.** Its claim stays live until the
  claim's time to live runs out (15 s by default), and the claim is the only
  evidence that the turn is running. A message handed off in that window is
  answered `handed-off` and nothing reads it. `handed-off` means *stored for
  the replica that holds the claim*, not *read*. A sender that needs to know
  reads the task afterwards and sends again once it is refused.
- **Needs a claim store that can.** The SQL stores and `InMemoryTurnClaims`
  hand messages over. A claim store of your own that does not override
  `hand_steering`, `peek_steering` and `ack_steering` (all three, or none)
  refuses with a message that says why.
- **The in-memory stores are one process.** They take no claims, so a task
  running on another replica cannot be steered through them.

## Security

- **Authority.** A steering message is a message from its sender and has no
  more authority than they have over the task. It does not cancel (that stays
  `CancelTask`), it is not an approval (the approval gate is not consulted, so
  a decision in its metadata is only text), and it cannot lift a budget. An
  executor treats its text as it treats any other user text. A model told to
  obey it will: [Prompt-injection containment](https://github.com/tomtom215/a2a-rust/blob/main/docs/injection-containment.md)
  lists what the SDK stops when a model obeys every instruction, and a
  steering message is one more place an instruction can come from, with the
  sender's authority.
- **Who may steer.** The message passes through the interceptors before
  anything else, with the task it names in `CallContext::task_id`, so the
  authorization that guards `GetTask` and `CancelTask` guards it. It is read in
  the caller's tenant and caller partition: another tenant's or caller's task
  is not found. A turn is also delivered to only for the tenant that started
  it, even over a store that does not keep tenants apart.
- **A flood.** The rate limit is an interceptor and sits in front of the
  inbox. What gets through is bounded by the inbox, and every message past it
  is answered `503`, none dropped silently and none waited for.
- **Duplicates.** Retry with the idempotency key: the retry is answered
  `replayed` and delivers nothing. Without a key a delivery whose answer was
  lost, sent again, is delivered twice.
- **Size.** 64 KiB of JSON by default, `413` past it.

## What it records

| Where | What |
|---|---|
| Counter `a2a.server.steering`, by `outcome` | `delivered`, `handed_off`, `replayed`, `full`, `ended`, `too_large`, `halted`, `unread` (accepted, then dropped when the turn ended), `received` (taken from the store into an inbox on this replica) |
| Span event `steering message accepted` | On the call's span: the task and where the message went |
| Audit record `task.steered` | Who (the actor), which task, the message id, the digest of the message, and where it went; not the content. Written when the server accepts the message, not when the executor reads it |

## What it does not do

- **Write the message to `Task.history`.** The running turn saves the task
  whole, and a write from admission would be lost by its next save. The audit
  record says who steered what; an executor that wants the message in the
  conversation records it (the tool loop adds it to the model's).
- **Wait for the executor.** The response is the task and a receipt. An
  executor that answers does so through the task: a status message or an
  artifact.
- **Steer through a stream.** A streaming send that steers is answered with the
  task as one event and ends; `SubscribeToTask` follows the task.
- **Group fan-out beyond `steer_all`.** ADR 0033's group of delegations is not
  built; `steer_all` is what it will wrap.
