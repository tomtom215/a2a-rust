<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0030: A delegation resumes a lost stream before it cancels, and its record can outlive the supervisor

**Date:** 2026-10-31
**Status:** Proposed. The maintainer chose the direction on 2026-10-31
("resume then cancel + reattach, and also build the journal now"); the
choices below that the maintainer did not make (where the SQL lives, the
defaults) are this ADR's and are open to change while 0.16.0 is unreleased.
Built: points 1 to 3 (resume, reattach, the trait and the in-memory
journal) and, of point 4, the PostgreSQL journal and, since 2026-11-04, the
SQLite journal (audit NEW-h-s0-4).
**Author:** Tom F.

---

## Context

`Delegation::wait` follows a child task over a streaming connection. Any
end of that stream before the child settled, a reset connection, a proxy
rollout or the five-minute `stream_idle_timeout`, was read as "the child is
lost": the delegation sent `CancelTask` to the child and finished with
`Outcome::Lost` (audit H4, run by reading `delegation/mod.rs` `wait` and
`lost`; tests `a_task_stream_that_ends_unsettled_is_lost_and_its_child_cancelled`
and `a_lost_stream_cancels_the_child` pin it). The client already had what a
resume needs: `EventStream::last_event_id`, `A2aClient::subscribe_to_task_from`,
and a server that replays from a `Last-Event-ID`. Nothing used them. In a
tree of delegations the cancel cascades: a blip on one link kills healthy
work below it.

Two further gaps follow from the same design:

- A supervisor that restarts has no way to pick a child up again. `detach`
  returns the id but not the stream position, and there is nothing to call
  with either.
- Nothing records, before the child exists, that a supervisor started it.
  A supervisor that dies between "sent" and "recorded" leaves a child nobody
  owns.

## Decision

1. **Resume, then cancel.** When the stream ends unsettled and the child's
   id is known, the delegation resubscribes from the last event id under a
   bounded `ResumePolicy` (attempts, backoff between them, an overall
   deadline; defaults 5 attempts, 200 ms doubling to 5 s, 30 s in all). If
   the subscription is refused, it asks for the task (`GetTask`) before
   anything else: a child that settled during the gap is reported as what it
   became, never as `Lost`; the A2A rule that a terminal task cannot be
   subscribed to makes this the common case, not an edge. Only a child that
   is still running after the policy is spent is cancelled, as before.
   `ResumePolicy::none()` restores the old behaviour. On by default, which
   changes what `Outcome::Lost` means; that is cheap while 0.16.0 is
   unreleased.
2. **`Delegation::reattach`**, with `Delegation::last_event_id`, so a
   supervisor that recorded `(task id, last event id)` can follow the child
   again after a restart. Reattaching to a child that has already settled
   returns a delegation that is already settled.
3. **A `DelegationJournal` trait and a `DelegationRecord`**, in
   `a2a-protocol-types`: record before the stream is opened (`begin`, with
   the idempotency key), note the child's id and position as events arrive
   (`progress`), settle, and list the records still open (`open`). The
   delegation calls it when one is attached with `with_journal`; without one
   nothing changes. An in-memory implementation ships beside the trait.
4. **SQL journals live in the server crate**, behind its existing `postgres`
   feature and its `sqlite` feature, and are re-exported by the SDK
   crate: `PostgresDelegationJournal` and `SqliteDelegationJournal`.

### Why the SQL lives in the server crate

Neither protocol crate depends on the other; both depend on
`a2a-protocol-types`, and only the server crate has the SQL stack (sqlx, the
migration and retry machinery for PostgreSQL, the tenant-aware layout).

| Option | For | Against |
|---|---|---|
| **A. Trait and records in `types`; SQL in `server`** (chosen) | No new crate to publish or version; reuses the pool, migration and PostgreSQL retry code; the dependency direction stays `types <- client`, `types <- server` | The SQL implements a client-side concept in the crate named "server". A supervisor is almost always also an agent, and so already links it; one that is not links a crate larger than it needs |
| B. A crate of its own for the SQL | Each crate has one job | A sixth crate: another release, another entry in the package and gate lists, a second copy of the PostgreSQL retry policy |
| C. sqlx in the client behind features | The journal sits beside its user | The client gains a database dependency and duplicates the server's pool and migration code; the client has no database dependency today (its `Cargo.toml` names no SQL crate), and this would add the first |

The trait is deliberately small and asynchronous in the style of the stores
(`Pin<Box<dyn Future>>`, no macro), so a deployment with a store of its own
implements it without a new dependency.

## Consequences

- A blip on a link no longer kills the child below it. The cost is time: a
  child whose connection is truly gone is cancelled after the policy's
  deadline (30 s by default) and not at once.
- Resumption depends on the agent replaying from `Last-Event-ID`. One that
  does not still sends the snapshot and the live stream, which is enough to
  learn the state; events between the cut and the snapshot are not replayed,
  and that is the agent's limit.
- The journal records are data about delegated work: ids, keys, positions and
  states. They carry no message text and no credentials. Retention is the
  deployer's: `settled` rows stay until `purge_settled` is called.
- Not decided here: a lease or budget grant is not restored by `reattach`
  (a reattached delegation does not renew a lease it did not grant); a
  supervisor that wants that records the grant itself. A journal shared by
  several supervisors needs an owner column, which the first version has but
  does not enforce. (Amended 2026-11-03: a lease is restored by
  `reattach_with`; see Amendment.)

## What was and was not run

