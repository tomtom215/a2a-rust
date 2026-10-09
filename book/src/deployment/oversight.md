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
# async fn admin(handler: &RequestHandler) {
// One tenant: its new sends are refused, and every task of it that is
// running is stopped.
let report = handler
    .halt(HaltScope::Tenant("acme".into()), "oncall:alice", "runaway spend, INC-4411")
    .await;
println!("stopped {} tasks", report.stopped.len());

// Later:
handler.resume(HaltScope::Tenant("acme".into()), "oncall:alice").await;

// Everything:
handler.halt(HaltScope::All, "oncall:alice", "model provider incident").await;
# }
```

While a scope is halted:

- **New sends are refused,** including sends that would continue a waiting task.
  HTTP+JSON answers `503` with status `UNAVAILABLE`, gRPC `UNAVAILABLE`, and
  JSON-RPC an internal error. Each message reads `halted: <reason>`.
- **Running tasks are stopped.** Each task's cancellation token fires. An
  executor that returns on it without a terminal state has its task ended
  `canceled` by its `cancel` hook, as on shutdown. An executor that uses a
  [`Delegation`](../client/delegation.md) for its sub-tasks cancels them too.
- **Reads and cancels are still served,** so whoever halted it can see what
  stopped.

With the [audit trail](./audit.md), each halt and resume is a `halt` record
naming the operator (`by`), the reason, and the ids of the tasks it stopped.

The SDK has no admin endpoint of its own. Call `halt` from one you already
guard, such as an internal route, a signal handler, or a feature-flag
watcher, so that who may halt is your access control's decision.

### Limits

- **A halt holds in one process.** It is not persisted: halt every replica,
  and halt again after a restart if it should still hold.
- **An executor that ignores its cancellation token keeps running.** Its task
  keeps whatever state it had. Executors should watch
  `ctx.cancellation_token`.
- **Tasks waiting at `input-required` are not running, so they are not
  cancelled.** The halt refuses the send that would continue them.

## Asking before acting

An agent about to take an action it should not take alone asks first. It
names the exact action by the SHA-256 digest of its canonical JSON:

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
use a2a_protocol_server::approval::ApprovalGate;
use a2a_protocol_server::{BearerTokenAuthInterceptor, RequestHandlerBuilder};
# struct MyAgent;
# a2a_protocol_server::agent_executor!(MyAgent, |_ctx, _q| async { Ok(()) });
let handler = RequestHandlerBuilder::new(MyAgent)
    .with_interceptor(BearerTokenAuthInterceptor::with_labelled_tokens([
        ("token-a", "alice"), ("token-b", "bob"), ("token-agent", "billing-agent"),
    ]))
    // Only these two may approve, and never the caller whose run asked.
    .with_approval_gate(ApprovalGate::new().with_approvers(["alice", "bob"]))
    .build();
# let _ = handler;
```

A continuation that carries a decision is refused before anything runs when:

| Check | Refused with |
|---|---|
| The task it names is not waiting on an approval request | invalid params |
| It answers a different request id | invalid params |
| Its digest is not the request's: the person was shown a different action | invalid params |
| The caller is not authenticated | permission denied |
| The caller is not one of the approvers | permission denied |
| The caller is the one whose run asked (unless `allowing_self_approval`) | permission denied |

The executor sees an admitted decision in `ctx.approval()`, with the
approver's identity. Without a gate, `ctx.approval()` is always `None`. A
decision in a message's metadata is a claim, and nothing checked it.

With the [audit trail](./audit.md), each admitted decision is an `approval`
record. Its actor is the approver. It holds the action's digest, the
request id, the decision, and who asked. The approver's comment is not
recorded.

### Limits

- **The executor decides which actions to ask about.** The gate checks and
  records answers. It cannot make an executor ask, or stop one that acts
  without asking.
- **The digest binds the decision to the action the agent described.** It
  does not prove the agent then did that action and nothing else. The
  executor's own code is what keeps that promise.

## Stopping what an agent started

An agent that hands work to other agents should send it with a
[`Delegation`](../client/delegation.md). Then cancelling or halting the
agent's task cancels the tasks it created elsewhere.
