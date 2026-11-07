<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Streaming Across Replicas

An agent's event queue lives in the process running its executor. A client
that subscribes on a different replica from the one running the task gets the
task's events from the store's event log instead, which the replicas share;
both SQL stores keep one. That client:

- **does** get a stream that terminates correctly when the task reaches a
  terminal state, with that state reported — spec §3.1.6 (`STREAM-SUB-002`) is
  satisfied across replicas;
- **does** see the artifact and status frames produced along the way, in the
  order the agent emitted them and at the positions the running replica's own
  subscribers see (spec §3.5.2). They arrive in batches: the subscription's
  reattach hook looks for new events every `subscribe_reattach_interval`
  (250 ms by default), so a frame can be up to one interval late. While the
  stream is idle the wait doubles, up to `subscribe_reattach_max_interval`
  (4 s by default), so the **first** frame after a long quiet can be up to that
  late; each frame that does arrive starts the wait over at the interval. An idle
  subscriber used to read the store about eight times a second for as long as its
  client stayed, on the pool that keeps the replica's claims alive; at the ceiling
  it makes one poll, two statements when the store keeps a log, every 4 s. Set the
  ceiling equal to the interval to keep every frame within one interval and pay
  the load, or raise it to pay less.

  **With the PostgreSQL stores, that wait can be cut short** (since 0.16.0, audit
  E-F25 (b), ADR 0057): build the store with `with_log_notifications` and every
  append to a task's log also runs `pg_notify`, in the append's own statement,
  and a listener on each replica wakes the streams waiting on that task at once.
  Off by default; see [Waking a waiting stream](#waking-a-waiting-stream) below.

The stream starts from a snapshot of the task and replays the log after the
position that snapshot reflects. Every shipped store returns the task together
with that position, in one read (`TaskStore::get_with_position`), so an event
the running replica logs and stores while the subscription starts arrives once;
before 0.15 it could arrive twice, and a client folding an `append: true`
artifact chunk would have folded it twice. Three edges remain. A replica still
on an earlier release does not write that position, so during a rolling
upgrade a subscriber can see an event its snapshot already holds a second time.
A custom store that does not return the position falls back to reading the
log's end and then the task, which can lose the one event another replica has
logged and not yet stored, or repeat one it logged and stored between the two
reads
([The log position a stored task reflects](../../building-agents/stores/replicas.md#the-log-position-a-stored-task-reflects)).
And a store without an event log has nothing to replay: its subscribers get
the ending and not the middle.

## Waking a waiting stream

```rust,no_run
use a2a_protocol_server::store::{LogListen, PostgresTaskStore};

async fn store(url: &str) -> Result<PostgresTaskStore, Box<dyn std::error::Error>> {
    // On every replica. `Pool` for a pool that reaches PostgreSQL directly.
    Ok(PostgresTaskStore::new(url)
        .await?
        .with_log_notifications(LogListen::Pool))
}
```

What it changes: an event another replica appends reaches a waiting stream in
about a notification's round trip instead of at the end of its wait, which can
be 4 s into an idle task.

What it does not change: the load of an idle stream. The timer stays as the
fallback, so a notification that never arrives costs one wait, and an idle
stream still reads the store every `subscribe_reattach_max_interval` (measured:
200 idle subscribers cost 93 statements a second, with the option or without
it). On a fleet where every replica has the option, raise that ceiling to cut
the load in proportion. A stream also never reads faster than once per
`subscribe_reattach_interval`, so a task that streams quickly costs a subscriber
what polling did, not more (two events a few milliseconds apart reach it up to
one interval apart).

What it does not wake: a task made terminal, or cancelled, with **no event in
the log**, such as a `CancelTask` of a parked task on another replica, or one
the claim reaper ends. Those reach a waiting stream at its next poll, up to the
ceiling later, as they did before. Measured on a local database (ADR 0057): an
event written on one replica ended a stream's 8 s wait on the other in 260 ms; a
cancel of a parked task took the whole 2 s of a 2 s ceiling.

The channel is `a2a_task_log`, and its payload names a task and its tenant
partition, so it is worth keeping out of the way of other consumers of the
database's notifications. Three things to know before turning it on:

* **Behind PgBouncer in transaction mode, use `LogListen::Direct(url)`** with the
  URL of PostgreSQL itself, as for the halt store ([Behind
  PgBouncer](./shared-stores.md#behind-pgbouncer)). `LISTEN` belongs to a session,
  and a pooler lends the session to another client, whose next statement then
  fails with `unexpected message: NotificationResponse`. A store built without
  `with_log_notifications` listens to nothing.
* **It holds one connection per replica**: one from the store's pool with
  `Pool` (make the pool one larger), or one of its own with `Direct`. The
  listener starts with the first stream that waits, so a replica serving no
  cross-replica stream holds none. A pool you build and pass to `from_pool` is
  unchanged.
* **Set it on every replica.** A replica without it neither announces its
  appends nor hears others': its events reach the others' streams by polling.
  A rolling upgrade is safe either way, since a wake is only a hint to read.

SQLite and the in-memory stores have no listener and are unchanged: their
streams poll, as the rest of this page says. `NOTIFY` is not free at very high
append rates: PostgreSQL serialises the commit of every transaction that
notifies, which is why this is opt-in; the measurement is in ADR 0057.

Where a poll's latency matters, or the store keeps no log, three ways to live
with it, in the order most deployments should consider them:

1. **Session affinity.** Have the load balancer route by task ID or by
   connection. A subscriber on the replica running the task reads its queue
   directly, with no poll in between.
2. **Poll instead of stream.** `GetTask` is replica-independent, because the
   store is. A client that polls is unaffected by any of this.
3. **Push notifications.** Configure a webhook; delivery is driven from the
   replica running the task and does not depend on where the client is.
