<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0057: A waiting subscriber is woken by an append, by opt-in; there is no separate read pool

**Date:** 2026-11-07
**Status:** Accepted for 0.16.0 (the maintainer put E-F25 (b) and (c) in scope
on 2026-11-07; the best engineering option is built and can be vetoed in
review). Audit E-F25 (b) built; (c) measured and not built.
**Author:** Tom F.

---

## Context

A `SubscribeToTask` stream on a replica that does not run the task has no event
queue to read. Its reattach hook reads the shared store (`get_with_position`,
`last_event_seq`) and waits between reads. E-F25 (a) made the wait double while
nothing changes, from `subscribe_reattach_interval` (250 ms) to
`subscribe_reattach_max_interval` (4 s): an idle subscriber's cost fell from
about eight statements a second to one poll of two statements every 4 s, and
the first event after a long quiet became up to 4 s late. (b) asks to remove
that latency without paying the polls again, by telling the stream when the log
changes; (c) asks for a small separate pool for these reads so that idle
subscribers cannot starve foreground requests.

## Decision: (b), built

1. **A defaulted trait method.** `TaskStore::watch_event_log(&TaskId) ->
   Option<LogWatch>`, `None` by default. `LogWatch` is a future that resolves
   to a `LogWake` (`Notified`, or `Resynced` when the store lost its source of
   notifications and has it again). It is **registered when the method returns**,
   not when first polled: the hook creates one, reads the store, and only then
   waits, so an append between the read and the wait is not lost. The method is
   synchronous, as `hold_events` is, and forwarded by `AuditedTaskStore`,
   `TracedTaskStore` and `EncryptedTaskStore` in the same commit, each with a
   test that the watch is the wrapped store's own; the three
   "overrides every method" tests parse the trait, so they cover it, and their
   thresholds were raised by one.
2. **The hook waits on the watch and on its timer.** A store that returns
   `None` (SQLite, in-memory, a custom store, a PostgreSQL store not opted in)
   takes the `sleep` it always did: same schedule, no metric. The timer stays as
   the fallback at the existing back-off ceiling, so a lost notification costs
   one wait.
3. **A wake never makes a stream read faster than once per interval.** After a
   wake, the next read waits until `subscribe_reattach_interval` has passed since
   the last one (`pace`, kept across the hook's calls). Without it a task that
   streams ten events a second would cost each cross-replica subscriber, by the
   code's own count, a poll, a replay and a second poll per event (two to four
   statements each), more than the sixteen a second polling made at 250 ms; that
   was reasoned from the code and not measured. With it the worst case is
   polling's. The price
   is that a second event a few milliseconds behind a first reaches the stream up
   to one interval later (measured: 260 ms for the artifact and the terminal
   status written together, on the 250 ms default); the first event after a quiet
   is not held at all.
4. **PostgreSQL announces in the append's own statement.** With the opt-in the
   append is a data-modifying CTE:
   `WITH ins AS (INSERT ... ON CONFLICT DO NOTHING RETURNING 1) SELECT
   pg_notify($5, $6) FROM ins`. One statement, so `tests/statement_count.rs`
   still reads 8 and 11 (a second test pins the announcing store at the same
   numbers). The statement's command tag is `SELECT n`, `n` the rows inserted,
   and sqlx's `rows_affected()` reads it, so `0` still means "nothing was
   written" and `append_event`'s conflict and fence handling is untouched; a test
   against the live database appends the same position twice (1 then 0) and
   checks that a replay is not counted, a different event at the position is, and
   neither notifies. The payload is `{partition length}:{partition}{task}`:
   length-prefixed so no two pairs share it whatever characters they hold, and
   looked up as it arrives, with nothing to parse. It is a bind parameter, never
   interpolated. It is at most 512 bytes against PostgreSQL's limit of 8,000, and
   a task whose key is longer (a task id is chosen by a client on a continuation)
   is **not announced and not watched**: its append runs the plain statement and
   its stream polls. A non-announcing store runs `APPEND_SQL` unchanged, so the
   default is byte-for-byte what it was.
5. **One listener per store, started by the first waiting stream, waking every
   waiter after a reconnect.** It is the halt store's design (ADR 0024): a
   `PgListener`, a one-second reconnect delay, and `resync` to every waiter once
   it is listening and after every lost connection, because what was announced
   meanwhile is gone and a read recovers it. A notification costs the listener
   one hash lookup, and a task nobody waits on, none beyond that. The registry
   holds an entry only while a stream waits on it.
