<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Human Oversight

This chapter covers three ways a person stays in control of agents built on
this SDK:

- **Stop it:** halt one tenant or the whole server.
- **Ask first:** an agent waits for a named person to approve an exact action.
- **Stop what it started:** cancelling a task cancels the tasks it delegated to
  other agents.

The EU AI Act asks a high-risk AI system to let a person interrupt it
(Article 14(4)(e)) and to override or reverse its output (14(4)(d)). How
far these features go toward each provision, and what is left, is in the
[control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md).

## Halting

```rust,no_run
use a2a_protocol_server::{HaltScope, RequestHandler};
# async fn admin(handler: &RequestHandler) -> Result<(), a2a_protocol_server::ServerError> {
// One tenant: its new sends are refused, and every task of it that is
// running is stopped.
let report = handler
    .halt(HaltScope::Tenant("acme".into()), "oncall:alice", "runaway spend, INC-4411")
    .await;
println!("stopped {} tasks", report.stopped.len());

// Later:
handler.resume(HaltScope::Tenant("acme".into()), "oncall:alice").await?;

// Everything:
handler.halt(HaltScope::All, "oncall:alice", "model provider incident").await;
# Ok(())
# }
```

While a scope is halted:

- **New sends are refused,** including sends that would continue a waiting task.
  HTTP+JSON answers `503` with status `UNAVAILABLE`, gRPC and SLIMRPC
  `UNAVAILABLE`, and JSON-RPC an internal error. Each message reads `halted: <reason>`.
- **Running tasks are stopped.** Each task's cancellation token fires. An
  executor that returns on it without a terminal state has its task ended
  `canceled` by its `cancel` hook. (A shutdown, by contrast, ends such a task
  `failed`, failure class `transient`: see
  [graceful shutdown](production/graceful-shutdown.md).) An executor that uses a
  [`Delegation`](../client/delegation.md) for its sub-tasks cancels them too.
- **Reads and cancels are still served,** so whoever halted it can see what
  stopped.

With the [audit trail](./audit.md), each halt is a `halt` record naming the
operator (`by`), the reason, and the ids of the tasks it stopped. A resume
that lifts a halt is a `halt` record with `action` `resume` and the operator.

The SDK has no admin endpoint of its own. Call `halt` from one you already
guard, such as an internal route, a signal handler, or a feature-flag
watcher, so that who may halt is your access control's decision.

### Limits

- **Without a halt store, a halt holds in one process.** It is then not
  persisted: halt every replica, and halt again after a restart. With one
  (`RequestHandlerBuilder::with_halt_store`, ADR 0019), a halt given to any
  replica stops every replica sharing the store, and a restarted process
  comes back halted. `PostgresHaltStore` and `InMemoryHaltStore` push the
  halt with the write (`LISTEN`/`NOTIFY` for PostgreSQL, ADR 0024), so it
  arrives about one notification round trip after it commits; a store that
  does not push, and a lost notification, fall back to the poll
  (`with_halt_poll_interval`, one second by default). A `CancelTask` on a
  replica that does not run the task is pushed the same way to the one that
  does, whose executor's token fires; streams open there end without the
  `Canceled` frame, and a `GetTask` reads `canceled`. Use `PostgresHaltStore` across replicas and
  `SqliteHaltStore` for one process. A halt the store cannot take still
  holds where it was given (`HaltReport::persisted` is `Some(false)`) and
  is written again on every poll; a resume it cannot take lifts nothing
  and returns an error. Until the store has been read once, a process
  refuses sends.
- **An executor that ignores its cancellation token keeps running.** Its task
  keeps whatever state it had. Executors should watch
  `ctx.cancellation_token`.
- **Tasks waiting at `input-required` or `auth-required` are not running, so
  they are not cancelled.** The halt refuses the send that would continue
  them.

## Asking before acting

An agent about to take an action it should not take alone asks first. It
names the exact action by the SHA-256 digest of its canonical JSON.
`action_digest` is behind the `signing` feature of `a2a-protocol-types`
(`signing` on `a2a-protocol-sdk` turns it on); the block below does not
compile without it:

