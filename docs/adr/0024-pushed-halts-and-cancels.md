<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0024: Halts and cancels are pushed to every replica

**Date:** 2026-10-21
**Status:** Accepted
**Author:** Tom F.

---

## Context

ADR 0019 made a halt reach every replica sharing a `HaltStore`, but only
within one poll interval (one second by default). It left a cancel on one
replica reaching an executor on another only through that executor's next
refused write, which may never come for an executor that is waiting on a
model or a tool. The control map's AIA-14-4e row (EU AI Act Art. 14(4)(e),
a "stop" that brings the system to a halt) and ASI10 row (rogue agents are
stopped) stay `partial` for both.

ADR 0019 rejected push on its own because it is PostgreSQL only, holds a
connection per replica, and needs a poll behind it for missed
notifications. The maintainer chose on 2026-10-21 to add it, with the poll
kept behind it: PostgreSQL `LISTEN`/`NOTIFY`, with SQLite and in-memory
stores polling as before.

## Decision

- **The store pushes.** `HaltStore` gains two provided methods, so an
  existing implementation still compiles and keeps polling:
  - `signals()` returns a `tokio::sync::broadcast::Receiver<HaltSignal>`,
    or `None` (the default) for a store that cannot push.
  - `publish_cancel(tenant, task)` tells every replica that `task` in
    `tenant` was cancelled. The default does nothing.

  `HaltSignal` is `Changed` (some halt was written or removed: read the
  store now) or `Cancel { tenant, task }`.
- **PostgreSQL.** `put` and `remove` run their statement and
  `pg_notify('a2a_halts', '')` in one transaction, so a notification goes
  out exactly when the write commits. `publish_cancel` sends
  `pg_notify('a2a_cancels', <JSON {"tenant","task"}>)`.

  `PostgresHaltStore::from_pool` starts one listener per store (a
  `PgListener` on the pool), which forwards both channels to the broadcast.
  The listener ends when the last clone of the store is dropped. When it
  loses its connection, it sends `Changed` on reconnecting, because
  notifications sent meanwhile are lost and a read recovers them.
- **In memory.** `InMemoryHaltStore` broadcasts the same signals to every
  handler holding a clone, so the behaviour is testable without a database.
  SQLite keeps polling.
- **Receiving.** The handler's poller waits on its tick *or* a signal:
  - On `Changed`, or when it lags behind the broadcast, it reads the store
    at once, exactly as a poll does, under the same lock.
  - On `Cancel`, it fires the cancellation token of that task, if the task
    runs here in that tenant. It fires the token only and does not mark the
    turn halted. The replica that took the `CancelTask` has already written
    `Canceled` to the store, and a second terminal write from this replica
    would be refused as an invalid transition.
  - The tick stays, so a lost notification costs at most one interval, as
    before.
- **Sending a cancel.** `CancelTask` on a replica that does not run the
  task (no local token) publishes the cancel after its own work. A publish
  that fails is logged and does not fail the cancel: the store already
  says `Canceled`, and the executor's next write is still refused, as
  today.

## Consequences

- A halt reaches every replica sharing a PostgreSQL store about one
  notification round trip after it commits, not after one poll interval.
  A cancel reaches the executor running the task on another replica by
  the same path, where before it was reached only through a refused write.
- What stays open on AIA-14-4e and ASI10:
  - An executor that ignores its token keeps running.
  - Streams open on the replica that ran the task end when its executor
    returns, without the `Canceled` frame. A `GetTask` there reads
    `canceled`. Delivering the frame across replicas needs a store-side
    terminal event and an independent review of the store contract; it is
    not done here.
  - SQLite and in-memory deployments across processes still poll.
- Each PostgreSQL replica holds one more connection, the listener's.
- Tests:
  - the store contract for signals: in memory always, PostgreSQL at
    `A2A_TEST_POSTGRES_URL`;
  - two handlers sharing a store, where a halt stops the other's task well
    inside one poll interval (the interval set to an hour, so only the push
    can explain it);
  - a cancel on one handler stopping the executor on the other;
  - a lagged receiver falling back to a read;
  - a store without signals still polling.
