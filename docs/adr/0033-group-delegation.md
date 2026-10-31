<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0033: Fan-out is a group of delegations with one join rule, one budget split and one cancel

**Date:** 2026-10-31
**Status:** Proposed. Nothing here is built or run. The maintainer asked on
2026-10-31 that the fan-out design be recorded for a later session.
**Author:** Tom F.

---

## Context

One child is covered. `Delegation` (`crates/a2a-protocol-client/src/delegation/`)
sends a child, learns its task id, cancels it when the parent stops watching
(`cancel`, a lost stream, a drop), renews a lease (`start_leased`, ADR 0020),
carries a budget (`start_budgeted`, ADR 0023), resumes a lost stream
(ADR 0030), and can keep a journal record that outlives the supervisor
(`start_journaled`).

Many children are not. The swarm example fans out by hand: a `Semaphore`, a
`JoinSet`, and `run_once` per child (`examples/swarm/src/supervisor.rs`). Every
orchestrator on this SDK writes the same five things again, and some of them
are easy to write wrongly:

1. **A cap on children in flight.** Without it a loop starts all of them.
2. **A join rule.** Wait for all; take the first success; wait for k; stop at
   the first failure. The rules that finish early need the rest cancelled and
   their ends awaited, which needs the handles in one owner.
3. **A budget split.** `BudgetHandle::grant` gives each meter the smaller of
   the amount requested and the amount left, and reserves it (ADR 0023, "Ten
   children cannot each be given the whole"). A loop that requests the
   parent's whole remainder for each child therefore gives the first child
   everything and the rest nothing. The behaviour is correct for one grant
   and a trap for a loop.
4. **One cancel for the lot**, including when the parent's task is dropped.
5. **Results in input order, each with its own end** (`Outcome`, and the
   failure class on a failed one), so a caller can retry only what is
   retryable.

The measurement that justified the single handle applies here: with the
cascade written by hand, 256 live executions went to 0 in 65 ms, and 512 to 0
in 84 ms (`docs/swarm-orchestration.md`, R1; that is 64 and 256 workers, two
hops). Nothing above 256 workers has been measured.

## Decision

In `a2a_protocol_client::delegation`, as a client-side type over existing
methods. No wire change, no new extension.

1. **`DelegationGroup`**, built with a **required** cap on children in
   flight (`NonZeroUsize`; there is no default, because an unbounded default
   is the footgun). Members are added with their client, message, optional
   lease and optional budget share. Members past the cap wait; a waiting
   member has no child and holds no reservation.
2. **`Join`**: `All`, `FirstOk`, `Quorum(k)`, `FailFast`. When the rule is
   met, the group cancels the members still running through
   `Delegation::cancel`, waits for each to settle within a bounded time, and
   reports every member's end, `Canceled` for those it stopped. A member the
   group never started is reported as not started, with the reason.
3. **Budget shares** are taken when a member *starts*, not when it is added:
   each meter's share is what the parent has left divided by the members not
   yet started (equal), or a caller-given fixed or weighted share. Children
   that settle refund their unspent amounts (ADR 0023), so later starters
   receive them. The sum of live reservations never exceeds what the parent
   had. A member whose share cannot be granted (`Exhausted`) is reported not
   started with that error, never silently dropped. The deadline is the
   parent's, less the existing reserve.
4. **One cancel**: a future given to `run`, and the drop of the `run` future,
   cancel every started member by the existing rules. A group is not
   `detach`able as a whole; detaching a member stays a member-level act.
5. **Results** come back in input order as `Vec<MemberOutcome>`, each holding
   the member's `Delegated` or the not-started reason. The group does **not**
   retry a settled member. A helper selects the members whose `FailureClass`
   is retryable so the caller can run them as a new group, within its own
   retry budget (ADR 0032 for the shared form).
6. **Leases** in a group are staggered: each member's renewal tick starts at a
   random offset within its first interval, so a thousand members started
   together do not renew in one burst. The cost stays one `GetTask` per
   member per third of the time to live (audit H5); this ADR does not reduce
   it.

### What stays out

DAGs and planning, choosing which agent gets which piece, reducing results
into one answer, and placement. Those are an orchestrator's, and
`examples/swarm` is where one lives (`docs/swarm-orchestration.md`, "Where
this work goes, and where it does not"; `docs/handoff-history.md`, "What not
to chase"). The group runs a set of calls the caller chose; it does not choose.

## Alternatives

| Option | Why not |
|---|---|
| Leave it to examples | Every orchestrator rewrites five things, and the budget split is where it goes wrong quietly. |
| `join_all`-style helpers over `Delegation` | No cap, no split, and the cancel-the-rest rules need one owner of the handles. |
| A protocol-level fan-out method | A cross-SDK change for something that is a client's loop. |
| Retries inside the group | A policy with a budget of its own (H6); a settled child is not run again behind the caller's back. |

## Consequences

* Additive; `Delegation` is unchanged.
* The group's journal story is open (below), so durable fan-out across a
  supervisor restart is not part of the first cut.

## What would settle it (nothing is run)

* Against a real server with N children: `All` returns N outcomes in input
  order; `FirstOk` and `FailFast` leave the rest `canceled` on the server;
  `Quorum(k)` stops at k.
* The in-flight cap, observed from the server side by a counting executor,
  never exceeds the cap.
* Budget: ten members and a parent of 100 tokens each get 10, none gets 100;
  the sum of reservations never exceeds the parent's remainder; a member
  that cannot be granted is reported as such. Property test over member
  counts, caps and parent budgets, plus mutation testing on the share
  arithmetic.
* Dropping the `run` future cancels every started member (the existing
  `aborting_the_parent_task_cancels_the_child` shape, for N).
* The swarm example rewritten on the group, the 64- and 256-worker cancel
  gates run again before and after, with hardware and build profile stated.
  Behaviour above 256 workers stays unmeasured until a run says otherwise.

## Open

* **The journal.** A restarted supervisor reattaches members one by one
  (ADR 0030). A group needs to find its members: a `group` field on the
  journal record (a change to ADR 0030's contract, the PostgreSQL schema and
  the shared contract tests), or a separate group record. Settle before
  building durable groups.
* Whether `Join` is an enum or a closure over the settled members so far.
* How `Quorum(k)` reports when fewer than k can still succeed.