Written before the code; the verification of what is built is in the pull
request that carries it. The in-memory journal and the delegation's use of it
are tested against this repository's server, and `PostgresDelegationJournal`
runs the journal contract on PostgreSQL 16.15. The SQL has not been run on
PostgreSQL 17, under concurrent replicas, or after a connection loss
mid-statement (it uses the crate's `pg_retry`, which is tested elsewhere).

## Amendment, 2026-11-03 (audit H4, H18, NEW-h-s0-2, NEW-h-s0-3, NEW-h-designs-2, NEW-h-designs-5; fork F5)

Found by running the delegation against the replica fleet, and decided by the
maintainer as fork F5. Built in the same release as the original.

1. **Hand-off on shutdown.** `Delegation::wait_or_hand_off(cancel, hand_off)`
   takes the supervisor's own shutdown signal. When it fires first and the
   delegation has a journal record, the child keeps running, the record stays
   open at the position the stream reached, and the outcome is
   `Outcome::HandedOff`. Without a record a hand-off cancels, as a cancel
   does: a child nothing can find again must not be left running. `wait` is
   `wait_or_hand_off` with a future that never fires. The reason: a graceful
   redeploy fires every executor's token, the documented pattern passes it to
   `wait`, and every child was cancelled and every record settled `canceled`.
2. **`DelegationOptions`**, taken by `start_with` and `reattach_with`: a lease,
   a budget grant, a journal record, a `ResumePolicy` and an id wait together.
   `start_leased`, `start_budgeted` and `start_journaled` call `start_with`.
   `reattach_with` renews the lease again from the moment the handle exists,
   because the lease a child was granted lapses after its time to live without
   a renewal, and the restart a journal is for would otherwise end the children
   it recorded. It refuses a budget grant (the parent's meter died with the
   process) and a new record (written before a send, and a reattach sends
   none) with `ClientError::Config`.
3. **A stream has worked** when its position advanced or when it delivered an
   event and stayed open for `ResumePolicy::healthy_after` (default 5 s). gRPC
   and WebSocket streams carry no event ids, so under the first rule alone a
   long quiet child was given up on after about six idle cuts. A server that sends
   a snapshot and drops the stream at once delivers an event but does not stay
   open, so it still exhausts the policy.
4. **Jitter.** The waits between attempts are shortened by up to half
   (`ResumePolicy::jitter`, default on), so that streams a proxy rollout cut in
   the same instant do not return in the same instant.
5. **Bounded journal writes.** Each write is given `WRITE_BOUND` (5 s) and then
   given up on; none is tried again for the 2 s progress interval; the record
   written before the first send fails the start with a timeout. The writes
   are awaited on the event path, so without a bound a journal outage stalled
   every delegation.

Not built, and why: a recovery driver. The loop is eight lines over
`DelegationJournal::open` and `reattach_with` (`book/src/client/delegation.md`),
and what it needs proving is a supervisor killed with `kill -9` against a
PostgreSQL journal with fifty children, which is a run, not code. gRPC and
WebSocket keep-alive is in ADR 0032's scope (the WebSocket client already pings
every 15 s by default).

## Amendment, 2026-11-04 (the live proof; audit LP-1, LP-2, LP-3)

The run the previous amendment called "a run, not code" was made: `scripts/live/delegation/`, PostgreSQL
16.15, fifty children that each hold a task for 60 s, a supervisor killed with `kill -9` at 20 s or sent
SIGTERM. Evidence, with each expectation written before its run:
`docs/audits/2026-11-01-triage/evidence/m-live-proof/`.

- **Held up.** After `kill -9` and a restart 5.4 to 7.5 s later, the book's loop found 50 open records and
  ended with 50 completed, none cancelled (3 of 3). After SIGTERM every child handed off, the 50 records stayed
  open and the 50 tasks working; a control that wires the same signal to the cancel future cancelled 50 of 50
  (3 of 3 each, for a bare supervisor and for one that is an agent behind `serve_with_shutdown`). A gRPC child
  quiet for 90 s under a 10 s client idle timeout resumed 8 times and completed, with no `CancelTask`
  (3 of 3; scaled timers, no proxy).
- **Three defects, all fixed in the same release.** (1) The book's loop waited on each child before it
  reattached the next, and a child's handle is what renews its lease, so with 15 s leases every child after the
  first lapsed (49 or 50 of 50 lost); the book reattaches every record first. (2) `Renewal::spawn` waited a
  third of the lease before its first renewal, which is right for a start and wrong for a reattach, whose lease
  was last renewed before the crash; a reattach renews at once. (3) The corrected loop first ended each
  reattach with `?`, and a dropped `Delegation` cancels its child, so one record that could not be reattached
  cancelled every child held (26 of 26 in 3 of 3 runs); the book matches on the result and goes on.
- **A limit, stated.** A restart that comes after the agent has noticed the lapse loses the child. With 15 s
  leases renewed every 5 s, a restart 9.5 s after the kill kept all 50 (6 of 6 runs) and one at 12 s kept none
  (3 of 3); between them the loss grows and two runs at one delay can differ. The agent's lease store was in
  memory; `PostgresLeaseStore` was not run, and nor was any other lease length, more than 50 children, or a
  `kill -9` of the `serve` supervisor. Size the lease to cover a restart (the book says so).
- **gRPC keep-alive** is built (`GrpcTransportConfig`, #193, opt-in), and `HttpPool` carries HTTP/2 PING for the
  JSON-RPC and HTTP+JSON bindings (ADR 0032); neither is on by default.
