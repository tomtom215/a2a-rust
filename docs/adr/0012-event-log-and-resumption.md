<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0012: An append-only event log, with the snapshot kept as the record

**Status:** Accepted

**Date:** 2026-09

## Context

A task's state is a **fold**: a stored snapshot, folded together with the
deltas that arrive after it. Until 0.13 that fold was the only thing kept.

Two measured consequences.

**A wrong fold was undetectable.** Issue #130 was a fold bug — artifacts from
one task appeared on another — and the only record of what had happened was
the folded result, which is to say the bug itself. There was nothing to check
it against.

**Intermediate states were unobservable.** In `mcp-bridge`'s demo the sample
agent emits three progress steps 120 ms apart and the MCP caller sees **one**,
because a poller can only ever observe the latest fold. A client that
reconnected got a fresh `Task` snapshot, not the events it had missed, so
"what happened while I was away" had no answer at all — §3.5.2 makes
reconnection an expected flow, and the SDK answered it with a state and a
shrug.

## Decision

Keep an **append-only log of the events the agent emitted**, beside the
snapshot, and make the snapshot a derived cache rather than the sole record.

1. **The snapshot stays authoritative for reads.** `get`, `list` and every
   response shape are unchanged. This is deliberately *not* event sourcing:
   state is not recomputed by folding the log on read.

2. **`seq` is a position, not a counter.** The key is `(task_id, seq)` and
   appends are `ON CONFLICT DO NOTHING`, so writing the same position twice
   leaves one row.

3. **The position is assigned once, in `InMemoryQueueWriter::write`, and
   carried on both channels** — the persistence channel that writes the log
   and the broadcast channel that feeds SSE.

4. **Frames the server synthesized carry no position.** The `SubscribeToTask`
   snapshot and the terminal frame the reattach hook rebuilds from stored
   state are not in the log, so `StreamEvent::seq` is an `Option`.

5. **SSE frames carry `id:`, and `Last-Event-ID` replays from it.** The offset
   is exclusive, so a client that has seen event *n* sends *n* and receives
   *n+1* onward. Replay is bounded by `HandlerLimits::subscribe_replay_limit`
   (default 1,000).

6. **The `TaskStore` methods default to refusing, not succeeding.**
   `supports_event_log` is what a caller checks.

7. **Every store this crate ships implements them.** Tables `task_events` and
   `tenant_task_events`, created by both the migration runners (SQLite 7,
   PostgreSQL 5) and each store's `from_pool` DDL. The tenant tables are keyed
   `(tenant_id, task_id, seq)` and carry `ON DELETE CASCADE`.

## Rationale

**A log, because a fold cannot check itself.** The log is the thing there was
nothing to check the snapshot against. It makes a wrong fold *detectable*,
gives a reconnecting subscriber the events it missed rather than a state it
must diff, and is the substrate anything like a signed execution receipt would
need.

**Positions rather than a counter, because a retried append must be safe.**
Idempotency by position is what lets an append be retried, or two writers
overlap, without a read first — the property `sqlite_store::journal` already
relies on. It also makes `last_event_seq` load-bearing rather than convenient:
a task parked at `input-required` and continued gets a *second* processor, and
a sequence restarting at 1 would collide with positions already written and be
swallowed as a replay, so the continuation's events would silently not be
recorded.

**Assigned in the writer, because the `id:` and the `seq` must be one number.**
The writer is the single fan-out point. Assigning there and carrying the value
makes the position a subscriber reads and the position the store holds
identical *by construction*. The alternative — counting frames in the SSE
layer — agrees with the log right up until the first lagged consumer, snapshot
frame, or failed append, and a resumption offset that is off by one drops an
event with nothing to indicate it.

**No position on synthesized frames, because a client sends the id back.**
An `id:` names an offset `read_events` can replay from. Giving one to a frame
that is not in the log would hand the client an offset that loses an event.

**Refusing defaults, because the failure must be loud.** Defaulting
`append_event` to `Ok(())` would advertise a log that silently lost every
event. A custom store that has not implemented these reports no log, which is
inconvenient; the alternative is a guarantee that is absent without saying so.

**A tenant column in the key, because an unscoped log is a cross-tenant read.**
Task ids are caller-supplied, so two tenants may legitimately use the same one.
An unscoped log would hand one tenant's resuming subscriber the other's
messages — message content, not merely a missed deduplication.

