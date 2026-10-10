<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Delegating to Another Agent

An agent that hands work to other agents creates tasks on them, and A2A does
not link those tasks to its own. Cancel the parent and its children keep
running unless the parent's code cancels each one. In `examples/swarm`,
every child kept running when its root was cancelled and the orchestrator
had not written that cascade.

`Delegation` is that cascade. It sends the child, learns the child's task id
from the first event that names it, and cancels the child when:

- the parent cancels: the `cancel` future passed to `wait` completes, or the
  parent calls `cancel()`;
- the child's stream is lost before the child settled; or
- the handle is dropped before the child settled. That happens when the
  parent's own future is aborted, or when its code returns early.

A child *settles* when it reaches a terminal state, or `input-required` or
`auth-required`. In those two it is waiting for the parent, so it is handed
back rather than cancelled.

```rust,no_run
use std::sync::Arc;
use a2a_protocol_client::ClientBuilder;
use a2a_protocol_client::delegation::{Delegation, Outcome};
use a2a_protocol_types::{Message, MessageSendParams, Part};
use a2a_protocol_types::failure::FailureClass;
# async fn run(parent_cancelled: impl std::future::Future<Output = ()>)
#     -> Result<(), Box<dyn std::error::Error>> {

let worker = Arc::new(ClientBuilder::new("http://worker:8080").build()?);
let params = MessageSendParams::new(Message::user("m-1", vec![Part::text("summarise")]));

// Inside an executor, pass `ctx.cancellation_token.cancelled()`.
let child = Delegation::start(worker, params).await?;
let done = child.wait(parent_cancelled).await;

match done.outcome {
    Outcome::Completed(_) => { /* fetch its artifacts by done.task_id */ }
    Outcome::Failed { class: FailureClass::Transient, .. } => { /* retry elsewhere */ }
    Outcome::Interrupted(_) => { /* it needs input: continue it by task id */ }
    other => eprintln!("child {:?}: {other:?}", done.task_id),
}
# Ok(())
# }
```

To see the child's events as they arrive, call `next_event()` in a loop
instead of `wait`. The handle still tracks the child's id and state.

## Outcomes

| Outcome | Meaning |
|---|---|
| `Completed`, `Failed`, `Canceled` | The child reached that state. `Failed` carries its [failure class](./failure-classes.md). |
| `Interrupted` | `input-required` or `auth-required`. The child was not cancelled. |
| `Message` | The agent answered with a message and created no task. |
| `Lost` | The stream failed or closed before the child settled. If its id was known, the child was sent `CancelTask`. |
| `CancelRequested` | `CancelTask` was accepted. The task it returned was not yet terminal. |
| `CancelFailed` | `CancelTask` was refused or failed. |
| `Unreachable` | A cancel was asked for before any event named the child, and none did within the id wait (`with_id_wait`, 5 s by default). No `CancelTask` could be sent. |

A child is cancelled in the tenant it was sent to (`MessageSendParams::tenant`),
not the client's default.

## Keeping a child

`detach()` stops following the child without cancelling it, and returns its
id. Use it to hand the child over, for instance to a supervisor that records
the id durably and later reattaches with `subscribe_to_task_from`.

## What it does not cover

- **A crashed parent.** The handle dies with the process, so nothing
  cancels the child. Covering that needs the child to stop on its own when
  its parent goes quiet, for example with a lease. That is gap G1-B in
  [`docs/swarm-orchestration.md`](https://github.com/tomtom215/a2a-rust/blob/main/docs/swarm-orchestration.md).
- **A drop outside a Tokio runtime.** A drop-time cancel is sent from a
  task spawned on the current runtime. Without a runtime nothing is sent; the
  drop is logged when the `tracing` feature is on.
- **A child that stays silent.** A child the agent created but never
  announced cannot be cancelled by id. The handle waits up to the id wait
  for its first event, then reports `Unreachable`.