```rust,no_run
use a2a_protocol_server::{EventEmitter, RequestContext};
use a2a_protocol_server::streaming::EventQueueWriter;
use a2a_protocol_types::approval::{ApprovalRequest, action_digest};
use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::task::TaskState;
# async fn refund(_a: &serde_json::Value) -> A2aResult<()> { Ok(()) }

async fn execute(ctx: &RequestContext, queue: &dyn EventQueueWriter) -> A2aResult<()> {
    let emit = EventEmitter::new(ctx, queue);
    let action = serde_json::json!({ "tool": "refund", "order": 1182, "amount": "40.00" });

    match ctx.approval() {
        // First run: ask, and stop here. The task waits at input-required.
        None => {
            let request = ApprovalRequest::new(
                uuid::Uuid::new_v4().to_string(),
                "Refund EUR 40.00 to order 1182",
                action_digest(&action)?,
            );
            emit.request_approval(request, "May I issue this refund?").await?;
            Ok(())
        }
        // The next run carries the decision, already checked by the gate.
        Some(a) if a.is_approved() => {
            refund(&action).await?;
            emit.status(TaskState::Completed).await
        }
        Some(_) => emit.status(TaskState::Rejected).await,
    }
}
```

The approver's client answers by continuing the task with a decision that
echoes the request:

```rust,no_run
use a2a_protocol_types::approval::{ApprovalDecision, ApprovalRequest};
use a2a_protocol_types::{Message, MessageSendParams, Task};
# async fn answer(client: &a2a_protocol_client::A2aClient, task: Task)
#     -> Result<(), Box<dyn std::error::Error>> {
let status = task.status.message.as_ref().expect("the question");
let request = ApprovalRequest::read(status)?.expect("an approval request");
// Show request.summary to the person; on their "yes":
let mut reply = Message::user_text(uuid::Uuid::new_v4().to_string(), "approved");
reply.task_id = Some(task.id.clone());
reply.context_id = Some(task.context_id.clone());
ApprovalDecision::approve(&request)
    .with_comment("checked the order")
    .attach(&mut reply);
client.send_message(MessageSendParams::new(reply)).await?;
# Ok(())
# }
```

### The gate

On the server, `with_approval_gate` decides which decisions reach the executor:

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
#         .with_security_schemes(std::collections::HashMap::from([(
#             "bearer".to_owned(),
#             a2a_protocol_types::security::SecurityScheme::Http(
#                 a2a_protocol_types::security::HttpAuthSecurityScheme {
#                     scheme: "Bearer".to_owned(),
#                     description: None,
#                     bearer_format: None,
#                 },
#             ),
#         )]))
# }
use a2a_protocol_server::approval::ApprovalGate;
use a2a_protocol_server::auth::caller_id;
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};
# struct MyAgent;
# a2a_protocol_server::agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_agent_card(card())
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("token-a", "alice"), ("token-b", "bob"), ("token-agent", "billing-agent"),
    ]))
    // Only these two may approve, and never the caller whose run asked. The
    // bearer interceptor names them `bearer:alice` and `bearer:bob`.
    .with_approval_gate(
        ApprovalGate::new().with_approvers([caller_id::bearer("alice"), caller_id::bearer("bob")]),
    )
    // The approver has to reach the requester's task.
    .share_tasks_across_callers()
    .build();
