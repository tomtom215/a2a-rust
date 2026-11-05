<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0054: A turn whose executor can be re-run is handed to another replica on shutdown

**Date:** 2026-11-04
**Status:** Accepted for 0.16.0 (maintainer's instruction of 2026-11-04: every
open plan item is in the release, the best engineering option is built and
can be vetoed in review). Audit D-21.
**Author:** Tom F.

---

## Context

A rolling deploy stops replicas one at a time. Since audit E-F2 a stopping
replica closes admission (sends are refused with a retry delay), lets running
turns finish for `completion_grace` (5 s), then fires every task's token and
ends each turn still running `Failed` with failure class `transient`
(`handler::messaging::terminal`, `SHUT_DOWN`). Nothing is cancelled any more,
but the work of every turn longer than the window is lost on every deploy,
and the caller has to send it again as a new task.

The supervisor side of a delegation already survives a redeploy: since ADR
0030 and the group M fix, `Delegation::wait_or_hand_off` detaches from its
children on SIGTERM without cancelling them, the journal keeps their ids, and
`reattach` picks them up from the next process. The live proof
(`docs/audits/2026-11-01-triage/evidence/m-live-proof/`) shows it, 3 of 3
runs for each of the bare supervisor and the supervisor behind
`Server::serve_with_shutdown`: 50 children `HandedOff`, 50 tasks still
`working`, 0 cancels, and the next process completed all 50. That covers
the process that *delegated*. It does not cover the process that *runs the
turn*: the worker in that proof never shut down. When the worker is
redeployed, its turns still end `Failed`/`transient`.

What a fleet with turn claims already has: every turn holds a claim in the
shared store, renewed every second; a reaper on every replica finds tasks
`submitted` or `working` that hold no live claim and were not written for a
claim time to live, takes the claim, and ends them `Failed`/`transient`
(audit E-F1). It does not re-run them because "what the executor had done (a
model call, a tool's side effect) is not known, so running the turn again is
the caller's decision" (`handler::claims::reaper`).

## Options

1. **Document and tune.** Say plainly that a turn longer than the completion
   window ends `Failed`/`transient` on a deploy; show raising
   `completion_grace` with `terminationGracePeriodSeconds`. Costs nothing,
   loses the work of every turn longer than the platform's grace.
2. **A longer default window**, bounded by the platform's deadline. Helps
   turns of tens of seconds; Kubernetes' default grace is 30 s, so a turn of
   minutes is still lost, and every deploy takes as long as its longest turn.
3. **The reaper re-runs the turn.** The stopping replica leaves the task
   `working` and marked; another replica's reaper takes the claim and calls
   the executor again from the stored task. Rejected after reading the code:
   the re-run has no request, so no caller credentials, no originating
   principal and no budget from the first call, and an executor that
   delegates with the caller's token would act as nobody; and the reaper
   holds only a weak reference to the claims, not the handler, so it would
   need a second copy of the whole send path (queue, claim, background
   processor, push lane) to run a turn. It also breaks the reaper's own rule
   that running a turn again is the caller's decision.
4. **Hand the turn back to its caller.** The stopping replica leaves the task
   `working`, marked as handed back, and releases its claim; the caller sends
   the turn's message again to the same task id, and whichever replica
   receives it runs the turn, as the caller, under the ordinary send path.

## Decision

Option 4, opt-in per executor, with option 1's documentation for everything
it does not cover. Who may run a turn twice is the executor's decision: only
it knows whether doing so is safe (idempotency keys on its tool calls, ADR
0026; side effects checked before they are made). Who runs it again is the
caller's.

* `AgentExecutor::resumes_after_hand_off(&self) -> bool`, default `false`.
* When it answers `true`, the turn holds a claim (a shared store that hands
  them out), and shutdown stops the turn (its token fired and the executor
  returned with no terminal or interrupted state), the handler does not run
  the cancel hook and does not write `Failed`. It writes a `working` status
  whose agent message (`HANDED_OFF`) says what happened and what to do, with
  `{"a2a-rust.com/hand-off": true}` (`HAND_OFF_METADATA_KEY`) in its
  metadata, and the claim is released as every turn's is.
