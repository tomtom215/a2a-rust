<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Running More Than One Replica

Two replicas behind a load balancer sharing a database is the first
architecture most people reach for, and until recently this book said nothing
about it. Most of what follows is measured by
`crates/a2a-protocol-server/tests/multi_replica.rs`, which runs two handlers
over one PostgreSQL instance; turn claims by `tests/turn_claims/` (two
handlers over one SQLite file or one PostgreSQL database) and
`tests/replica_processes.rs` (separate OS processes, one of them killed
mid-turn); the cancel rows by `tests/cross_replica_cancel/`, the lost-save row
by `tests/stale_write_position/`, the schema lock by
`tests/postgres_store_tests.rs`, and the load figures by
`tests/soak_multi_replica.rs`. Where something does not work, there is a test
pinning that too — so if a future release changes it, the test fails rather
than this page quietly becoming wrong.

## The short version

| Behaviour | Across replicas | What makes it so |
|---|---|---|
| Replicas can start together against an empty database | **Yes** | Schema creation takes one advisory lock |
| A task created on one replica is readable on another | **Yes** | The store holds it |
| `GetTask` / `ListTasks` see every replica's tasks | **Yes** | Same |
| A subscription terminates when the task finishes elsewhere | **Yes** | The reattach hook polls the store |
| A subscriber sees intermediate events from another replica | **Yes, up to a poll late** | The reattach hook replays them from the store's event log |
| Rate limiting enforces the configured limit | **Only with a shared counter** | Otherwise each replica counts alone |
| A second send to a running task is refused | **Yes** | The turn's claim, a row in the shared database (below) |
| A continuation of a parked task starts from what the last turn stored | **Yes** | It waits for the claim, then reads the task again |
| A task one replica finished stays finished | **Yes** | The store refuses to move a terminal task |
| `CancelTask` stops an executor running on another replica | **Within about a second** | The cancel marks the turn's claim; the owner's next renewal (every second) fires the executor's token. With `with_halt_store(PostgresHaltStore)` it is also pushed at once (ADR 0024) |
| A continuation's save keeps what another replica's turn wrote meanwhile | **Yes** | The claim serialises the turns; without claims it loses it (`tests/stale_write_position`) |
| A replica that dies mid-turn holds its tasks | **For one claim time to live (15 s)** | Then the next send takes the claim over and continues the task |
| A replica that stalled past its claim writes over the task's next turn | **No** | Every write of a turn is fenced by its claim's token |
| A halt (`halt` / `resume`) stops every replica | **Only with a halt store** | `with_halt_store(PostgresHaltStore)` writes and pushes it (ADR 0019, 0024); without one it holds in one process |
| A delegated child stops when the agent that delegated it goes quiet | **Only with a lease store** | `with_lease_store(PostgresLeaseStore)` keeps the deadline where every replica reads it, so a renewal on any replica counts (ADR 0020) |

The halt and lease rows are covered in [Human oversight](./oversight.md).

Sharing a task store gets you most of the way, and with the bundled SQL
stores it gets you all of it except rate limiting, which needs a shared
counter, and streaming latency, which is up to a poll late across replicas.

