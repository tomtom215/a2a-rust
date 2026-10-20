<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0017: The context lookup lists without history; `stored_task` stays whole

**Date:** 2026-10-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

Every send resolves its context to a task with
`RequestHandler::find_task_by_context`. It calls `TaskStore::list` for up
to ten tasks in the context, picks a live one over finished ones, and the
task it picks becomes the executor's `RequestContext::stored_task`, which
is documented as "the previously stored task snapshot". `stored_task` is a
`pub` field of a `pub` struct.

`list` returned every candidate whole, so a context holding ten tasks at
the history cap (1,024 messages) copied ten histories to use one.
`docs/swarm-scale-findings.md` measured this stage at 511 µs per send at
the cap, the largest remaining O(history) cost on the send path, and left
it open because shrinking the result changes what every executor sees.

Measured on 2026-10-18 with `context_lookup_cost`
(`crates/a2a-protocol-server/src/handler/context_lookup_cost.rs`). Each
task holds 1,024 history messages of 100 bytes and one 64 KiB artifact.
The build is release, on a shared 4-vCPU Intel Xeon at 2.80 GHz. Each
figure is the median of 201 lookups; each range covers three runs.

| context | before | after |
|---|---|---|
| one task | 132–146 µs | 135–277 µs |
| ten tasks (one live, listed last) | 3.21–3.73 ms | 364–552 µs |

The machine is noisy: the one-task range after the change spans 2x with
the code unchanged on that path.

## Options

1. **Keep `stored_task` whole; list without history.** The lookup asks
   the store for `history_length: Some(0)` and reads the chosen task whole
   by id. A store that ignores `history_length` returns whole tasks; one
   of those is used as it is, with no second read. No API change.
2. **Serve `stored_task` without history** and give executors a way to
   load it. This removes the last copy, the one-task row above. But it
   silently changes the meaning of a public field. An executor that reads
   `stored_task.history` would get `None` and carry on, with no compile
   error and no runtime error.
3. **Make the view configurable** (`RequestHandlerBuilder` option: whole,
   last *n*, none), defaulting to whole. It is the only way to remove the
   one-task copy without a silent change, at the price of a new option and
   a per-store way to read a task with a bounded history. `TaskStore::get`
   takes no such parameter, so a new trait method is needed, and with it
   CONTRIBUTING's "A default is not free" obligations.

## Decision

Option 1.

- `find_task_by_context` lists with `history_length: Some(0)`. It chooses
  exactly as before: the first live task listed, else the first task
  listed.
- If the chosen task came back with a history, the store did not trim, and
  the task is used as listed. If it came back without one, it is read
  whole with `get`. A task that truly has no history pays that second read.
  A task deleted or evicted between the two reads is treated as absent,
  which is what the listing would have said a moment later.
- `include_artifacts` is not used to trim. A store that trimmed artifacts
  but ignored `history_length` would hand back a task with a history and
  no artifacts. That task would pass the "came back with a history" test
  and reach the executor incomplete.
- `TaskStore::list`'s documentation says a store may trim each listed
  task's history to `history_length`, since the caller trims to the same
  length anyway. `InMemoryTaskStore` (and so the tenant-aware in-memory
  store, which holds one per tenant) trims while copying. The public
  `ListTasks` handler is unchanged and trims again.
- `RequestContext::stored_task` keeps its type, its visibility and its
  meaning: the stored task, whole.

## Consequences

- A context with many tasks no longer costs a history copy per task: in
  the ten-task row, 3.2–3.7 ms became 0.36–0.55 ms.
- A context with one task still costs one whole copy (the one-task row).
  That copy is the executor's view. Removing it is option 3, which stays
  open and is not needed for correctness.
- The SQLite and PostgreSQL stores ignore `history_length` and still
  parse every listed row whole. They pay no second read. Trimming in SQL
  (`json_remove`, `jsonb - 'history'`) would extend the gain to them, but
  their lookup cost has not been measured, so it is not claimed here.
- Tests: `context_lookup_tests` (whole on either kind of store, one
  second read only on a store that trims, the choice unchanged) and
  `in_memory::listing` (the store trims exactly as the handler does).
