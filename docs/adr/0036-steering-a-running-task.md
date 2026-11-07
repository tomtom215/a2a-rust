<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0036: A message to a running task is an extension, delivered through an inbox

**Date:** 2026-11-07
**Status:** Accepted, and built on 2026-11-07 (see *Built* and *Not built*
below). Proposed on 2026-11-01, when the maintainer decided (triage fork F10)
to write the ADR and reserve the extension URI first and build after the audit
pull request.
**Author:** Tom F.

---

## Context

The maintainer asked for agents that can message a teammate while it works:
steer a running task, or tell several to change course.

What the specification says (v1.0.1, sections 3.4 and 7.6): a follow-up
message is defined for a task in an interrupted state (`input-required`,
`auth-required`). It says nothing about a message sent to a task that is still
`working`. So steering is neither required nor forbidden: it is room for an
extension, the way idempotency, failure class, budget, lease and lineage are
(`https://a2a-rust.com/extensions/budget/v1` is one).

What this SDK did before: a send naming a task whose turn is still running is
refused with `UnsupportedOperation`, "task ... is already being processed"
(`crates/a2a-protocol-server/src/handler/messaging/admission.rs`). That is
correct for a peer that sends no extension and stays so. The related ledger row
(spec section 7.6.1) is `PARTIAL`.

What it was built on: the per-turn claim (a task's turn is owned by one
replica, `handler::claims`, with the watchdog of 2026-11-01), the executor
context (`RequestContext`), `Delegation`, and the halt store's
`LISTEN`/`NOTIFY` (ADR 0024).

## Decision

An extension, negotiated through `A2A-Extensions`, with the URI
`https://a2a-rust.com/extensions/steering/v1` (reserved on 2026-11-01, now
`a2a_protocol_types::steering::STEERING_EXTENSION_URI`).

1. **Wire.** A `SendMessage` whose message names a task that is running and
   lists the URI in `Message.extensions` is a *steering message*. A task is
   running when its stored state is `working`, or `submitted` (the executor is
   started when the task is created, and the row says `submitted` until its
   first write), and a turn runs it. Without the extension, the refusal stays
   exactly as it was. The response is the task as it stands (a `Task`), not a
   new task and not the executor's eventual result; it carries a receipt in
   `Task.metadata["a2a-rust.com/steering"]`, `{"delivery": ...}`, which says
   where the message went: `inbox` (this replica's turn), `handed-off`
   (stored for the replica that holds the turn's claim) or `replayed` (an
   idempotency key held by this same message: nothing delivered again). A
   task that is *not* running is not steering: one parked at `input-required`
   or `auth-required` is continued as the specification says, with no receipt,
   and a finished one is refused as it always was.
2. **Server.** Off by default (`RequestHandlerBuilder::with_steering`), and
   the card declares the extension only when it is on. A steering message goes
   through everything a send does before it is looked at: the interceptors
   (authentication, the authorization of the named task through
   `CallContext::task_id`, the rate limit), required extensions, the tenant
   and caller partition, the message's own validation. Then, instead of leasing
   the task's queue, it takes no tenant slot and no admission slot (it starts
   no turn), is checked against the halt of the task's job, against the size
   cap, and against the idempotency key, and is put in the running turn's
   **inbox**: a bounded per-turn queue. A full inbox is refused as overloaded
   (`503` with `Retry-After`; on JSON-RPC the internal error with `RetryInfo`),
   never dropped silently and never blocking the caller.
3. **Executor.** `RequestContext::inbox()` returns the inbox, present only when
   the server was built with the extension on. An executor that never reads it
   is unaffected. The inbox is closed when the executor returns: a later
   message is refused as the turn having ended, and what was never read is
   dropped with the turn, said in the log and counted (`unread`). The response
   says whether the message was *delivered to the inbox*, not that the
   executor acted on it. `ToolLoopExecutor::with_steering` is the bundled
   consumer: it reads the inbox before each model turn and adds each message's
   text as a user turn.
