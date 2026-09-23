<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Running More Than One Replica

Two replicas behind a load balancer sharing a database is the first
architecture most people reach for, and until recently this book said nothing
about it. Everything below is measured by
`crates/a2a-protocol-server/tests/multi_replica.rs`, which runs two handlers
over one PostgreSQL instance and asserts each claim. Where something does not
work, there is a test pinning that too — so if a future release changes it, the
test fails rather than this page quietly becoming wrong.

## The short version

| Behaviour | Across replicas | What makes it so |
|---|---|---|
| Replicas can start together against an empty database | **Yes** | Schema creation takes one advisory lock |
| A task created on one replica is readable on another | **Yes** | The store holds it |
| `GetTask` / `ListTasks` see every replica's tasks | **Yes** | Same |
| A subscription terminates when the task finishes elsewhere | **Yes** | The reattach hook polls the store |
| A subscriber sees intermediate events from another replica | **No** | Event queues live in process memory |
| Rate limiting enforces the configured limit | **Only with a shared counter** | Otherwise each replica counts alone |
| A second send to a running task is refused | **No** — each replica admits one | Admission state is per-handler |
| A task one replica finished stays finished | **Yes** | The store refuses to move a terminal task |
| `CancelTask` stops an executor running on another replica | **At its next write** | The refused write is the only signal |

Sharing a task store gets you most of the way. What it does not cover is
streaming, rate limiting, and who may write to a running task — and only rate
limiting has a complete fix.

## Share the store

Nothing works across replicas without this. Point every replica at the same
PostgreSQL instance:

```rust
# use a2a_protocol_server::store::PostgresTaskStore;
# use a2a_protocol_server::{agent_executor, RequestHandlerBuilder};
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _q| async { Ok(()) });
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let store = PostgresTaskStore::new(&std::env::var("DATABASE_URL")?).await?;
let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_task_store(store)
    .build()?;
# Ok(())
# }
```

With that in place, a client whose second request lands on a different replica
still finds its task. This is the property everything else here depends on.

Every PostgreSQL store creates its schema at construction, and replicas that
start together against an empty database used to race: `CREATE TABLE IF NOT
EXISTS` is not safe to run concurrently, and one replica could fail its first
start with a duplicate key in `pg_type` or `pg_class`. Since 2026-09-23 every
constructor — the task stores, both push-config stores, the shared rate-limit
counter and `with_migrations` — does its DDL under one transaction-scoped
advisory lock, so they serialize. The key is the bytes of `"a2a_schm"` read as
a big-endian `i64`; an application taking advisory locks of its own on the
same database should avoid it.

## Rate limiting needs a shared counter

`RateLimitInterceptor` counts in a process-local map. That is correct for one
process and wrong for two: each replica admits the full configured rate, so
**N replicas admit N times the limit**. Two limiters configured for 5 requests
per window admit 10 — measured, not inferred.

If the limiter exists to protect an upstream with a real quota, that is the
whole point of it defeated, and it fails in the direction that matters least
when you have one replica and most when you have twenty.

`with_shared_counter` moves the count somewhere every replica can see:

```rust
# use std::sync::Arc;
# use a2a_protocol_server::{PostgresRateLimitCounter, RateLimitConfig, RateLimitInterceptor};
# async fn example(database_url: String) -> Result<(), Box<dyn std::error::Error>> {
let counter = Arc::new(PostgresRateLimitCounter::new(&database_url).await?);
let limiter = RateLimitInterceptor::new(
    RateLimitConfig::default()
        .with_requests_per_window(100)
        .with_window_secs(60),
)?
.with_shared_counter(counter);
# Ok(())
# }
```

Every replica now increments the same row, and the limit is the deployment's.

### Give each caller its own key

A shared counter makes the limit global. It does not make it *per caller* —
that depends on `CallContext::caller_identity`, and only an authentication
interceptor can supply one:

- `JwtAuthInterceptor` records the validated `sub` automatically.
- `ApiKeyAuthInterceptor::with_labelled_keys` and
  `BearerTokenAuthInterceptor::with_labelled_tokens` record a label you choose.

The label is separate from the credential on purpose. A caller key reaches the
rate-limit table your replicas share, and can reach logs and metrics; a bearer
token or API key belongs in none of those.

**Register authentication before the limiter.** The chain runs interceptors in
registration order over one context, so a limiter registered first reads an
identity nothing has set yet and buckets every caller together. Nothing rejects
that ordering — it just quietly stops being per-caller.

Without any of this every caller falls back to a shared `"anonymous"` bucket,
so the limit still holds but one noisy client spends everyone's budget.

### What it costs

A round trip on every request, and the figures are not small — loopback,
release build, best of three runs of 2,000 requests:

| Counter | Per request |
|---|---:|
| In-process (the default) | 0.2 µs |
| `PostgresRateLimitCounter` | 232 µs |
| The same on a durable pool | 598 µs |

