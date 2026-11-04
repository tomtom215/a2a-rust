<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0037: Push notifications get a durable outbox, claimed per webhook, where the store is shared

**Date:** 2026-11-04
**Status:** Proposed. Nothing here is built or run. It records the third
option of audit D-05 (a durable outbox), which the maintainer's accepted
position for D-05 (2026-11-03) said to write down rather than build: the
delivery id and the dead-webhook policy first, then this.
**Author:** Tom F.

---

## Context

Push delivery today is a few tries held in memory (`push::sender`,
`handler::event_processing::background::push_delivery`):

* Each task has one lane, in memory, up to `push_queue_capacity` deliveries
  deep. When it is full the oldest delivery that does not end a turn is
  dropped (`dropped`).
* One delivery is up to three attempts, 1 s then 2 s apart, bounded by
  `push_delivery_timeout` (30 s).
* Nothing is written down. A delivery being retried when the process stops,
  or waiting in a lane, is not delivered later by that replica or another.
* Since 0.16.0 every request carries `A2A-Delivery-Id` (one id per event per
  webhook, the same on each attempt, not kept across a restart), a `410 Gone`
  removes the config, and a webhook that fails `push_suspend_after` deliveries
  in a row is suspended for a pause that doubles (audit D-05, option 2). The
  counts behind the suspension are per replica and in memory.

Spec §4.3.3 asks for at least one attempt per webhook and allows retries and
stopping after consecutive failures. It does not ask for durability, and a
receiver that must not miss an event reads the task (`GetTask`,
`ListTasks`) after a gap. So this is not needed to be conformant; it is what
an operator running a fleet would ask for when a receiver is down for an
hour, or a replica is rolled mid-delivery.

What a fleet already has that a delivery queue could use:

* the event log (`TaskStore::append_event`), per task, ordered, in the same
  database as the tasks on the SQL stores;
* turn claims (`a2a_task_claims`; the horizontal-scaling book's turn-claims
  page): a row naming an owner, with a lease the owner renews, so a dead
  owner's work is taken over after the time to live;
* `FOR UPDATE SKIP LOCKED` batch deletes (audit E-F21), the pattern for
  several replicas working one table without waiting on each other;
* the database's own clock for deadlines (audit E-F16, E-F17).

## Decision (proposed)

An outbox in the task database, worked by every replica, on the SQL stores
only and only when asked for. The in-memory stores keep the lanes as they are:
a process-local store has nothing to survive a restart with.

1. **One row per event and webhook**, written in the same transaction as the
   event-log append that produced the event, so an event that is stored has
   its deliveries queued, and one that is not stored has none. The row names
   the partition (tenant and caller), the task, the config, the event's
   position in the log and a delivery id; it does **not** carry the event
   body: the worker reads the event from the log by position, so a row is
   small and the body is stored once. The log's retention must outlive the
   rows that point into it (a row whose event is gone is dropped and counted).
2. **Claimed per webhook, in order.** A worker takes the oldest unfinished row
   of a `(partition, task, config)` and no other row of it while it holds
   that one, with `FOR UPDATE SKIP LOCKED` and a lease on the row, so a
   webhook sees a task's events in the order they were stored, from whichever
   replica works them, and a dead replica's row is taken over after its lease.
3. **Retries with backoff and jitter**, from about a second up to an hour,
   for as long as the row is younger than a maximum age (a default of
   twenty-four hours is a number to choose); then it is dropped and counted
   (`expired`). A `2xx`, or a refusal no retry will change, ends the row. A
   `410` removes the config and every row of it.
4. **The delivery id is the row's**, so it is the same across attempts,
   replicas and restarts: a receiver that sees an id twice knows it is a
   retry. Delivery is at least once, and the book says so.
5. **Bounds.** A per-webhook backlog cap (the lanes' `push_queue_capacity`,
   counted in rows), dropping the oldest row that does not end a turn, and a
   ceiling on the table across webhooks. A sweep, as for idempotency keys,
   deletes rows past their age.
6. **The dead-webhook state moves into the database** with it: consecutive
   failures and the suspension deadline become columns of the config row, so
   every replica agrees, and the per-replica memory of option 2 goes.
7. **Opt-in, additive schema.** A new table and the functions that work it,
   installed the way the claims table is (and with an upgrade path for a
   database an earlier release uses). A replica without the outbox keeps
   delivering from its lanes, so a rolling upgrade does not stop deliveries;
   it can deliver an event a second time, which the delivery id makes visible.
   A switch (`with_push_outbox`) turns it on; whether it becomes the default
   is a decision for a later release.

## Open questions

* Whether to fan out at write (one row per config at append time) or at read
  (one row per event, per-config progress in a second table). Fan-out at write
  is simpler and costs rows in proportion to configs times events; the
  per-task cap on configs (100 by default) bounds it, and E-F14's fix is what
  makes that cap hold across replicas.
* What the lease on a row is: a time to live renewed by the worker (claims'
  shape), or the length of one attempt plus a margin.
* What metric shows it: a gauge of rows waiting and the age of the oldest, and
  `retrying` as an outcome, so an alert can say "a webhook has been behind for
  an hour".

## Alternatives not taken

* **Keep the lanes and the policy of option 2.** Adequate where a push is a
  hint. It cannot survive a restart, which is the reason for this ADR.
* **An external queue** (a broker, a managed queue). Rejected by ADR 0002's
  dependency philosophy for the SDK itself; an operator can run one behind a
  custom `PushSender` today.
* **Reuse turn claims for deliveries.** A claim is per task turn and short; a
  delivery backlog outlives turns, and a task's webhook can be down across
  many.

## How it would be checked

The probe the audit wrote for it: with a database shared by two replicas and a
receiver that is down, queue deliveries on replica A, kill it with `kill -9`,
bring the receiver up, and watch the deliveries arrive from replica B, in
order, with the same delivery ids as the attempts that were lost. Then the
failure cases: the receiver down past the maximum age (the rows expire and are
counted), a `410` (the config and its rows go), two receivers of one task (one
slow does not hold the other), and a database that restarts mid-delivery.
Mutation tests for the claim and the ordering rule, and a PostgreSQL suite in
`run_postgres_suites.py`, as for the claims.

## Consequences

* A webhook that is down for hours gets its events when it returns, which the
  lanes cannot do, at the cost of rows in the task database and a worker per
  replica.
* Ordering per webhook is held by the claim, so one webhook that fails slowly
  holds back only itself.
* At-least-once is now a property, not a possibility: receivers have to
  deduplicate, as the spec already tells them to, and the delivery id makes it
  possible.
* A schema addition that older replicas ignore, and an opt-in switch, so it
  can be tried on a fleet before it is the default.