4. **Replicas.** The turn is owned by the replica that holds its claim. A
   steering message that arrives elsewhere is handed through the claim store
   (`TurnClaimStore::hand_steering`, in one atomic step with the check that the
   claim is live) to that replica, which moves it into the turn's inbox at its
   next look at the store (every claim renewal interval) or, on PostgreSQL with
   notifications on, when the store announces it. If the claim lapses before
   the hand-off, or ends before the message is read, nothing is delivered and
   the sender is told (`503`, "ended"): refused as transient, because the turn
   it was for has ended, not lost. A message waits in the store until the
   inbox has room, so a full inbox delays a handed-off message and never loses
   it; the store's queue is bounded by the same capacity, so a task holds at
   most twice its inbox's capacity. The in-memory stores are one process: they
   have no claims, and a task that runs on another replica cannot be steered
   through them (`InMemoryTurnClaims` is for tests that run several handlers in
   one process).
5. **Client.** `Delegation::steer(message)` sends one to a delegated child, and
   `steer_all(children, make_message)` to each of a set of delegations at once;
   each reports per child whether it was delivered (`Steering`), and a child
   not yet named by an event is reported as such with nothing sent.
   `Delegation::steer_handle` gives an owned handle for steering from another
   task while the delegation is awaited. ADR 0033's group of delegations
   (`DelegationGroup::steer_all`) does not exist yet (ADR 0033 is only
   Proposed): when it does, it wraps `steer_all`.
6. **Authority.** A steering message is a message from its sender. It has no
   more authority than the sender has over the task, and an executor treats its
   text as it treats any other user text (`docs/injection-containment.md`). In
   particular, a steering message does not cancel, and cancelling stays
   `CancelTask`; it is not an approval decision (the gate is not consulted, so
   a decision in its metadata is not one); and it cannot lift a budget.

### The open items, decided

* **The inbox bound: 16 messages** (`SteeringConfig::with_inbox_capacity`;
  `build` refuses 0). A person or a supervising agent corrects course a few
  times while a turn reads between steps; sixteen holds that, and with the
  size cap below bounds the memory a turn that never reads its inbox holds at
  about a mebibyte, for as long as it runs. A larger bound is a setting, not a
  default: the cost of a bound too small is a `503` the sender can retry, the
  cost of one too large is memory held for an executor that is not reading.
* **The message size cap: 64 KiB of the message's JSON**
  (`SteeringConfig::with_max_message_size`; `413`; `build` refuses 0). A
  steering message is read by a model between steps and, across replicas, is a
  row in the shared database; one past 64 KiB is a document, and a document
  belongs in a task of its own. Measured on the message as it serializes, so
  the limit is exact (a test pins one byte under, at and one byte over).
* **The response carries the task and the receipt, not an executor
  acknowledgement.** Waiting for the executor to say something would turn a
  delivery into a wait, and a caller that was promised "never blocking" would
  block for as long as the executor chose; an executor that does not read its
  inbox would make every steer time out, and "no acknowledgement" would not
  say whether the executor was slow, absent or ignoring it. An executor that
  wants to answer says so through the task: a status message or an artifact,
  which every watcher of the task sees.
* **`steer_all` waits for all.** Each send is bounded by the client's own
  timeout, one that fails or is refused does not stop the others, and the
  caller gets the whole outcome at once, in the order given, which is what a
  fan-out of corrections is for. Returning as they come would be a stream type
  and a second API; ADR 0033's group can offer it.

### Decisions the build added

* **Notifications are opt-in** (`SteeringConfig::with_notifications`). A
  `LISTEN` session on a pooled connection breaks behind a connection pooler in
  transaction mode (measured against `PgBouncer` 1.26.1 on 2026-10-30, for the
  halt store, which listens the same way): the next notification arrives in
  the middle of another client's statement. Steering is itself opt-in, but an
  operator turning it on to steer should not also get a pooler incident, so
  the default is the look every renewal interval (one second), which delivers
  every message, and the push is a setting for a direct connection. With it on
  and a three-second renewal interval, a handed-off message reached the
  executor within 1.5 s in a test that fails if only the poll can deliver it.
