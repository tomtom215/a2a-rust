<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0019: A halt store every replica follows

**Date:** 2026-10-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

`RequestHandler::halt` refuses a scope's new sends and fires the
cancellation token of its running tasks. It held in the process that called
it and nowhere else. A deployment of N replicas had to be halted N times,
and a replica that restarted came back unhalted. The control map's
AIA-14-4e row (EU AI Act Art. 14(4)(e), a "stop" that brings the system to
a halt) and ASI10 row (rogue agents are stopped) were `partial` for that
reason, and `ROADMAP.md` committed to "a shared store for halts, read at
admission".

## Options

1. **A halt store read on every send.** Admission asks the store each time,
   so a halt anywhere refuses the next send everywhere. That adds a store
   round trip to every request, and it still does not stop tasks that are
   already running on other replicas: a replica with no traffic never
   reads. A poll is needed anyway.
2. **A halt store written through and polled.** `halt` and `resume` write
   the store. Each replica reads it before admitting its first send, and
   then on an interval. Admission reads the process's own map, as before. A
   halt given elsewhere takes effect within one interval. A send costs
   nothing extra.
3. **Push through PostgreSQL `LISTEN`/`NOTIFY`.** Lower latency, but
   PostgreSQL only. It holds a connection per replica, and a missed
   notification (a dropped connection) needs a poll behind it.

## Decision

Option 2, with a one-second default interval
(`RequestHandlerBuilder::with_halt_poll_interval`; zero is refused).

- `HaltStore` has three methods: `load`, `put` and `remove`. It ships as
  `InMemoryHaltStore` (handlers in one process), `SqliteHaltStore` (one
  process across restarts) and `PostgresHaltStore` (replicas). The SQL
  tables key a scope as `all` or `tenant:<name>`. A row whose key names no
  scope fails the load: skipping it would resume what it halts.
- **Restart.** Before a handler with a store admits its first send, it
  reads the store and starts its poller. While the store cannot be read,
  sends are refused (`Halted`, "the halt store could not be read"): a
  process that cannot tell whether it is halted does not start work.
- **Another replica's halt.** A poll that finds a scope newly halted stops
  the tasks it covers here, exactly as a local halt does. A poll that finds
  a scope gone resumes it here.
- **A halt the store refused.** The halt still holds here at once, since
  stopping must not wait on a database. It is kept as unwritten, counted as
  in force by every poll, and written again on each one until the write
  succeeds. `HaltReport::persisted` says `Some(false)`. Without this, the
  next poll would read a store that lacks the halt and silently lift it.
- **A resume the store refused** changes nothing and returns
  `ServerError::Internal`. The halt still holds here and everywhere.
  `resume` now returns `ServerResult<bool>`. It was unreleased, so this is
  not a breaking change.
- Polls, halts and resumes hold one async lock across their store round
  trips. A poll that read the store before a resume therefore cannot apply
  what it read after it, which would halt the scope again on stale data
  and fire its tasks' tokens. Admission does not take the lock.
- The poller holds the handler's state weakly and ends when the handler is
  dropped.

## Consequences

- A halt reaches every replica sharing the store within one poll interval,
  and on the replica that gave it at once. A restarted process comes back
  halted before it admits anything.
- What stays open: deciding to halt is still a person's or a monitor's. An
  executor that ignores its token keeps running wherever it runs. A cancel
  (not a halt) on one replica still reaches another replica's executor only
  through its next refused write. Those parts of AIA-14-4e and ASI10 remain.
- Each replica does one store read per interval: 3,600 an hour at the
  default, each reading the whole table, which holds one row per halted
  scope.
- Tests: `store::halt::tests` runs the store contract against all three
  stores, the PostgreSQL one at `A2A_TEST_POSTGRES_URL`.
  `handler::halt::store_tests` covers two replicas, a restart, a refused
  write, a refused read, a refused resume, the stale-poll race, the poller
  ending, and the poll that first reads a halt stopping exactly what it
  covers.