**What is supported.** One replica, or several sharing a PostgreSQL database
(or several processes on one host sharing a SQLite file), with **no session
affinity**: each turn of a task claims it in the database, so two replicas
never run one task at once (below). Before turn claims (2026-10-28) the
shared store did not do this, and the guidance was to route every turn of a task to one replica;
affinity still saves a cross-replica stream its poll, but it is no longer what
keeps tasks correct, except while a fleet is being upgraded from 0.15.1 or
earlier ([below](#upgrading-a-fleet-to-this-release)).

## Share the store

Nothing works across replicas without this. Point every replica at the same
PostgreSQL instance:

```rust
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_server::store::PostgresTaskStore;
# use a2a_protocol_server::{agent_executor, RequestHandlerBuilder};
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _q| async { Ok(()) });
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let store = PostgresTaskStore::new(&std::env::var("DATABASE_URL")?).await?;
let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_agent_card(card())
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
advisory lock, so they serialize. So do the audit, halt and lease stores,
which have taken it since they were added. The key is the bytes of `"a2a_schm"` read as
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

- `JwtAuthInterceptor` records the validated issuer and `sub` automatically,
  as `jwt:<issuer>:<sub>` (the issuer percent-encodes `%` and `:`).
- `ApiKeyAuthInterceptor::with_labelled_keys` and
  `BearerTokenAuthInterceptor::with_labelled_tokens` record a label you
  choose, as `api-key:<label>` and `bearer:<label>`.
- `MutualTlsAuthInterceptor` records the client certificate's fingerprint (or
  a name you map it to) as `mtls:<name>`.

The label is separate from the credential on purpose. A caller key reaches the
rate-limit table your replicas share, and can reach logs and metrics; a bearer
token or API key belongs in none of those.

An identity does more than key the limiter: it also gives the caller a task
partition of its own (spec §13.1; the default with an authenticating
interceptor since 0.15), which only stores that partition by tenant can hold.
With identified callers, use `TenantAwarePostgresTaskStore` and
`TenantAwarePostgresPushConfigStore` in place of the plain stores above, or
call `share_tasks_across_callers()` if callers are meant to see each other's
tasks. `build()` refuses the plain stores until you choose. See
[Multi-tenancy](./multi-tenancy.md#callers-within-one-tenant).

**Register authentication before the limiter.** The chain runs interceptors in
registration order over one context, so a limiter registered first reads an
identity nothing has set yet and buckets every caller together. Nothing rejects
that ordering — it just quietly stops being per-caller.

Without any of this an unidentified caller is keyed by its client address:
with `trusted_proxy_hops` set, the `trusted_proxy_hops`-th entry from the
right of `X-Forwarded-For`; with it zero, the connection's peer address (an
IPv6 client by its `/64`). Only when neither is available — a forwarded chain
shorter than `trusted_proxy_hops`, or a transport that records no peer — does
it fall back to a shared `anon` bucket, which keeps the limit but makes those
callers spend one budget. Keying by the peer address is new in 0.15; before,
every caller with no identity and no trusted forwarded address shared one
bucket.

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

## Streaming across replicas

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
  (250 ms by default), so a frame can be up to one interval late.

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
([The log position a stored task reflects](../building-agents/stores.md#the-log-position-a-stored-task-reflects)).
And a store without an event log has nothing to replay: its subscribers get
the ending and not the middle.

Where a poll's latency matters, or the store keeps no log, three ways to live
with it, in the order most deployments should consider them:

1. **Session affinity.** Have the load balancer route by task ID or by
   connection. A subscriber on the replica running the task reads its queue
   directly, with no poll in between.
2. **Poll instead of stream.** `GetTask` is replica-independent, because the
   store is. A client that polls is unaffected by any of this.
3. **Push notifications.** Configure a webhook; delivery is driven from the
   replica running the task and does not depend on where the client is.

## Continuations and cancellation

### One turn of a task at a time, on whichever replica

Within one replica, a second `SendMessage` naming a task whose executor is
still running is refused, and a continuation of a task parked at
`input-required` waits for the turn that parked it to store its last events.
Both decisions read state the replica keeps in memory, so on their own they
did not cross replicas: before turn claims a continuation that reached replica B
while the task's turn ran on replica A was admitted, B started a second
executor, and B's save replaced what A stored after B read the task.

**Turn claims** close it. The PostgreSQL and SQLite stores (plain and
tenant-aware) keep a table, `a2a_task_claims`, in the same database as the
tasks, and the handler takes a row in it for each turn:

1. **A send claims its task** once it knows which task that is, after an
   idempotent replay has been answered (so a retried send replays rather than
   waits). The row names this replica (`HOSTNAME`, the process id and a random
   suffix; `with_replica_name` overrides the first) and a random token for the
   turn.
2. **A claim another turn holds is waited for**, for up to
   `executor_drain_timeout` (5 s by default), the same bound the in-process
   wait for a parked turn has. If it is still held then, the send is refused
   exactly as on the replica running the task: `UnsupportedOperation`, "task …
   is already being processed". A send that waits gives up its idempotency
   key for the wait and claims it again once it holds the turn, so a retry of
   it that reaches a third replica meanwhile is not answered "accepted" for a
   send that is then refused: the retry claims the key and waits its own turn,
   and the first of the two to hold the turn runs while the other replays it.
3. **A continuation reads the task again** once it holds the claim, and starts
   from that read, so it keeps everything the last turn stored, wherever that
   turn ran.
4. **The claim is released when the turn's last write is done**: the
   executor's future and its event consumer (the background processor, or a
   blocking send's collector) both hold it, and it is deleted when the last of
   them ends.
5. **Every write a turn makes carries its token**, and the store refuses it,
   in the same statement, unless the token still holds a claim. A replica that
   stalled for longer than the claim's time to live (a long pause, a partition
   from the database) and wakes up after another replica took the task over
   is refused on its first write; its executor is then cancelled and it writes
   nothing more (`ClaimLost`). On PostgreSQL the check locks the claim row
   (`FOR KEY SHARE`). Without the lock, under READ COMMITTED, a write that
   started before a takeover and then waited on the task row was checked
   against the claim as it stood when the statement began, passed, and landed
   over the new turn's write (reproduced with two `psql` sessions). Renewals
   and cancel marks change no key column and never wait on the lock.
6. **A replica renews the claims it holds** every second, in one statement
   naming each one, while it holds any. A claim it no longer tracks (a release
   the database did not answer, a request dropped while its claim was being
   written) is not renewed and expires. A replica that dies stops renewing;
   after the time to live (15 s by default) the next send for one of its tasks
   takes the claim over and continues the task, which is still `Working` in
   the store. That is the longest a task waits after its replica dies.
7. **A replica that shuts down releases its claims** (`shutdown`,
   `shutdown_with_timeout`), so a rolling restart does not hold its tasks for
   the time to live. An executor still running then has its next write
   refused, as after a takeover.

`with_turn_claim_timing(ttl, renewal)` changes the two durations; `build`
refuses a time to live under 3 s, or a renewal interval that is not at most a
third of it. `without_turn_claims()` turns them off, for a store only one
replica uses: each turn then saves its two statements (the claim and its
release) and the renewal. A custom store several replicas share provides
claims by overriding `TaskStore::turn_claims` and fencing its writes with
`store::claim::current_fence` ([Stores](../building-agents/stores.md)), or is
given them with `with_turn_claims`. `build` refuses `with_turn_claims` over a
store that has claims of its own, the bundled ones included: their writes are
fenced against their own table, so claims kept anywhere else would refuse
every write.

An approval toward a quorum (`with_approval_gate`, `with_quorum`) is counted
under the claim, so two approvals arriving at two replicas at once are
counted one after the other and the executor resumes once.

**What it costs.** `tests/soak_multi_replica.rs` (two replicas, one
PostgreSQL 17 database, eight clients each sending a new task to one replica
and reading it from the other, 60 s), release build, on one 4-vCPU Linux
container with the database on the same host, three runs each way,
alternating (2026-10-28): 360 pairs/s with claims (±17, standard deviation),
423 without (±14), so 15% fewer; median latency per pair 21.4 ms against
18.6 ms. Every claim was released by the end of each run. A deployment whose
database is across a network pays the two extra round trips per turn at its
own latency; measure it there before relying on this figure.

What a claim does not change: the first terminal state either way is final
(every shipped store refuses to move a stored terminal task to another state,
atomically with the write), and a stream on a replica not running the task is
still fed by polling the event log (above).

Measured: `tests/turn_claims/` runs each case on SQLite and PostgreSQL, and
`tests/replica_processes.rs` runs two replica processes over each, killing one
mid-turn with `SIGKILL`: the other took its task over 2.82 s after the kill,
with a 3 s time to live and a 0.5 s renewal interval (2026-10-28, PostgreSQL
17.11 and SQLite).

### Upgrading a fleet to this release

Replicas on 0.15.1 or earlier take no claims and fence no writes. In a fleet
that mixes them with this release, a new replica's claim does not stop an old
replica from running the same task, and an old replica's writes pass every
fence. The database is not the problem: the new replica's migration adds the
claims table, and the old ones ignore it. So during the rollout either:

- route every turn of a task to one replica, as 0.15 required (session
  affinity on the task or context id), until no old replica is left; or
- replace the fleet at once: stop the old replicas, then start the new ones
  (or switch a blue/green deployment's traffic in one step).

A Kubernetes `RollingUpdate` without affinity runs old and new side by side for
the length of the rollout, and is the case the first option exists for. Once
every replica runs this release, the affinity can go.

### Cancelling a task that runs elsewhere

`CancelTask` can arrive at any replica. On replica B, for a task whose
executor runs on replica A:

1. B finds no executor of its own to signal. It calls its own executor's
   `cancel` (so an override that releases in-process resources releases B's,
   not A's), writes `Canceled`, and answers the client `Canceled`.
2. B marks A's claim on the task. A's next renewal, within about a second,
   reads the mark and fires the executor's cancellation token. With a halt
   store (`with_halt_store(PostgresHaltStore)`) the cancel is also pushed to A
   at once (ADR 0024).
3. Whatever A's executor writes after B's `Canceled` is refused by the store,
   because the task is terminal in another state. From that refusal A
   **cancels its executor's token** (if the renewal has not already),
   adopts the stored task, and stops writing, pushing and logging the
   executor's events. A client streaming from A ends on `Canceled`, not on
   whatever A's executor emitted; a blocking `SendMessage` waiting on A is
   answered with the `Canceled` task; A delivers `Canceled` to the task's
   webhooks, which nothing else would, because B has no processor for the
   task.

The client B answered is therefore never contradicted: before this was
enforced, all three stores let A overwrite `Canceled` with `Completed`, and
`tests/cross_replica_cancel/` reproduced it on each.

What this does **not** give you:

- **A clean stream on A.** Non-terminal frames A's executor emitted between
  the cancel and its next write still reach a client streaming from A —
  though never the store. The terminal frame is held until the store has
  ruled on it, so the ending is right.
- **An executor that honours the token.** Cancellation is cooperative. An
  executor that ignores `RequestContext::cancellation_token` keeps running on A
  until it returns, with its writes refused.

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

`tests/replica_processes.rs` runs replicas as separate processes over one
database, and kills one; everything else here runs two handlers in one
process. All of it is loopback on one machine: no real network between the
replicas and the database, no partition, no database failover, and no
connection pooler. A pooler in transaction mode (PgBouncer) does not change
the claims, which are single statements; it does stop the halt store's
`LISTEN`, which then falls back to its poll. If you are running this at scale
and have measurements, they would be welcome.
