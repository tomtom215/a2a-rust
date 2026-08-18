<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0011: Task retention is explicit, and the store does not schedule it

**Status:** Accepted

**Date:** 2026-08

## Context

The SDK ships two kinds of task store, and through 0.9.0 they disagreed about
how long a task lives — without saying so anywhere.

`InMemoryTaskStore` has always had retention: `TaskStoreConfig` defaults to a
one-hour TTL and a 10,000-task ceiling, and eviction enforces both. The
persistent stores — `SqliteTaskStore`, `PostgresTaskStore` and their
tenant-aware variants — take a connection URL and keep every task they are ever
given, forever. Nothing in the type signatures, the documentation or the
deployment chapter mentioned the difference.

**The divergence was the defect, not the growth.** Measured on this branch, a
task row costs 826 B on PostgreSQL and 781 B on SQLite, so a million tasks is
under a GiB — unremarkable for a database, and not an emergency for most
deployments. What is a real problem is that moving from the in-memory store to a
durable one silently changes the retention semantics of the application, in the
direction of unbounded growth, with no signal at the call site.

## Decision

Add `purge_expired` to all four persistent stores, and make it **opt-in**:

- `RetentionPolicy::new(terminal_max_age)` names the one thing a policy must
  say. `batch_size` defaults to 1,000; `max_batches` defaults to unbounded and
  can bound a single sweep.
- `purge_expired(&policy)` deletes **terminal tasks only** — the states
  `terminal_states()` reports, which is asserted against
  `TaskState::is_terminal` for every protocol variant — older than
  `terminal_max_age`, in batches, and returns a `PurgeReport` (`tasks_deleted`,
  `journal_orphans_deleted`, `batches`, `complete`).
- **The default is that nothing is deleted.** A store that is never asked to
  purge behaves exactly as it did in 0.9.0.
- **No timer runs inside the store.** Retention is driven by whatever already
  schedules work in the deployment — cron, a Kubernetes `CronJob`, a
  `tokio::spawn` loop.

The behaviour is stated in the module documentation, the deployment chapter and
the type itself, so the in-memory/persistent difference is now written down
rather than discovered.

## Rationale

**Opt-in, because the alternative is silent data loss on a durable store.**
Making the persistent stores match the in-memory one-hour TTL would delete
users' task history on upgrade, in a component whose entire purpose is that it
does not forget. A default that destroys data to fix a documentation gap is the
wrong trade.

**No internal timer, because the store does not know when your traffic peaks.**
A sweep that fires on its own fires whenever the clock says so, which is as
likely to be during peak load as not. The deployment already owns a scheduler
and already knows its quiet hours; `purge_expired` is a function it can call
from there. This also keeps the store free of a background task, a runtime
handle and a shutdown path it would otherwise have to own.

**Batched, because a single unbounded `DELETE` is a lock-duration hazard.** A
first purge on a store that has accumulated for months could touch a very large
number of rows; `batch_size` bounds each statement and `max_batches` lets an
operator cap a sweep's total work and finish it on the next run — which is what
`PurgeReport::complete` reports.

**Terminal-only, because a running task is not garbage.** Age alone is not
evidence that a task is finished; a long-running or resumable task can be older
than any sensible TTL and still be live.

## Consequences

- Deployments that want bounded growth must schedule the call. This is
  documented, but it is a step someone can skip — the trade accepted above.
- `journal_append` never touches the `tasks` row, so a task made terminal *via*
  an append would leave a stale `state` column that a sweep would read. This
  cannot happen on the real path, because the status transition that makes a
  task terminal is a full `save`. Recorded because it constrains any future
  change to the delta path.
- `purge_expired` returns `A2aResult`, not the driver's error type. The book's
  doctest gate caught the first version leaking `sqlx::Error` into callers'
  signatures.

## Alternatives considered

- **A background sweeper inside the store**, behind a feature flag. Rejected for
  now: it fires without knowing the deployment's load shape, and it obliges the
  store to own a task, a runtime handle and a shutdown path. Reconsider if users
  ask — it belongs behind a flag, not in the default path.
- **Match the in-memory TTL by default.** Rejected: silent deletion of durable
  history on upgrade.
- **Database-native TTL** (PostgreSQL partition dropping, SQLite triggers).
  Rejected: not portable across the two backends, invisible to the SDK's own
  tests, and it moves an application policy into schema that operators would
  have to maintain by hand.

## Revisit Trigger

- Users report that scheduling the sweep themselves is a burden — then add the
  optional background sweeper behind a feature flag.
- A future protocol revision adds a terminal state: `terminal_states()` is
  asserted against `TaskState::is_terminal` across `TaskState::ALL`, so the test
  fails rather than the sweep silently skipping the new state.