# let _ = handler;
```

`ApprovalGate::new()` on its own lets any authenticated caller approve,
except the one whose run asked; `with_approvers` narrows that to the
identities you list. Without it, anyone your interceptor authenticates can
approve.

An approver answers by continuing the requester's task, so the two must
share it. With an authenticating interceptor, tasks are isolated by caller
unless the builder says otherwise, and `build()` refuses a gate that wants a
second caller until it does: call `share_tasks_across_callers()`, as above,
or let requesters answer their own runs with
`ApprovalGate::allowing_self_approval()`.

A continuation that carries a decision is refused before anything runs when:

| Check | Refused with |
|---|---|
| The task it names is not waiting on an approval request | invalid params |
| It answers a different request id | invalid params |
| Its digest is not the request's: the person was shown a different action | invalid params |
| The caller is not authenticated | permission denied |
| The caller is not among the identities given to `with_approvers` | permission denied |
| The caller is the one whose run asked (unless `allowing_self_approval`) | permission denied |

The executor sees an admitted decision in `ctx.approval()`, with the
approver's identity. Without a gate, `ctx.approval()` is always `None`. A
decision in a message's metadata is a claim, and nothing checked it.

With the [audit trail](./audit.md), each admitted decision is an `approval`
record. Its actor is the approver. It holds the action's digest, the
request id, the decision, and who asked. The approver's comment is not
recorded.

### More than one approver

The EU AI Act's Article 14(5) asks, for the remote biometric identification
systems in its Annex III point 1(a), that no action or decision is taken by
the deployer on the basis of an identification unless at least two natural
persons have separately verified and confirmed it. The paragraph does not
apply to systems used for law enforcement, migration, border control or
asylum where Union or national law considers it disproportionate. The
[control map](https://github.com/tomtom215/a2a-rust/blob/main/docs/compliance/control-map.md)
did not check whether Regulation (EU) 2026/1744 amends this paragraph, so
read the consolidated text. `with_quorum(n)` makes the gate wait for `n`
distinct approvers:

```rust
use a2a_protocol_server::approval::ApprovalGate;
use a2a_protocol_server::auth::caller_id;
// Any two of these three, never the caller whose run asked.
let gate = ApprovalGate::new()
    .with_approvers([
        caller_id::bearer("alice"),
        caller_id::bearer("bob"),
        caller_id::bearer("carol"),
    ])
    .with_quorum(2);
# let _ = gate;
```

Each approver sends the same decision they would send to a gate without a
quorum, for the same request id and digest. Until `n` of them have
approved, the executor does not run: each approver is answered with the task
still `input-required`, and its status message carries, under
`a2a-rust.com/approval-quorum`, who has approved so far and how many are
required. The `n`-th approval reaches the executor, and `ctx.approval()`
names every approver in `approvers`.

- Approvers are told apart by their authenticated identity. One approver
  approving twice counts once; the requester is refused, as without a quorum.
- A deny from any allowed approver reaches the executor at once, naming who
  had approved before it. The action needs all `n`, so one refusal settles it.
- A decision for another digest is refused and counts for nothing.
- Each decision is its own `approval` audit record, with `required`, the
  `approvedBy` list as it stood, and `reachedExecutor`.
- `build()` refuses a quorum of `0`, one larger than the `with_approvers`
  list, or one above `1` with tasks isolated by caller.

The count is kept on the task, so replicas sharing a task store count
together. One handler serializes the approvals for a task; two replicas do
not, and two approvals arriving at two replicas at the same moment can each
overwrite the other's record. The count then falls short, and never counts an
approval nobody sent: the approver whose record was lost sends the approval
again. The last two approvals can also reach two replicas at the same moment
and both meet the quorum, which resumes the executor twice. Run one replica,
or route every turn of a task to one replica
([Running More Than One Replica](./horizontal-scaling.md)).

Whether a system falls under Annex III point 1(a), and whether the
approvers have the competence, training and authority the Article asks for,
are the deployer's to decide; the gate counts identities, so a person who
holds two allowed identities counts twice.

### Limits

- **The executor decides which actions to ask about.** The gate checks and
  records answers. It cannot make an executor ask, or stop one that acts
  without asking.
- **The digest binds the decision to the action the agent described.** It
  does not prove the agent then did that action and nothing else. The
  executor's own code is what keeps that promise.

## Stopping what an agent started

An agent that hands work to other agents should send it with a
[`Delegation`](../client/delegation.md) and pass
`ctx.cancellation_token.cancelled()` to `wait`. Then cancelling or halting
the agent's task cancels the tasks it created elsewhere. A parent that
crashes cancels nothing unless it started the child with
`Delegation::start_leased` (ADR 0020), which grants the child a lease that
lapses when the parent stops renewing it.
