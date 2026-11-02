<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0036: A message to a running task is an extension, delivered through an inbox

**Date:** 2026-11-01
**Status:** Proposed. Nothing here is built or run. The maintainer decided on
2026-11-01 (triage fork F10): write the ADR and the extension URI now, build
after the audit pull request.
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

What this SDK does today: a send naming a task whose turn is still running is
refused with `UnsupportedOperation`, "task ... is already being processed"
(`crates/a2a-protocol-server/src/handler/messaging/admission.rs`). That is
correct for a peer that sends no extension and stays so. The related ledger row
(spec section 7.6.1) is `PARTIAL`.

What exists to build on: the per-turn claim (a task's turn is owned by one
replica, `handler::claims`, with the watchdog of 2026-11-01), the executor
context (`RequestContext`), `Delegation` and the group delegation of ADR 0033.

## Decision

An extension, negotiated through `A2A-Extensions`, with the URI
`https://a2a-rust.com/extensions/steering/v1` (reserved here, not yet
registered by any code).

1. **Wire.** A `SendMessage` whose message names a task that is `working`,
   and declares the extension in `Message.extensions`, is a *steering
   message*. Without the extension, the refusal stays exactly as it is. The
   response is the task's current state (a `Task`), not a new task and not the
   executor's eventual result.
2. **Server.** Admission, for a steering message, does what a send does
   (authentication, authorization, tenant scoping, rate limit, the halt
   check) and then, instead of leasing the task's queue, puts the message in
   the running turn's **inbox**: a bounded per-task channel. A full inbox is
   refused as overloaded (`503` with `Retry-After`), never dropped silently and
   never blocking the caller.
3. **Executor.** `RequestContext::inbox()` returns the receiver, present only
   when the server was built with the extension on. An executor that never
   reads it is unaffected, and a steering message to it sits in the inbox until
   the turn ends; the response says whether the message was *delivered to the
   inbox*, not that the executor acted on it.
4. **Replicas.** The turn is owned by the replica that holds its claim. A
   steering message that arrives elsewhere is handed through the store to that
   replica, which delivers it; the message is part of the task's history either
   way. If the claim lapses before delivery the message is refused as
   transient, because the turn it was for has ended.
5. **Client.** `Delegation::steer(message)` sends one to a delegated child, and
   `DelegationGroup::steer_all` (ADR 0033) to each child of a group; each
   reports per child whether it was delivered.
6. **Authority.** A steering message is a message from its sender. It has no
   more authority than the sender has over the task, and an executor treats its
   text as it treats any other user text (`docs/injection-containment.md`). In
   particular, a steering message does not cancel, and cancelling stays
   `CancelTask`.

## Alternatives

| Option | Why not |
|---|---|
| Accept a message to a working task without an extension | Changes what a conforming peer sees (today's refusal) with no negotiation. |
| Use `input-required` for steering | The task would have to stop to be steered; the point is to steer it while it works. |
| A second task for the follow-up | Loses the running task's context, which is what steering is for. |
| A subscription that carries messages upstream | Not what the streaming binding is; a different protocol surface. |

## Consequences

* Peers without the extension are unchanged.
* The inbox is memory per running turn: bounded, with the bound a setting.
* Delivery is at most once per attempt and the sender is told which. Retrying
  a steering message needs the idempotency extension's key, the way a send
  does.

## What would settle it (nothing is run)

* An executor that reads `ctx.inbox()` sees a message sent while it works, and
  one that does not is unaffected; a peer without the extension still gets
  today's refusal (the existing test stays green).
* A full inbox answers `503` with `Retry-After`, with the executor blocked.
* Two replicas on one PostgreSQL: a steering message to the replica that does
  not hold the claim reaches the executor on the one that does; with the claim
  lapsed it is refused, not lost.
* Authorization: a caller without access to the task cannot steer it, and a
  tenant cannot steer another tenant's.
* Mutation testing on admission's new branch.

## Open

* The inbox bound and the message size cap.
* Whether the response may carry the executor's acknowledgement when it
  replies quickly (a `Message` in the response) or only the state.
* Whether `DelegationGroup::steer_all` waits for all deliveries or returns as
  they come.
