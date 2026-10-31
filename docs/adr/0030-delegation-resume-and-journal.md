<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# ADR 0030: A delegation resumes a lost stream before it cancels, and its record can outlive the supervisor

**Date:** 2026-10-31
**Status:** Proposed. The maintainer chose the direction on 2026-10-31
("resume then cancel + reattach, and also build the journal now"); the
choices below that the maintainer did not make (where the SQL lives, the
defaults) are this ADR's and are open to change while 0.16.0 is unreleased.
Built: points 1 to 3 (resume, reattach, the trait and the in-memory
journal) and, of point 4, the PostgreSQL journal. Not built: a SQLite journal
(the lease store, the precedent, has none either).
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
   feature (and `sqlite`, if one is wanted), and are re-exported by the SDK
   crate. The PostgreSQL one is `PostgresDelegationJournal`.

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
  does not enforce.

## What was and was not run

Written before the code; the verification of what is built is in the pull
request that carries it. The in-memory journal and the delegation's use of it
are tested against this repository's server, and `PostgresDelegationJournal`
runs the journal contract on PostgreSQL 16.15. The SQL has not been run on
PostgreSQL 17, under concurrent replicas, or after a connection loss
mid-statement (it uses the crate's `pg_retry`, which is tested elsewhere).
