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
| A task created on one replica is readable on another | **Yes** | The store holds it |
| `GetTask` / `ListTasks` see every replica's tasks | **Yes** | Same |
| A subscription terminates when the task finishes elsewhere | **Yes** | The reattach hook polls the store |
| A subscriber sees intermediate events from another replica | **No** | Event queues live in process memory |
| Rate limiting enforces the configured limit | **Only with a shared counter** | Otherwise each replica counts alone |

Sharing a task store gets you most of the way. The two things it does not
cover are streaming and rate limiting, and only one of them has a fix.

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
let limiter = RateLimitInterceptor::new(RateLimitConfig {
    requests_per_window: 100,
    window_secs: 60,
    ..Default::default()
})?
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

### The `tasks` table grows without bound

Visible in that run: 65,399 rows after 60 seconds, and nothing removes them.
A2A has no `DeleteTask` and `PostgresTaskStore` has no capacity eviction —
correctly, because a durable store silently dropping records would be worse
than one that grows.

The consequence is a retention policy you have to supply yourself. Nothing in
the SDK does it for you, and until this page there was nothing telling you so.
A periodic `DELETE FROM tasks WHERE updated_at < now() - interval '30 days'`
is the usual shape; pick the interval from how long your clients may reasonably
poll for a task after it finishes.

## What is still unevidenced

The replicas above are two handlers in one OS process, so nothing here can see
a defect that needs real process isolation. Both runs are loopback on one
machine: no real network, no failover, no partition. If you are running this at
scale and have measurements, they would be welcome.
