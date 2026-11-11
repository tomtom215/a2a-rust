<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0037: Push notifications get a durable outbox, claimed per webhook, where the store is shared

**Date:** 2026-11-04
**Status:** Accepted and built in 0.16.0, amended 2026-11-05 and again
2026-11-11 (the amendments below are what was built; where they depart from
the proposal, they say why). Proposed 2026-11-04 as the third option of audit D-05, which the
maintainer's position for D-05 (2026-11-03) said to write down rather than
build; the maintainer's instruction of 2026-11-04 put it in 0.16.0 (H14).
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

## Amendment, 2026-11-05: what is built

`push::outbox` holds the trait and three stores, and
`RequestHandlerBuilder::with_push_outbox` turns it on. Without it nothing
changes: the lanes deliver from memory as before.

1. **`PushOutbox`**, with `InMemoryPushOutbox`, `SqlitePushOutbox` and
   `PostgresPushOutbox` (table `a2a_push_outbox`, created under the schema
   lock, additive). The in-memory one keeps nothing across a restart; it is
   there so a deployment and its tests run one code path whichever store
   they use, and it is what the proposal called "the lanes as they are" for
   a process-local store, with retries that outlast one delivery.
2. **Written after the event is stored, not in its transaction** (departs
   from decision 1; replaced for the events below by amendment 2026-11-11). The event log and the outbox are in different stores,
   which the handler composes but cannot enlist in one transaction without
   a cross-store API every `TaskStore` would have to implement. The record
   is written by the task's lane, in order, as soon as the event is stored:
   a crash between the two writes loses that event's deliveries. The window
   is the lane, not one write (corrected 2026-11-11: this said "one write"):
   the event waits in memory behind the rows queued before it, up to
   `push_queue_capacity`, and a slow outbox database widens it. Before the
   outbox it was every queued delivery and every retry.
3. **The event body is in the row** (departs from decision 1). Reading it
   back from the log by position would tie the outbox's life to the log's
   retention and to an API of every task store. The body is sealed with the
   store's `PushConfigCipher` when one is set (`with_cipher`), as configs
   are, bound to the row's tenant, task and delivery id; without one it is
   stored as the event log stores it.
4. **One row per event and config** (the open question: fan-out at write),
   carrying the tenant and caller the delivery runs as, the task, the
   config's id (not the config: credentials stay in the config store, and a
   config deleted or removed by a `410` ends its rows when they are next
   claimed), and a delivery id, sent as `A2A-Delivery-Id` on every attempt,
   from every replica.
