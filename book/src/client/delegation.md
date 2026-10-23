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

## A crashed parent: leases

The handle dies with its process, so on its own it cannot cancel the child
of a parent that crashed, hung or was partitioned away. A lease can
(ADR 0020): `Delegation::start_leased` grants the child a time to live and
renews it every third of that while the handle lives. An agent built with
`RequestHandlerBuilder::with_lease_store` stops a child whose lease lapses,
so a parent that goes quiet leaves a child that stops within about one
time to live.

```rust,no_run
use std::sync::Arc;
use std::time::Duration;
use a2a_protocol_client::{ClientBuilder, delegation::Delegation};
use a2a_protocol_types::{Message, MessageSendParams, Part};
# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let worker = Arc::new(ClientBuilder::new("http://worker:8080").build()?);
let params = MessageSendParams::new(Message::user("m-1", vec![Part::text("summarise")]));
let child = Delegation::start_leased(worker, params, Duration::from_secs(30)).await?;
# let _ = child;
# Ok(())
# }
```

The agent side, sharing one store across replicas:

```rust,no_run
# use a2a_protocol_sdk::prelude::*;
# struct MyAgent;
# agent_executor!(MyAgent, |_ctx, _queue| async { Ok(()) });
# async fn agent(pool: sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::PostgresLeaseStore;

let handler = RequestHandlerBuilder::new(MyAgent)
    .with_lease_store(PostgresLeaseStore::from_pool(pool).await?)
    .build()?;
# let _ = handler;
# Ok(())
# }
```

A renewal is a `GetTask` that activates the lease extension
(`A2aClient::renew_lease`); a plain read renews nothing. Over WebSocket
the extension header is sent only with the upgrade, so ask for the
extension when connecting. An agent that grants no leases ignores the
grant. `detach()` stops the renewals too: a detached leased child stops
once its lease lapses.

## A parent's budget: deadlines and meters

A child can also outlive its parent's *deadline*, or spend more than the
parent had. A budget prevents both (ADR 0023). Every agent built on this SDK
holds a turn to the deadline its caller sent, and a turn the deadline stops
ends `failed`, classed `budget-exhausted`. Inside an executor,
`ctx.budget()` is the turn's account: what is left, `charge` for what was
spent, and `grant` to carve a child's budget out of it.

```rust,no_run
use std::sync::Arc;
use a2a_protocol_client::{A2aClient, delegation::Delegation};
use a2a_protocol_types::budget::{Budget, BudgetHandle};
use a2a_protocol_types::{Message, MessageSendParams, Part};
# async fn run(budget: &BudgetHandle, worker: Arc<A2aClient>)
#     -> Result<(), Box<dyn std::error::Error>> {
// Up to 20,000 of this turn's tokens, and all the time it has left less a
// reserve for transit and wrap-up.
let grant = budget.grant(&Budget::new().with_meter("tokens", 20_000)?)?;
let params = MessageSendParams::new(Message::user("m-1", vec![Part::text("summarise")]));
let child = Delegation::start_budgeted(worker, params, grant, None).await?;
let done = child.wait(std::future::pending()).await;
// The parent has been charged what the child reported spending.
# let _ = done;
# Ok(())
# }
```

A grant reserves each meter from the parent at once, so ten children
cannot each be given the whole. A meter the request does not name goes to
the child as 0, so a child cannot spend outside the parent's budget by
leaving it out. Each status update the child's agent writes carries what is
left and spent, under `a2a-rust.com/budget` in its `metadata`. When the
update that settles the child reports its spend, the parent is charged that
in place of the reservation. Any other ending (a lost stream, a cancel, a
detached or dropped handle) refunds nothing. Pass `Some(ttl)` as the last argument to grant a lease as
well.

The SDK's tool loop charges each model reply's reported token usage to the
`tokens` meter and stops the run once it is spent.

## What it does not cover

- **An agent that does not implement the budget.** It ignores the budget
  and reports no spend, so its child is bounded only by its own limits,
  and the parent is charged the whole reservation.
- **A drop outside a Tokio runtime.** A drop-time cancel is sent from a
  task spawned on the current runtime. Without a runtime nothing is sent; the
  drop is logged when the `tracing` feature is on.
- **A child that stays silent.** A child the agent created but never
  announced cannot be cancelled by id. The handle waits up to the id wait
  for its first event, then reports `Unreachable`.
