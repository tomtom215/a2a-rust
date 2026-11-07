<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0056: A parked task may expire, by opt-in, and is deleted when it does

**Date:** 2026-11-07
**Status:** Accepted for 0.16.0 (the maintainer put the item in scope on
2026-11-07; the best engineering option is built and can be vetoed in
review). Audit D-18, left to the maintainer when it was fixed (part).
Amends [ADR 0011](0011-task-retention.md).
**Author:** Tom F.

---

## Context

A task parked in `input-required` or `auth-required` is a person's pending
decision. Two settled rules say it is never removed:

- the in-memory store's capacity eviction never takes one, and a store at
  `max_capacity` holding nothing else refuses a new task with a temporary
  error (audit D-18, `refuses_for_parked_tasks`);
- the SQL stores' `purge_expired` deletes terminal tasks only; ADR 0011 and
  `store::retention` say a parked task is "a workflow waiting on someone, not
  a leak".

Both rules are right for a wait someone will answer. Neither has an answer for
one nobody will: the in-memory store then fills until it refuses all new work
(RAM is a hard bound there, which is why that store evicts under pressure at
all), and a SQL store keeps the row and its event log for ever. The audit left
"expire parked tasks after a long idle time" to the maintainer, who has now
included it. The default must stay as it was: nothing expires.

## Decision

1. **In memory:** `TaskStoreConfig::with_parked_task_ttl(Duration)`. A parked
   task whose last write is at least the TTL old becomes evictable like a
   finished one: the TTL pass (every `eviction_interval` writes, or
   `run_eviction`) removes it, the capacity pass takes it with the finished
   tasks, a new task is not refused for a store full of expired ones, and a
   tenant partition's shared-budget admission may free its slot. The age runs
   from the last write, inclusive ("at least"), exactly as `task_ttl` does for
   finished tasks. Unset (the default), no code path changes. Each removal is
   counted (`InMemoryTaskStore::parked_evictions`) and the TTL pass logs it.
2. **SQL stores:** `RetentionPolicy::with_parked_max_age(Duration)`, unset by
   default, and a second sweep in `purge_expired` of all four stores
   (`SqliteTaskStore`, `TenantAwareSqliteTaskStore`, `PostgresTaskStore`,
   `TenantAwarePostgresTaskStore`), in the same batches, under the same
   `max_batches`, counted in a new `PurgeReport::parked_tasks_deleted`. The
   terminal sweep runs first, then the parked one, then the keys. A parked
   task is older than the policy when its `updated_at` is strictly older, by
   the database's clock, as for a terminal one. Whole seconds, as
   `terminal_max_age`.
3. **Delete, not cancel.** Moving a parked task to `canceled` would need what
   `CancelTask` writes: the status event a stream replays, the push
   notification to the task's webhook, the audit record, and the executor's own
   `cancel` hook. A store-level purge has none of them, and a task that says
   `canceled` over a history that does not is worse than an honest deletion.
   So the sweep deletes, with the event log (cascade), as it does a terminal
   task. A client that answers the task afterwards is told it was not found;
   a stream open on it ends without a terminal state (the reattach hook reads
   "gone" as the end). A handler-level "cancel what has been parked too long"
   that does write all of that is possible and not built; see *Alternatives*.
4. **A live turn claim protects the task.** A terminal task is never written
   again, so its sweep cannot race a writer. A parked one is continued by the
   next message, which takes the task's turn claim before it writes. The
   parked sweep therefore skips a task with an unexpired claim in
   `a2a_task_claims` (scope `''`, or the tenant for the tenant stores), in
   the same statement as the delete, and locks the row (`FOR UPDATE OF t SKIP
   LOCKED` on PostgreSQL) so a continuation that has written meanwhile is
   passed over. Not closed: the instant between a continuation reading the
   task and taking its claim. A sweep landing there deletes the task and the
   continuation's save then creates it again from what it read, without the log
   that went with it. The parked age is hours or days; the window is
   milliseconds; it is recorded here and in the code, not claimed away.
5. **A key outlives the task it names.** `effective_idempotency_key_max_age`
   was never below `terminal_max_age`; it is now never below `parked_max_age`
   either, so the retry of the send that created a parked task replays or is
   told the task is gone, and never creates a second.

## Rationale

**Opt-in, because the first rule is the protection of a person's decision.**
A default that deleted a pending approval after an hour would be the same
silent data loss ADR 0011 refused for terminal tasks, in the one place the
data cannot be recreated by re-running work.

**One TTL setting per store family, not one shared knob.** The in-memory
store's bound is memory and its rule is "evict under pressure and on a TTL";
the SQL stores' is "delete only when told", driven by the operator's
scheduler. The two settings sit where each family already keeps its other
age (`task_ttl`, `terminal_max_age`), with the same units and the same
inclusive/strict comparison each uses for its finished tasks, so a parked
and a finished task of the same age expire together in the same store.

**Counted apart, not added to `tasks_deleted`.** Finished work being
forgotten is the healthy reading of the sweep; a wait nobody answered is
something an operator watching it wants to see move on its own, in the
report and (in memory) on the counter and in the log.

**The capacity pass finds expired parked tasks within its scan window, and the
TTL pass finds the rest.** The capacity sweep walks the oldest 1,024 tasks by
status time, as it does for finished work, so that a full store pays O(1) per
write. An expired parked task beyond that window is removed by the next TTL
pass; in the meantime the store may sit a few writes over its cap (at most
`eviction_interval`). A scan keyed by last write would make every write to a
full store walk the expired prefix.

## Consequences

- With the default, nothing changes: the existing "never evicted / never
  purged" tests pass unchanged apart from the new `None` argument of an
  internal method.
- A deployment that sets either age accepts that an answer arriving later
  meets `TaskNotFound`. The book (`sustained-load.md`, `configuration.md`)
  says so beside the setting.
- `PurgeReport` and `RetentionPolicy` are `#[non_exhaustive]`, so the new
  field and the new option are additive (STABILITY.md section 4).
- The in-memory store's expired parked tasks are removed with their event
  log (it lives on the entry), so a stream attached to one ends at the next
  reattach.
- No push configuration is removed: the push-config stores are keyed by task
  id and are swept by their own owners, as they are for terminal tasks.

## Alternatives considered

- **Transition to `canceled` in the store.** Rejected: see decision 3.
- **A handler-level sweep that cancels what has been parked too long**,
  writing the event, the push and the audit record. The better behaviour for a
  client that is still listening. It needs a place to run (the handler has no
  timer, by the same argument as ADR 0011) and a way to ask the executor to
  cancel a task that has no running turn; not built, and the store-level
  deletion does not preclude it.
- **Expire parked tasks only under capacity pressure** in memory. Rejected:
  the TTL pass is what makes `parked_task_ttl` a TTL; a store that is not
  full would keep abandoned waits for ever and only drop them in a burst.
- **Reuse `terminal_max_age` for parked tasks** on the SQL stores. Rejected:
  the two ages answer different questions (how long finished work is worth
  reading back, how long a person may take to answer), and one value
  cannot be right for both.
- **Delete without the claim check.** Simpler and wrong for a continuation in
  flight; the check costs one `NOT EXISTS` over an indexed primary key.

## Revisit trigger

- Adopters want the cancellation to be seen by listening clients: build the
  handler-level sweep above.
- `PurgeReport::parked_tasks_deleted` is non-zero in a deployment that has
  not set `parked_max_age`: something is wrong, since the field is zero
  without it.
