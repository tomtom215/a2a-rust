<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Sustained Load and Table Growth

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
The test asserts it on the median, last quarter against first, under 3×:
growing tables slow every request, while another job on the machine slows
some, and a p95 assertion tripped on that alone. It also refuses to judge
memory growth on fewer than 1,000 pairs after warm-up, where one allocator
step reads as a leak.

## The `tasks` table grows until you tell it not to

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

### How fast it actually grows

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

### Turning it on

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