5. **Claimed per webhook, in order.** A worker per handler claims rows whose
   due time has come and which have no earlier unfinished row of the same
   tenant, caller, task and config, with a lease (`FOR UPDATE SKIP LOCKED`
   on PostgreSQL; one `UPDATE ... RETURNING` on SQLite; a lock in memory). A
   lease is three times `push_delivery_timeout` plus 30 s (the open
   question): a delivery's wait for the gate, its webhook's time, a wait for
   an address and its time again fit inside it, so a lease that lapses
   belonged to a replica that is gone. At shutdown the worker releases what
   it holds at once. A row whose body cannot be read (sealed under another
   row's context or a key the store does not hold) is skipped by the claim,
   left in place and logged, rather than failing every claim; its webhook's
   later rows wait behind it until an operator acts.
6. **Retries.** A delivery that fails, times out or is throttled is due
   again after a backoff that doubles from 1 s to `push_outbox_backoff_max`
   (1 h), with up to a fifth of jitter; a suspended webhook's row waits the
   same way. A `2xx` ends the row, and so does a refusal no retry will
   change (a `4xx` other than `408` and `429`), as it ends a delivery today.
   A row older than `push_outbox_max_age` (24 h) when it is claimed is
   dropped and counted `expired`.
7. **Bounds.** At most `push_queue_capacity` rows wait per webhook of a task;
   recording one more drops the oldest that does not end a turn and is not
   claimed (`dropped`), as the lanes do.
8. **Metrics**: every attempt reports its outcome as today, and the new
   outcome `expired`; `PushOutbox::pending` counts what waits, for a gauge.

### Not built, and why

* **The dead-webhook state in the database** (decision 6). Suspension stays
  per replica (D-05): a suspended webhook's rows wait out the backoff, so a
  replica that has not seen the failures tries it again sooner, which is
  what spec section 4.3.3 asks for anyway. Sharing it is a schema of its
  own for a gain in politeness, not delivery.
* **A dead-letter table.** An expired row is counted and logged; the event
  itself is in the task's log, which the receiver reads with `GetTask` or
  `SubscribeToTask` after a gap, as the spec tells it to.
* **A ceiling on the table across webhooks.** The per-webhook cap times
  `max_push_configs_per_task` times the tasks with rows bounds it; a
  ceiling would have to choose whose events to drop.

## Amendment, 2026-11-11: the rows are reserved before the event is stored

An independent review of 2026-11-11 (X-08; audit H14) held that "the outbox
row in the event's transaction" was still open: amendment 1 wrote the row after
the event was stored, so the window was the time the event waited in its lane
(up to `push_queue_capacity` events, longer behind a slow outbox database), not
one write. What was built closes the window without putting both writes in one
transaction.

**Why not the event's transaction.** The event log is appended before the fold
for every event except a terminal one, which is appended after the store has
ruled on it; a row tied to that append would queue events the fold then
refuses. Tying the row to the task save instead would add an outbox statement
to every write statement of every SQL task store, on two databases, and would
hold only where the task store and the outbox share one. A reservation holds across any
pair of stores.

**What is built.**

1. `PushOutbox::reserve`, `arm`, `release` and `held` (default: unsupported;
   `supports_reservations` is false for an outbox that does not override
   them), implemented for the in-memory, SQLite and PostgreSQL outboxes behind
   one contract suite. A reserved row is **unarmed**: never claimed, never
   dropped to make room, and it holds its webhook's place, so the order of a
   webhook's deliveries is the order of the events whether or not they are
   stored yet. It carries the event's log position and a hold that the
   database's clock ends. Columns `armed`, `reservation`, `log_seq`,
   `hold_ms`/`hold_until` are added to a table an earlier build created.
2. The background processor, and the collector that folds a blocking send's
   events, reserve an event's rows (one per config of the task) **before the
   event's first durable write**, runs the fold in a scope
   in which `PushLanes::push` only notes that the state machine announced the
   event, and then **arms** the rows if it did and **releases** them if not
   (the store refused the event, the write failed, nothing is announced for
   it). The state machine's own decision, not a copy of it, says which.
3. A reservation whose hold has run out (three `store_write_retry` plus a
   minute: 150 s by default) was left by a process that stopped, or that stalled
   past any fold. Every replica's worker settles those every 10 s, asking the
   task store: the event was stored if the stored task has reached its log
   position (`get_with_position`) and the log does not hold a *different*
   event at that position (`read_events`); the log holding nothing counts as
   stored, because a terminal event is logged after its save and an event
   whose log write failed is never logged. Stored: armed and delivered, with
   the delivery id the reserver gave it. Not stored: removed, and no webhook
   is told of an event the task does not have. Each is counted
   `reservation_settled`.
4. A processor that was only slow, whose reservation the worker already
   removed because the store did not yet show the event, finds nothing to arm
   when its write completes and records the deliveries afresh, with new ids.
   This is the one path on which the same event can reach a webhook twice
   under two delivery ids (the first being the settlement's if the write had in
   fact landed before the worker looked): at-least-once, as before.

5. A task's ending that no queued event carried (the reaper's, an executor's
   panic, a verdict from another writer) is **recorded in the outbox before it
   is written to the log** (`PushLanes::record_now`, awaited). It used to be
   logged and then queued, and the reaper skips an ending the log already
   holds, so a stop in between left a log with the ending and a webhook that
   was never told. Now a stop in between leaves the log without it, which the
   reaper completes (at worst delivering it twice, under two ids). The reaper
   records through the outbox when the handler has one, which also puts its
   ending behind the rows a dead replica still has reserved: a live run
   delivered `failed` 40 s before an artifact the killed replica had stored
   when the reaper sent from its own loop.

**What it does not cover** (each is stated in the book's delivery page and
CHANGELOG): a store with no event log, and an outbox with no reservations,
keep amendment 1's after-the-store write; a task ended by an executor's panic
is saved before its ending is recorded, so a stop in that one write leaves a
terminal task with no row, which the reaper completes only where turn claims
are on (a single replica without them has nothing to find it);
an outbox that is down when an event
is reserved leaves that event to the lane, and a webhook can then see it after
a later event; a position reused by another turn after the reserver stopped
before logging is told apart only by comparing the event in the log with the
reserved one, so a log write that also failed lets the reservation be armed;
the settling worker's hold means a stopped replica's last event reaches the
webhook 150 s (by default) after the stop, and the ending behind it with it.

**How it was checked.** The contract suite on all three outboxes (PostgreSQL
live), including two replicas settling one reservation and a table from an
earlier build upgraded in place; the settlement's decision table, one unit
test per row; processor-level tests that abort a task at a chosen point of the
store write (before it, after it, a terminal event between its save and its
log write, stalled past the hold) with two replicas over one outbox; hand
mutants of each rule; and `scripts/live/outbox_live.py`, which runs two real
replicas on PostgreSQL and sends one `SIGKILL` at its `reserve` or its `arm`
(the event stored and its rows unarmed: delivered once by the survivor, 71 s
after the kill, before the reaper's ending; the event not stored: never
delivered).