* A send to that task on any replica is admitted as a turn on it (the
  in-process check that refuses a second send while an executor runs finds
  none there, and the claim serialises turns across replicas) and runs with
  the stored task as `stored_task`, so the executor can tell a re-run.
* The reaper leaves a handed-off task alone for `HAND_OFF_WINDOW` (600 s)
  from its hand-off status: the SQL claim stores' `abandoned` statements
  leave it out (so a deploy that hands off more turns than a look ends,
  32, does not crowd genuinely abandoned tasks out of every look). After the
  window, a task nobody continued is ended `Failed`/`transient`, as today.

## The caller's side: `Delegation` continues a handed-back child (added 2026-11-05)

The lead's decision of 2026-11-05: this crate's `Delegation` acts on the mark
by default. When an event of the child carries `a2a-rust.com/hand-off` on a
`working` status, the delegation drops the stream and, after a wait, sends
the message it started the child with again: a new message id, the same task
and context ids, a new idempotency key (the first would be answered with the
task, not run), and the supervisor's own client, so its own credentials. It
follows the new stream as before.

* Bounded by `HandOffPolicy` (`DelegationOptions::with_hand_off`): at most 3
  continuations per child by default, after 1 s, 2 s and 4 s (each wait
  doubles, up to 8 s). Past the bound, or when a send fails each time, the
  mark is treated as a lost stream: resumed under the `ResumePolicy`, then
  the child is cancelled with `Outcome::Lost`. A task nobody runs is ended
  `failed`/`transient` by the reaper after the window.
* `HandOffPolicy::never()` opts out.
* Safe as a default: the mark is only ever written by a server whose executor
  opted in, on a store with claims; against any other server nothing changes.

The window is `HandlerLimits::hand_off_window` (default 600 s,
`HAND_OFF_WINDOW`), in the configuration reference. The reaper passes it to
`TurnClaimStore::abandoned_except_handed_off` (a new method whose default
ignores the window and asks `abandoned`, so claim stores of your own keep
working); the SQL stores bind it.

## What this covers, and what it does not

Covers: turns of an executor that opts in, on a handler with turn claims, on
a graceful shutdown (`Server::serve_with_shutdown`, and `finish_in_flight` /
`cancel_in_flight` called by the application), on every binding.
`handler::messaging::hand_off_tests` runs it with two handlers on one SQLite
file: the first is shut down, the task is `working` and marked, the second
admits the caller's next send and completes it.

Does not cover, and says so:

* **A crash** (SIGKILL, out of memory, a lost node). No mark is written; the
  reaper ends the task `Failed`/`transient` as today.
* **Callers that do not act on the mark.** A client that is not this crate's
  `Delegation` and does not read the status message sees `working`, then
  `failed`/`transient` after the window, as before plus the window.
* **A delegation that reattached after a restart.** The journal keeps no
  message text, so a reattached handle has nothing to send again; it treats
  the mark as a lost stream (resume, then cancel).
* **Progress inside the turn.** The turn runs again from its input; an
  executor that wants to resume from a checkpoint keeps it itself.
* **Open streams on the stopping replica.** They see the `working` hand-off
  status and then end when the replica goes; a client resubscribes
  (`SubscribeToTask`) on another replica. A webhook sees the hand-off status.
* **Handlers without claims** (the in-memory store, a store that hands out no
  claims) and claim stores of your own whose `abandoned` does not leave the
  mark out: no hand-off, or the reaper may end a handed-off task after one
  claim time to live instead of the window.

## Consequences

* A new trait method with a default, two public constants
  (`HAND_OFF_METADATA_KEY`, `HAND_OFF_WINDOW`): additive, not breaking.
* An opted-in turn's work is no longer lost on a deploy if its caller
  continues it; it costs the turn its time since its start.
* The stale doc comments that still said `Canceled` on shutdown
  (`serve/graceful/config.rs`, `handler/shutdown/in_flight.rs`) are corrected.