6. **Opt-in, and the pooler story is the halt store's.** Reasons it is not the
   halt store's default:
   - The halt store is built by an operator who names its pool and listens
     unless told not to. The task store is built by everyone, most behind a
     pooler. A default `LISTEN` there has the failure seen live on 2026-10-30:
     in transaction mode the notification arrives in the middle of another
     client's statement and sqlx fails it with `unexpected message:
     NotificationResponse`. An upgrade that does that to a deployment that did
     nothing is the thing this project's maintainer does not ship.
   - PostgreSQL takes a lock at commit that serialises every transaction that
     notifies, so a `NOTIFY` in the hottest write there is costs something at
     high append rates, to a deployment whose subscribers all live on the
     replica that runs the task and gain nothing.
   `LogListen::Pool` takes one connection of the store's own pool;
   `LogListen::Direct(url)` opens one of its own to PostgreSQL, which is the
   answer behind a pooler; a store not opted in listens to nothing. `Debug` does
   not print the URL.
7. **Metric.** `Metrics::on_subscribe_wake(cause)`, counter
   `a2a.server.subscribe.wakes`, `cause` one of `notification`, `resync`,
   `timeout`, reported only for a store that returned a watch (a store that
   cannot wake a stream reports no wake). Defaulted, forwarded by `Arc` and
   `Tee`, exported by `OtelMetrics` and `GlobalOtelMetrics`
   (`check_otel_metrics_coverage.py` passes).

### The terminal state: a wake at the append is enough where the log has the event

The processor logs an event before it stores the state the event leads to. A
wake at the append of the terminal event finds the stored task not yet terminal,
finds the log past the stream's position and returns `Logged`; the reader
replays the terminal event and ends the stream without waiting for the save.
Measured (`tests/log_wake_e2e.rs`, two handlers over two stores on a local
PostgreSQL 16, test profile, a machine at a load average above 17): an event
written on replica A ended a stream's 8 s wait on replica B, with the artifact
and the terminal status written together, in 260 ms (the notification, plus the
interval's pace for the second event). One run each, by `--nocapture` output, not
a distribution.

What a wake at the append does not cover is a status written with **no event**:
a `CancelTask` of a parked task on another replica (the cancel writer is
throwaway, and the status is saved by `save_status_delta`), a task the claim
reaper ends. Measured: a cancel of a parked task, with the ceiling at 2 s, ended
the stream on the other replica at 2.01 s, the whole of its nap, by the poll. That
is what E-F25 (a) already gave, not a regression, and it is bounded by the ceiling
(4 s by default).

Notifying at status saves was not built. Making the saves announce means a
`pg_notify` in each of the five statements that write status (`save`,
`save_status_delta`, `try_save_status_delta`, `save_appending_history`,
`insert_if_absent`), each guarded by the terminal fence and the claim machinery
and pinned by `statement_count`, and a notify for every status change of every
task, where only a terminal one matters; the gain is at most the ceiling on two
rare paths. A trigger on `tasks` would avoid editing the statements and would
run for every writer, replicas that did not opt in included. A separate
announcement after a terminal save is one more statement per finished task. If
cancel latency on another replica matters to an adopter, the smallest correct
step is a `publish_cancel`-style announcement from `CancelTask` (the halt store
already carries one for the executor), not a change to `save`.

## Decision: (c), a separate read pool, not built

MEASUREMENTS_C

## Consequences

- With the default, nothing changes: no `NOTIFY`, no `LISTEN`, the same append
  statement, the same sleeps. The SQLite and in-memory stores are untouched.
- A deployment that opts in holds one connection per replica (the listener's)
  from the first cross-replica stream, and gives its idle cross-replica
  subscribers the latency of a notification instead of the ceiling.
- Every in-repo `TaskStore` wrapper forwards the method; an out-of-tree wrapper
  that does not is a store that cannot push, which is the behaviour before.
- `LogListen` and the new `Metrics` method are additive; `LogWake` is
  `#[non_exhaustive]`.

## Alternatives considered

- **A broadcast channel of every notification, filtered by each stream.**
  Simpler, and a wake for every stream on every append in the fleet: at a few
  thousand subscribers and a few hundred appends a second that is a million
  wakeups a second to learn nothing. A per-task registry costs one lookup.
- **Return a cursor and wait on it** (`wait_for_change(task, since)`).
  Race-free too, and it needs the stream to carry store state across polls and
  the trait to define what "since" means for each store. A watch registered
  before the read is the same guarantee in a type the stream just holds.
- **Listen at construction.** Eager, as the halt store is. A replica that
  serves no cross-replica stream would hold a connection for nothing, and
  `with_log_notifications` would have to be async or need a runtime.
- **Announce by trigger.** One place instead of two statements, and it runs for
  replicas that did not opt in, adds DDL to a table every release has created
  the same way, and cannot be switched off per store.
- **On by default.** See decision 6.

## Revisit trigger

- A deployment reports `a2a.server.subscribe.wakes{cause="timeout"}` far
  outnumbering `notification` with the opt-in on: notifications are not
  arriving (a pooler, a replica without the opt-in).
- Cancel latency across replicas matters: announce from `CancelTask`.
- The measurement of (c) at a subscriber count it was not run at shows a
  foreground effect.