* **The steering table is created on first use**, by the claim store
  (`a2a_task_steering`, in the task store's database), not with the task
  store's schema: a deployment that never steers has no table, and a replica of
  an earlier release is not affected. A message is keyed by the claim's token,
  so a later turn of the same task never reads an earlier turn's mail; rows
  for a claim that ended are deleted at its release and swept with the claims.
* **A message is not written to `Task.history`.** The proposal said "the
  message is part of the task's history either way". The running turn's
  processor holds its own copy of the task and saves it whole when an agent
  message arrives (`handler/event_processing/background/state_machine.rs`,
  `append_message`), so a write to the stored task from admission, on this
  replica or another, would be overwritten by the turn's next save: a lost
  update, the class of defect turn claims exist to prevent. Writing the message
  as an event through the turn's queue would have it folded into the task's
  status message (spec section 3.1.2) as if the agent had said it. So the
  record of who steered what is the audit trail (`task.steered`: the actor, the
  task, the message id and the digest of the message, and where it went), and
  the executor that wants the message in the conversation writes it
  (`ToolLoopExecutor` adds it to the model's conversation). Putting steering
  messages in `Task.history` is *not built*; it needs the processor to own the
  write.
* **A turn that has parked is not steered**, even before its executor has
  returned: the message is admitted as a continuation, which first waits for
  the turn to finish, as every message to a parked task does.
* **A steering message to another tenant's turn is refused by the turn too**:
  the handler records the tenant on each running turn and delivers only to a
  turn of the tenant of the request, so a deployment that opted in to a store
  that does not keep tenants apart (`accept_unisolated_tenants`) still cannot
  steer across tenants.

## Alternatives

| Option | Why not |
|---|---|
| Accept a message to a working task without an extension | Changes what a conforming peer sees (today's refusal) with no negotiation. |
| Use `input-required` for steering | The task would have to stop to be steered; the point is to steer it while it works. |
| A second task for the follow-up | Loses the running task's context, which is what steering is for. |
| A subscription that carries messages upstream | Not what the streaming binding is; a different protocol surface. |
| Write the message to `Task.history` from admission | Lost to the running turn's next whole-task save; see above. |
| Hand the message across replicas with `NOTIFY` alone | A notification is lost when the listener is away; the message would be too. The store holds it, the notification only hurries it. |

## Consequences

* Peers without the extension are unchanged: every existing test of the
  refusal is green, and three more pin it with steering off and declared, on
  and not declared, and on with the extension named only in metadata.
* The inbox is memory per running turn: bounded, with the bound a setting.
* Delivery is at most once per attempt and the sender is told which. Retrying
  a steering message needs the idempotency extension's key, the way a send
  does: with one, the retry is answered `replayed` and delivers nothing.
  Without a key a delivery whose answer was lost, sent again, is delivered
  twice. If releasing a key after a refusal fails, a retry reads as `replayed`
  though nothing was delivered (the send path has the same limit; it is logged).
* A message accepted into an inbox and unread when the turn ends is dropped
  and counted (`unread`). A message handed off and still in the store when the
  claim is released is deleted with it, and is not counted; the window is the
  time the turn takes to settle after its executor returns.
* A replica that dies keeps its claim until the claim's time to live (fifteen
  seconds by default) runs out, and the claim is what says the task is running
  on a replica that can read. In that window a message handed off for the dead
  replica is accepted (`handed-off`) and nothing reads it; when the reaper ends
  or resumes the turn, the message goes with the claim. The sender was told
  `handed-off`, which is *accepted for delivery*, not *read*; a sender that
  needs to know reads the task's state afterwards, or sends again once the task
  is `failed`, when the send is refused. This is the limit of every claim-based
  answer to "is the owner alive": the lease is the only evidence there is.
  `scripts/live/steering` kills a replica with `SIGKILL` and records it.
* One more read of the claim store per renewal interval on a replica that holds
  a claim and takes steering (one indexed statement); none when steering is off.

## What would settle it, and what did

Each item is a test that fails without the behaviour; the commands are in the
pull request's report.

* An executor that reads `ctx.inbox()` sees a message sent while it works, and
  one that does not is unaffected; a peer without the extension still gets
  today's refusal: `an_executor_that_reads_its_inbox_sees_the_messages_sent_while_it_works`,
  `an_executor_that_never_reads_its_inbox_is_unaffected`,
  `a_peer_without_the_extension_is_refused_exactly_as_before` (and, on the
  wire, `a_peer_that_does_not_ask_for_steering_gets_the_refusal_it_always_got`).
* A full inbox answers `503` with `Retry-After`, with the executor blocked:
  `a_full_inbox_answers_503_with_the_executor_blocked` and, on both bindings,
  `a_full_inbox_answers_503_with_retry_after_on_each_binding`.
* Two replicas on one PostgreSQL: a steering message to the replica that does
  not hold the claim reaches the executor on the one that does; with the claim
  gone it is refused, not lost: `tests/turn_claims`
  (`postgres_a_steering_message_reaches_the_executor_on_the_replica_that_holds_the_claim`,
  `postgres_with_the_claim_gone_a_steering_message_is_refused_not_lost`), the
  same on SQLite, and two real server processes
  (`scripts/live/steering/README.md`).
* Authorization: a caller without access to the task cannot steer it, and a
  tenant cannot steer another tenant's: `a_caller_the_authorizer_refuses_cannot_steer_the_task`,
  `another_caller_cannot_steer_a_task_it_cannot_see`,
  `a_tenant_cannot_steer_another_tenants_task`,
  `a_shared_store_does_not_let_a_tenant_steer_another_tenants_turn`, and at the
  store, `a_message_is_for_the_scope_of_the_claim`.
* Mutation testing on admission's new branch: zero survivors, reported in the
  pull request with the command.

## Built

* The extension, its receipt and its contract snapshot
  (`a2a_protocol_types::steering`, `tests/golden/uri_contracts/steering.json`).
* The server: `SteeringConfig`, `Inbox`, `Steered`, `RequestContext::inbox`,
  `RequestHandlerBuilder::with_steering`, the card's declaration, the
  admission described above, the inbox closed when the executor returns.
* Across replicas: the defaulted `TurnClaimStore` methods `hand_steering`,
  `peek_steering`, `ack_steering` and `steering_signals`, with the in-memory,
  SQLite (plain and tenant-aware) and PostgreSQL implementations, the
  delivery loop beside the claim renewer, and the `LISTEN`/`NOTIFY` push.
* `Metrics::on_steering` and the `a2a.server.steering` counter (outcomes
  `delivered`, `handed_off`, `replayed`, `full`, `ended`, `too_large`,
  `halted`, `unread`, `received`), a span event on the call
  (`steering message accepted`), and the audit record `task.steered`.
* The client: `Delegation::steer`, `SteerHandle`, `steer_all`, `Steering`.
* The consumer: `ToolLoopExecutor::with_steering`.
* The book page *Steering a running task*, the extension's section in *Swarm
  Extensions*, and the gate that a claim store overrides all three steering
  methods or none (`scripts/check_store_delta_forwarding.py`).

## Not built

* Steering messages in `Task.history` (above).
* ADR 0033's `DelegationGroup`, which does not exist; `steer_all` is what it
  will wrap.
* A steering message on a streaming send is answered with the task as one
  event and the stream ends; it does not subscribe the caller to the running
  task's events (that is `SubscribeToTask`).
* The gRPC and WebSocket bindings carry a steering message as they carry any
  `SendMessage`; the tests drive JSON-RPC and HTTP+JSON only.
* No acknowledgement from the executor in the response (decided above).
* A count of messages dropped from the store with a released claim (the
  `unread` counter covers messages that reached an inbox).
* A read receipt: `handed-off` means stored for the replica that holds the
  claim, including, for up to the claim's time to live, a replica that has
  died (Consequences).