For scale, a complete JSON-RPC request through this server measures ~195 µs on
the same machine. A shared counter roughly doubles the cost of a request, and a
counter across a real network costs whatever that network costs.

That is why it is opt-in. A single-replica deployment gains nothing and should
not pay it. A deployment that needs both a global limit and the last
microsecond should implement `RateLimitCounter` against Redis or another
in-memory keyspace — the trait has one method, so that is a few lines.

`PostgresRateLimitCounter::new` runs its own pool with
`synchronous_commit = off`, which is where the 598 µs → 232 µs comes from. A
rate-limit count describes one window and is swept away shortly after, so the
worst a crash can do is forget that a caller had spent part of its budget. That
setting is **not** applied by `from_pool`, because that pool is usually the task
store's, and it is not a counter's business to make a task store non-durable.

### When the counter is down

The request is counted locally instead. The failure mode is exactly the
per-replica behaviour you had before adopting a shared counter — not an outage,
and not an open door. Failing closed would make adding a shared limiter a
reliability regression; failing open would remove the limit at the moment an
attacker with database access most wants it gone.

## Streaming does not cross replicas

An agent's event queue lives in the process running its executor. A client that
subscribes on a different replica from the one running the task:

- **does** get a stream that terminates correctly when the task reaches a
  terminal state, with that state reported — spec §3.1.6 (`STREAM-SUB-002`) is
  satisfied across replicas;
- **does not** see the artifact and status frames produced along the way.

So a client that reconnects mid-stream to another replica keeps a correct task
and a correct ending, and loses the middle. That is a property of the
architecture, not a bug, and there is no configuration that changes it today.

Three ways to live with it, in the order most deployments should consider them:

1. **Session affinity.** Have the load balancer route by task ID or by
   connection. This is the direct fix and most balancers do it already.
2. **Poll instead of stream.** `GetTask` is replica-independent, because the
   store is. A client that polls is unaffected by any of this.
3. **Push notifications.** Configure a webhook; delivery is driven from the
   replica running the task and does not depend on where the client is.

## Continuations and cancellation

### Two replicas can both run the same task

Within one replica, a second `SendMessage` naming a task whose executor is
still running is refused: the handler checks its own map of in-flight
executors under its own lock. Neither is in the store, so **across replicas
the refusal does not apply**. A continuation sent to replica B while the
task's executor runs on replica A is admitted, B spawns a second executor, and
both write to the same row. `the_single_writer_refusal_does_not_cross_replicas`
in `tests/multi_replica.rs` pins this: the same continuation is refused by A
and accepted by B.

What the store guarantees in that situation is narrower than "no harm":

- **The first terminal state wins, and stays.** Every shipped store refuses a
  write that would move a stored terminal task to a different state, and does
  so atomically with the write — a condition on the `UPDATE` or upsert for
  SQLite and PostgreSQL, the write lock for the in-memory stores. The other
  executor's later writes are refused unless they carry that same state, and
  the first refusal cancels it, as below.
- **Everything before that is last-writer-wins.** Two executors emitting
  artifacts, history or non-terminal statuses for one task interleave their
  writes, and neither sees the other's.

If your clients can send to a running task — multi-turn agents do — route by
task or context ID (session affinity, as for streaming) so every turn of one
task lands on one replica.

### Cancelling a task that runs elsewhere

`CancelTask` can arrive at any replica. On replica B, for a task whose
executor runs on replica A:

