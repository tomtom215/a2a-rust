<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0044: A lease renewal answers with the status alone, and one read checks every lease

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0 (maintainer's instruction of 2026-11-04).
Audit NEW-h-designs-3 and H5; the index decision of NEW-h-s0-8 is recorded
here too.
**Author:** Tom F.

---

## Context

A delegated child's lease (ADR 0020) is renewed by a `GetTask` that activates
the lease extension, every third of its time to live, per child. The server
reads the task, renews, and answers with the whole task: history, every
artifact, metadata. With a content signer (required by `Profile::Production`,
ADR 0027) it signs the status message and every artifact on the way out, and
the client then verifies all of it, only to read whether the child has
ended (NEW-h-designs-3).

On the worker, each leased turn had a watcher task of its own that asked the
lease store about its lease every quarter of its time to live, at most every
second. N leased turns on a replica cost the store N reads per interval
(H5).

## Measured before

* Lease checks: `handler::lease::cost_tests::n_leased_turns_cost_one_lease_read_per_interval`,
  50 leased turns of a 60 s lease (checked every second), the Tokio clock
  paused so the window holds exactly three intervals: **150 reads** of the
  lease store on the code before this change (the test's failure output:
  `left: 150, right: 3`). That is N per interval. The count does not depend
  on hardware; it was run on Linux 6.18, 4 CPUs, dev profile.
* Renewal answer: a `GetTask` with the lease extension answered the whole
  task (`a_renewal_only_read_renews_and_answers_with_the_status_alone`
  asserts that a renewal without the new URI still does: one artifact and
  the history, in the test's task).

## Decision

1. **A renewal can ask for the lease alone.** A second URI,
   `https://a2a-rust.com/extensions/lease/v1/renewal-only`
   (`LEASE_RENEWAL_ONLY_URI`), activated beside the lease extension, tells
   the server to renew and answer with the task's id, context and status, and
   no history, artifacts or metadata. The trim happens before signing, so a
   content signer signs the status message only. `A2aClient::renew_lease_status`
   sends both URIs, and a delegation's renewal loop uses it. A server that
   does not know the URI ignores it and answers the whole task; a client that
   does not send it gets the whole task from a new server. Both directions
   keep working, so it is not a break.
2. **One watcher per handler.** Every lease a handler's turns hold is checked
   in one call, `LeaseStore::lapsed_many`, per interval (the shortest any
   watched lease asks for, so no lease is checked less often than before).
   The default asks `lapsed` for each lease, which is right in memory;
   `PostgresLeaseStore` answers with one statement (a join on `unnest`). What
   each answer does is unchanged: a lapsed lease stops its turn as a halt
   does, a released one is no longer watched, an unreadable store stops
   nothing, and a turn's end releases its lease unless the turn is parked.

Options not taken: a new RPC method for renewal (all three bindings, and
not A2A); `historyLength: 0` alone (trims the history only, the artifacts
are still sent and signed); lease groups, one lease for a supervisor's
children (a wire change, and the renewal count is per child on the client
for a reason: each child may live on another server).

## Measured after

* Lease checks: the same test, **3 reads** in three intervals for 50 turns:
  one per interval, whatever N is.
* Renewal answer: `a_renewal_only_read_renews_and_answers_with_the_status_alone`
  asserts the lease was renewed and the answer has no artifacts, history or
  metadata; `a_renewal_asks_for_the_lease_alone` (client) asserts a
  delegation's renewal sends both URIs.

## What is unchanged, and why

* **Renewals per child.** The client still makes one `GetTask` per child per
  third of the lease: the children may be on different servers, and A2A has
  no batch read. Each now costs the server one task read, one lease update
  and the status alone.
* **Halt polling** is already one `HaltStore::load` per handler per interval
  (`handler::halt::sync`, `DEFAULT_HALT_POLL_INTERVAL`), not per task.
* **Journal writes** are already throttled per child: a position is written
  at most every two seconds (`PROGRESS_EVERY`), and only when it moved
  (`delegation::journal::write_progress`), so an idle child costs none.
* **Idle subscribers' polling** backs off already (audit E-F25a: 250 ms up to
  `subscribe_reattach_max_interval`, 4 s).

## Subscribers (added 2026-11-05)

Nothing bounded how many `SubscribeToTask` streams were open: each holds a
broadcast receiver and, while its task is quiet, reads the store every few
seconds. Two bounds are added to `HandlerLimits`, both refused before any
work as `Overloaded` (a retry delay on every binding) and given back when the
stream is dropped, however it ends (`handler::subscribers`):

* `max_subscribers_per_task`, default 256: the fan-out on one task.
* `max_subscribers`, default 10,000, over all tasks. Decided: needed as well,
  because nothing else bounds the total by default. Each subscriber is a
  connection or an HTTP/2 stream, but the server's `max_connections` is unset
  unless the operator sets it, and a per-task bound alone lets one client
  open streams on many tasks.

Both are additive and generous; a deployment that wants none sets them to
`usize::MAX`.

## The reaper's query on `tenant_tasks` (NEW-h-s0-8)

The audit conjectured a full table scan per replica per claim time to live.
Measured on PostgreSQL 16.15 (500,000 rows over 2,000 tenants, 50
`working`): an index scan of `idx_tenant_tasks_state` with `state = ANY(..)`
as its condition, 1,215 buffers, 3.0 ms, against a 163 MB heap and a 4.7 MB
index. No partial index now: one on `(updated_at) WHERE state IN
('TASK_STATE_SUBMITTED', 'TASK_STATE_WORKING')` would make the look
proportional to running tasks, for one more index write per write of a
running task, and pays at tens of millions of retained rows. The test
`store::claim::postgres_plan_tests` keeps the measured plan from regressing.

## The SQL stores' history append (NEW-h-designs-6)

`TaskStore::save_appending_history`'s docs said the SQL stores splice the new
messages into the stored document; none did. Measured before deciding, on
PostgreSQL 16.15, a task with 1.04 MB of history JSON (91 KB stored, TOAST
compressed), one message appended: `jsonb_set` in the database wrote 200,712
bytes of WAL, replacing the whole document 190,448, the same in three runs.
A JSON column is rewritten whole by any update, so a splice would save the
read and the transfer of the history, not the write, while repeating the
byte trim (`HistoryLimit`) and `save`'s write rules (fence, terminal state,
status columns) in SQL for four stores. The docs now say the SQL stores take
the default, and `store::task_store::sql_append_tests` checks what each of the
four leaves: the tail appended and trimmed, the snapshot's other fields, and
an insert when there was no record.