**Cascade here, unlike `tenant_idempotency_keys`, because the safe direction is
opposite.** A key that outlives its task keeps a retry from executing twice. An
event that outlives its task is replayed to whoever next claims that id.

## Consequences

- `EventQueueReader::read` yields a `StreamEvent` rather than a
  `StreamResponse`. Breaking, and taken in 0.13.0.
- A lagged SSE consumer now produces a **visible gap** in the numbering rather
  than a dense sequence that silently mis-numbers. A gap is the honest report.
- An append that fails **or records nothing** leaves a gap too, and is logged
  and counted under the `event_append` persistence-error label rather than
  failing the task. The log is a record of the run, not a precondition for it.
  Two kinds are distinguished: `event_append_error::POSITION_CONFLICT`, where
  a second writer already holds the position, and
  `event_append_error::TASK_ABSENT`, where the task is gone. A collision whose
  stored payload matches what was offered is a **replay**, not a loss, and is
  deliberately not counted — the comparison is on the serialized form, which
  is the log's own round-trip form, not on event identity.

  Worth stating plainly, because the original framing of position collision as
  purely a safety mechanism was too kind to it: a collision is also the one way
  this design loses an event without an error. The mitigation is observation,
  not prevention. There is still no database-backed lease, so two replicas
  numbering the same task independently remains possible; what changed is that
  it is now visible.
- **The log may not go back as far as a subscriber asks.** The in-memory log
  is bounded (`TaskStoreConfig::max_events_per_task`, default 512) and a
  persistent one is swept, so "resume after position N" can name a position
  that no longer exists. This is a different thing from the numbering gaps
  above, which are positions that were skipped: here the position was real and
  the record of it is gone. A store reports the earliest position it still
  holds (`TaskStore::earliest_event_seq`), a caller asks
  `TaskStore::event_log_covers`, and a resubscribe for a dropped position is
  served **from the snapshot** rather than as a partial replay — a replay that
  silently begins later than asked is a hole the subscriber cannot detect,
  because its own next `Last-Event-ID` would skip straight past it.
  Truncation never empties a log (a zero bound is floored at one), which is
  what keeps `earliest_event_seq` answerable.
- The log grows with the task and is reclaimed with it: by `delete`, by the
  foreign key, and — because `ON DELETE CASCADE` only fires with
  `foreign_keys=ON`, which a pool handed to `from_pool` may not set — by the
  SQLite retention sweep's anti-join, reported as
  `PurgeReport::orphan_rows_deleted`. That sweep ran only when a purge had
  deleted at least one task until 2026-09-20, which meant rows stranded by a
  partly-failed purge — whose earlier batches are already committed — survived
  until some later sweep happened to delete something. It runs every time now.
  `orphan_rows_deleted` is structurally zero on PostgreSQL: there is no orphan
  statement there rather than one that finds nothing, because Postgres has no
  per-session equivalent of `foreign_keys=OFF` and a declared cascade always
  fires.
- WebSocket, gRPC and SLIM streams carry no position. Resumption is the SSE
  binding's `id:`/`Last-Event-ID` pair; a spelling for the others would be a
  protocol extension this SDK invented.

## Alternatives considered

- **Full event sourcing — recompute state by folding the log on read.**
  Rejected for this round, deliberately rather than by omission: it is what
  would make #130-class bugs impossible rather than merely detectable, and it
  is a migration for every existing deployment. The log is the substrate it
  would need, so this stays open.
- **Count frames at the SSE layer instead of carrying a position.** Rejected:
  see the rationale above. It is the cheaper change and the one that fails
  silently.
- **Carry the position in `Message.metadata`.** Rejected: it would put a
  server-internal offset on the wire in a field the protocol reserves for
  callers, and it would not reach `StatusUpdate` or `ArtifactUpdate` frames.
- **Replay on any transport that sends a `last-event-id` key.** Not pursued:
  the non-SSE bindings have no way to return a new offset, so a client could
  resume once and never again.

## Revisit Trigger

- Adopters need to reconstruct state from the log rather than trust the
  snapshot — then the fold-on-read alternative above becomes the next step,
  with a migration.
- A transport other than SSE gains a standard resumption mechanism — then
  `StreamEvent::seq` is already there to carry.
- `subscribe_replay_limit`'s default proves wrong in practice: it bounds a
  client-supplied offset, and truncation is safe only because every replayed
  frame carries its own `id:`.
