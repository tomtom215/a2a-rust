<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Stores shared by replicas

When several replicas share one store, the store does more than hold rows: it keeps a cancelled task cancelled, tells a subscriber which events a stored task already reflects, and lets one replica at a time run a task's turn. The bundled SQL stores provide all three. Read this page to understand them, or to give a custom store the same guarantees. The deployment side is in [Running More Than One Replica](../../deployment/horizontal-scaling.md).

## Terminal states are final

Every store shipped here refuses a write — `save` or any of the delta
methods — that would move a stored `Completed`, `Failed`, `Canceled` or
`Rejected` task to a different state, and applies the check inside the write
itself: a `WHERE` condition on the SQL `UPDATE` or upsert, the write lock for
the in-memory stores. Re-writing the same terminal state is allowed. The
refusal is an `UnsupportedOperation` error carrying a `TerminalStateConflict`,
and the server reacts to it: `CancelTask` answers `TaskNotCancelable`, and the
event processors adopt the stored state and cancel the local executor.

This is what makes a `CancelTask` on one replica stick against an executor
still running on another (see
[Running More Than One Replica](../../deployment/horizontal-scaling.md)). A custom
store does not get it for free. To give the same guarantee, apply
`store::refuses_write(stored, written)` atomically with each write, and report
a refusal with `TerminalStateConflict::into_error`. A read followed by a write
is not enough — that is the race the rule exists to close.

## The log position a stored task reflects

A `SubscribeToTask` on a replica with no live queue for the task starts from
the stored task and replays the task's event log after the position that task
reflects. A processor logs each event before it stores the state the event
leads to, so on another replica the log's end can be one event ahead of the
stored task. Only the store can say which events a stored task already holds:
`TaskStore::get_with_position` returns the task and that position, read
together.

Every store shipped here keeps it. The processors run each event's writes
inside `store::AppliedPosition::scope(seq, …)`; a store reads
`AppliedPosition::current()` in each write method and stores it in the same
atomic write (the SQL stores' `applied_seq` column), keeping the stored value
when it is `None`. A write outside a scope, such as the send path's history
append, changes no logged event's effect, so it keeps the stored position.

Two cases it does not cover. A write that read the task before another
replica's write landed and saved over it keeps that write's position while
losing its effect; that lost update predates the position, and turn claims
(below) are what prevent it. And a replica on
an earlier release does not write the position, so during a rolling upgrade a
subscriber can receive events its snapshot already holds a second time.
Duplicates, not losses, and they stop once every replica runs this release.

A custom store that keeps a log should do the same, and override
`get_with_position` to read the task and its position together. One that does
not keeps the trait's default, no position, and a subscription falls back to
reading the log's end and then the task, which can lose the one event another
replica has logged and not yet stored, or repeat one it logged and stored
between the two reads.

The SQL stores add the column themselves: migration 8 (`SQLite`) and 7
(`PostgreSQL`) for `with_migrations`, and `from_pool` for a database an earlier
version created.

## Turn claims: one replica at a time per task

Several replicas sharing a store must not run one task at once, and a
replica that stalled must not write over the turn that another replica took
over meanwhile ([Running More Than One Replica](../../deployment/horizontal-scaling/turn-claims.md#one-turn-of-a-task-at-a-time-on-whichever-replica)).
The handler handles the first with a claim per turn, taken from
`TaskStore::turn_claims`, and needs the store for the second.

The SQL stores provide both. A custom store that several replicas share
should:

1. **Override `turn_claims`** to return a `store::TurnClaimStore` over storage
   every replica shares, and in the same database as its tasks, since the
   second step reads it. `PostgresTurnClaims::from_pool` and
   `SqliteTurnClaims::from_pool` are there to reuse when the store already
   runs on one of those. One of your own must renew only the tokens
   `renew` names, never every claim of the owner: a claim the handler no
   longer tracks has to expire.
2. **Fence every task write and event-log append.** Read
   `store::claim::current_fence()` at the start of each write method, before
   anything is spawned (it is a task-local, like the tenant). When it is
   `Some(token)`, make the write conditional, in the same atomic step, on that
   token still holding a claim (the SQL stores add `EXISTS (SELECT 1 FROM
   a2a_task_claims WHERE token = $n)` to the statement), and when it refuses,
   return `ClaimLost::new(task_id).into_error()`. A write with no fence (a
   `CancelTask`, which must win wherever the task runs) is not checked. On a
   database whose statements read a snapshot taken when they start
   (PostgreSQL's READ COMMITTED), lock the claim row in that condition (the
   PostgreSQL stores add `FOR KEY SHARE`), or a write that waited on the task
   row through a takeover passes on the claim as it was.

A store that takes the default, `None`, gets neither: the handler takes no
claims and refuses a second send to a running task only within its own
process. That is right for a store one process uses; with several replicas,
route every turn of a task to one of them. The guard that keeps a replica of
an earlier release off a task during a rolling upgrade
([Running More Than One Replica](../../deployment/horizontal-scaling.md#upgrading-a-fleet-to-this-release))
belongs to the bundled PostgreSQL stores; a custom store's own upgrade is its
own. `with_turn_claims` on the builder
supplies claims for a store that does not provide them, but the fence is still
the store's to apply. A wrapper that forwards to another store must forward
`turn_claims` (`scripts/check_store_delta_forwarding.py` checks the ones in
this repository).

The SQL stores create the claims table themselves: migration 9 (`SQLite`) and
8 (`PostgreSQL`) for `with_migrations`, and `from_pool` otherwise.