1. B finds no executor of its own to signal. It calls its own executor's
   `cancel` (so an override that releases in-process resources releases B's,
   not A's), writes `Canceled`, and answers the client `Canceled`.
2. A's executor has not heard. The next time it emits anything, A's write is
   refused by the store, because the task is terminal in another state.
3. From that refusal A **cancels its executor's cancellation token**, adopts
   the stored task, and stops writing, pushing and logging the executor's
   events. A client streaming from A ends on `Canceled`, not on whatever A's
   executor emitted; a blocking `SendMessage` waiting on A is answered with the
   `Canceled` task; A delivers `Canceled` to the task's webhooks, which nothing
   else would, because B has no processor for the task.

The client B answered is therefore never contradicted: before this was
enforced, all three stores let A overwrite `Canceled` with `Completed`, and
`tests/cross_replica_cancel/` reproduced it on each.

What this does **not** give you:

- **Prompt cancellation.** A's executor learns at its next write. One that
  works silently for an hour runs for an hour. There is no cross-replica
  cancel signal; the refused write stands in for one.
- **A clean stream on A.** Non-terminal frames A's executor emitted between
  the cancel and its next write still reach a client streaming from A —
  though never the store. The terminal frame is held until the store has
  ruled on it, so the ending is right.
- **An executor that honours the token.** Cancellation is cooperative. An
  executor that ignores `RequestContext::cancellation_token` keeps running on A
  until it returns, with its writes refused.

Session affinity makes all three moot: the cancel then reaches the replica
running the executor, which signals it directly.

## Under sustained load

`tests/soak_multi_replica.rs` runs both replicas against one database for as
long as you ask. A 60-second run on a 4-core machine:

| | |
|---|---|
| Request pairs (a `SendMessage` on one replica, a `GetTask` on the other) | 64,642 |
| Failures | 0 |
| Cross-replica misses | 0 |
| Resident growth after warm-up | 19.2 bytes per pair |
| Rate-limit table, peak | 16 rows for 8 callers across ~12 windows |
| p95 latency, first quarter → last | 9,333 µs → 9,707 µs (1.04×) |

Three things worth drawing out.

**Cross-replica reads held under load.** Every one of those 64,642 pairs wrote
on one replica and read on the other, and none missed. The consistency shown
one-request-at-a-time earlier is not an artifact of a quiet system.

**The shared counter's table stays bounded.** It holds the current window's
keys plus, briefly, the previous window's — 8 or 16 rows for 8 callers. Without
the sweep it would have reached 96 in a minute and would keep going for the
life of the deployment. That is asserted, and the assertion was checked by
disabling the sweep: 104 rows against a ceiling of 32.

**Latency did not degrade** as the `tasks` table grew past 65,000 rows.

### The `tasks` table grows until you tell it not to

Visible in that run: 65,399 rows after 60 seconds, and nothing removes them.
A2A has no `DeleteTask`, so nothing in the protocol ever shrinks the table.

**The settled policy: a persistent store deletes nothing by default, and
`purge_expired` is how you choose otherwise.**

That default is deliberate, and it is the opposite of what the in-memory store
does — `TaskStoreConfig` defaults to a one-hour TTL and a 10,000-task cap, so
the default in-process deployment forgets a task an hour after it finishes,
while the durable one keeps it forever. Forgetting is right for a cache and
wrong for a database: a library that quietly deleted rows from your PostgreSQL
would be a far worse surprise than one that grows, and "how long do we keep
completed work" has legal answers as often as engineering ones. The divergence
itself was the real defect — not that the table grows, but that the two stores
disagreed and neither said so anywhere an operator would look.

#### How fast it actually grows

Measured, so you can decide whether this is urgent or a note for next year. A
completed task carrying a two-message history and no artifacts, with the
indexes the store creates:

| store | bytes/row (incl. indexes) | 1M tasks |
|---|---|---|
| PostgreSQL 16 | 826 | 0.77 GiB |
| SQLite | 781 | 0.73 GiB |

The document dominates, so scale it by your own task size — a task carrying
artifacts is bigger by however large the artifacts are. At a million completed
tasks you are under a gigabyte; the point at which this needs attention is
volume, not time.

#### Turning it on

```rust
use a2a_protocol_server::store::{PostgresTaskStore, RetentionPolicy};
use std::time::Duration;

async fn sweep(store: &PostgresTaskStore) -> a2a_protocol_types::error::A2aResult<()> {
    let policy = RetentionPolicy::new(Duration::from_secs(30 * 24 * 3600))
        .with_batch_size(1_000)
        .with_max_batches(50);      // bound one sweep; the next continues

    let report = store.purge_expired(&policy).await?;
    println!(
        "retention: deleted {} task(s), complete={}",
        report.tasks_deleted, report.complete
    );
    Ok(())
}
```

Call it from whatever already schedules work — a cron entry, a Kubernetes
`CronJob`, a `tokio` interval in your own binary. It is deliberately not wired
to a timer inside the store: a sweep that fires on its own fires during your
traffic peak, and the store does not know when that is.

Only terminal tasks are eligible — `Completed`, `Failed`, `Canceled`,
`Rejected`. A task still `Working`, or parked in `InputRequired` waiting on a
human, is never deleted however old it is. Pick the interval from how long your
clients may reasonably poll for a task after it finishes.

Three details worth knowing rather than discovering:

* **It is safe from several replicas at once.** Each batch is one `DELETE`
  whose subquery picks its own rows, so two sweeps racing delete disjoint sets.
* **Batching is about locks, not throughput.** One `DELETE` covering years of
  backlog holds locks and keeps a transaction open for its whole run. The
  default 1,000-row batches let everything else through in between.
* **`report.complete` distinguishes "nothing left" from "ran out of budget"**,
  so a bounded sweep does not have to be inferred from a count.

The naive form of this — `DELETE FROM tasks WHERE updated_at < now() -
interval '30 days'` — is what this page used to recommend. It is fine at small
scale and has two teeth at large: it is unbatched, and on SQLite it leaves
`task_artifact_appends` rows behind if the pool was built without
`foreign_keys=ON`, where they would be spliced onto the next task to reuse the
id.

## What is still unevidenced

The replicas above are two handlers in one OS process, so nothing here can see
a defect that needs real process isolation. Both runs are loopback on one
machine: no real network, no failover, no partition. If you are running this at
scale and have measurements, they would be welcome.
