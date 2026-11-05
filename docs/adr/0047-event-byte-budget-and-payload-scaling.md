<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0047: A process-wide byte budget over buffered events, and compaction of live tasks' logs; blob offload stays with the executor

**Date:** 2026-11-04
**Status:** Accepted (0.16.0; audit H13). Built: the byte budget
(`HandlerLimits::with_max_buffered_event_bytes`) and live-log compaction
(`RetentionPolicy::max_events_per_live_task`, `TaskStore::trim_event_log`;
amended 2026-11-05). Decided and not built: blob offload (option B) and the
SQL `save_appending_history` overrides; each is said below, with why.
**Author:** Tom F.

---

## Context

Audit H13 (`docs/audits/2026-10-29/H-swarm-scale.md` §13, re-read on
2026-11-01 and again on 2026-11-04) has four parts:

1. **No byte budget.** Each running turn has an event queue bounded by count
   (`DEFAULT_QUEUE_CAPACITY`, 256 events) and each event by size
   (`DEFAULT_MAX_EVENT_SIZE`, 16 MiB), with up to 1024 queues. Nothing bounds
   the product. An event stays in memory while any holder has it: a broadcast
   slot keeps it until every subscriber has read it (`streaming/event_queue/mod.rs`,
   `Shared`), and a subscriber that has stopped reading keeps up to 256 of them.
2. **Payloads in the row.** `raw` parts are base64 in the task document.
3. **SQL sends are O(history) twice.** `PostgresTaskStore` and
   `SqliteTaskStore` take the trait's default `save_appending_history` (read
   the task, append, write it whole); the trait's doc said they spliced the
   tail in (NEW-h-designs-6).
4. **The event log of a live task grows without bound.** Rows go only when a
   terminal task is purged.

Options from the triage (DECISIONS.md, H13): **A** a process-wide semaphore
over in-flight event bytes; **B** a `BlobStore` that rewrites `raw` parts to
`url` parts before persistence and broadcast; **C** compaction of a live
task's log to a snapshot event; and the SQL overrides, which need no decision.

## Decision

### A: a byte budget over buffered events (built)

`HandlerLimits::with_max_buffered_event_bytes(n)`, default `None` (no budget,
as before). With a budget:

* Each event a writer accepts is charged its serialized size (the size the
  `max_event_size` check already measures), clamped to the budget so one
  event never needs more than all of it.
* The charge travels with the event, inside the `Arc` both channels share,
  and is returned when the last holder drops it: the persistence processor
  after storing it, every subscriber after sending it, the broadcast slot once
  every subscriber has read it. So the budget bounds what is actually held,
  including what a stalled subscriber pins, without the queue having to
  guess when an event is done.
* A write that finds the budget spent waits for room, bounded by the
  writer's `write_timeout` (the bound a write already has for a full
  persistence channel), and then fails as that write fails: the executor is
  told, and the task fails rather than storing less than the agent wrote.
  The charge is taken before the event gets its log position, so a refused
  write leaves no hole in the log.
* A new turn is refused `Overloaded` (503, `Retry-After`) while less than a
  tenth of the budget is free, so new work does not take the last bytes from
  turns already running.

The test `tests/event_byte_budget.rs` measures it with a counting allocator:
a writer pushing large events at a subscriber that never reads, with and
without the budget.

### B: blob offload (not built here)

Rewriting `raw` to `url` inside the SDK changes what every reader receives,
needs a fetch path with the reader's own authorization, and needs content
signatures (ADR 0027) to cover the URL and a digest. The executor already
can emit a `url` part for a large payload, and is the one place that knows
where the bytes may live and who may read them. Decision: no SDK-side
rewriting; the book's payload page says to emit `url` parts above a size, and
`max_event_size` refuses what is too large. A `BlobStore` hook can be added
later without a break, should a deployment show the executor cannot do it.

### C: live-log compaction (built, amended 2026-11-05)

Compaction needs a store method that trims a live task's log below a
position and a resume contract (a reader from before the trim gets the
snapshot, not a silent gap). Decision: `RetentionPolicy::max_events_per_live_task`,
compacting to the stored snapshot, with resume from before the trim answered
by the snapshot frame as a lagged reader is today. As built:

* `TaskStore::trim_event_log(task_id, below)` drops the task's events below
  `below` and returns how many. Its **default is `unsupported_operation`**,
  as the other event-log methods default: a sweep that asks a store which
  cannot trim learns so, rather than reading `0` as "nothing needed
  trimming". Nothing in the handler calls it, so the default breaks no
  out-of-tree store. Implemented by the in-memory, tenant-aware in-memory,
  SQLite, tenant SQLite, PostgreSQL and tenant PostgreSQL stores; the
  traced, encrypted and audited wrappers forward it.
* `purge_expired` on the four SQL stores, with the policy set, finds the
  tasks whose logs hold more than `max` events and compacts each live one
  (`store::retention::compact`): the cut is below the oldest of the newest
  `max` events, never past the stored task's position plus one (so every
  event dropped is one the snapshot reflects), and never the last event, from
  which a continued task numbers on (`0` is taken as `1`). A terminal task is
  left whole (retention purges it by age), and so is a task stored without a
  position (what its snapshot holds is unknown). The tenant stores scope each
  task's partition themselves, since the sweep runs outside any request.
  `PurgeReport::events_compacted` counts the events dropped.
* The resume contract needed no new code: `earliest_event_seq` already
  reports the cut, `event_log_covers` answers `false` for a reader from
  before it, and the subscription answers that reader with the snapshot and
  a lagged live stream with `streamLagged`, as for the in-memory store's own
  bound. The in-memory store keeps that bound
  (`TaskStoreConfig::max_events_per_task`, 512 by default), which compacts
  its live logs on every append; the policy is for the SQL stores, whose
  logs had none.
* Not built: holds (`TaskStore::hold_events`) on the SQL stores. A stream
  that still owes events below a cut ends with `streamLagged` and resumes
  from the snapshot; the in-memory store spares them up to its ceiling.

Tests: `store::retention::compact_tests` runs one contract on all six stores
(PostgreSQL in a schema of its own): the newest ten kept, nothing past the
snapshot, terminal and unpositioned tasks whole, `event_log_covers` false
before the cut and true at it, and a task that writes 50 events between five
sweeps holding exactly its newest ten after each.

### The SQL `save_appending_history` overrides (not built here)

An override must leave the store holding exactly what the default would,
trim included. The trim is by message count and by bytes of serde's compact
JSON; PostgreSQL's `jsonb` text form is not that form (it spaces after `:` and
`,`), so a trim computed in SQL disagrees with the default on where to cut,
and a trim computed in Rust needs the history it was meant not to ship. The
trait's doc is corrected to say the SQL stores do not override it.

## Consequences

* Default behaviour is unchanged.
* With a budget, a stalled subscriber can no longer pin memory past it; the
  cost is that writers slow down or fail when the budget is spent, which is
  the point of a budget.
* B and the SQL overrides remain open and are listed for the maintainer; C is
  built (2026-11-05).
