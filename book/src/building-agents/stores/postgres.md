<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# PostgreSQL task store

The PostgreSQL stores are the ones to use when several replicas share one store. This page covers setup, the connection pool, and what the store does when the database goes away for a moment. For partitioning by tenant or caller, see [Tenant-aware stores](./tenants.md); for what replicas sharing a store rely on, see [Stores shared by replicas](./replicas.md).

PostgreSQL-backed stores ship with the crate — `PostgresTaskStore`,
`PostgresPushConfigStore`, tenant-aware variants, and a forward-only
migration runner — and are exercised against a live PostgreSQL 16 service
in CI (`postgres_store_tests.rs`):

```toml
[dependencies]
a2a-protocol-server = { version = "0.15", features = ["postgres"] }
```

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use a2a_protocol_sdk::prelude::*;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::PostgresTaskStore;

let store = PostgresTaskStore::with_migrations("postgres://user:pass@localhost/a2a").await?;
let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_agent_card(card())
    .with_task_store(store)
    .build()?;
# Ok(())
# }
```

## The connection pool

`new` and `with_migrations` open a pool of 10 connections, as does
`TenantAwarePostgresTaskStore::new`. sqlx's default pings a pooled connection
every time it is handed out, a round trip before every statement; these pools
ping one only when it has been idle for a second or more, and a statement
that finds its connection gone is handled as the next section describes.

To size the pool yourself, build it from the same options with
`PostgresTaskStore::pool_options()` and pass it to `from_pool`:

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::PostgresTaskStore;

let pool = PostgresTaskStore::pool_options()
    .max_connections(20)
    .connect("postgres://user:pass@localhost/a2a")
    .await?;
let store = PostgresTaskStore::from_pool(pool).await?;
# Ok(())
# }
```

Share such a pool only with the PostgreSQL task stores, plain and
tenant-aware, which are written for it. The push-config, rate-limit and audit
stores are not: give them a pool with sqlx's defaults, or a statement of
theirs can meet a connection the server closed while it sat idle. A pool you
build without `pool_options` keeps its own settings; with sqlx's defaults it
pings before every statement.

## When the connection is lost or the database is busy

**A lost connection.** When a statement fails because its connection is gone
(an I/O error, SQLSTATE class `08`, or the server ending the backend: `57P01`,
`57P02`, `57P05`), the store runs it once more on a connection pinged first,
if a second run leaves the database as one run would: a read, a write keyed
by something only this caller holds, an append that does nothing at a
position already taken. Other statements, such as a delete keyed by
something another caller could take in between, are pinged before they run
instead. A lost connection does not prove the statement did not run, which
is why not everything is retried. Each second run is counted in
`a2a.server.store_retries` with `operation="statement"` and
`cause="connection_lost"`. This holds for a pool passed to `from_pool` too.

**A failover to a new primary.** After a failover that moves a DNS name or a
virtual IP, a connection still open to the old primary, now a standby,
answers every write with `25006` (`read_only_sql_transaction`) and every
ping with success, so nothing above would retire it. The first `25006` any
store in the process meets marks every connection opened before it: each
pool these stores open closes such a connection the next time it would hand
it out, and opens a new one, which resolves the name afresh. A statement
refused this way did not run, so the store runs it once more, counted with
`cause="read_only"`, and the error is `A2aError::unavailable` rather than
internal. A pool you build without `pool_options` does not retire its
connections this way. Until 2026-10-30 a `25006` was internal, and every
write failed until the old connections aged out of the pool (sqlx's default
`max_lifetime`, 30 minutes).

**"Not now" errors.** An error a retry may get past (no connection free
before the pool's acquire timeout, a closed pool, a lost connection, a
serialization failure or deadlock, a lock or statement timeout, too many
connections, a server starting up or shutting down, a read-only server) is returned as
`A2aError::unavailable`; a full disk and any other error stay
`A2aError::internal`. What a client sees for each is under
[Implementing a store](./custom.md#what-a-store-error-answers).

**A turn's event writes.** While a turn runs, the handler writes each event
to the task's log and to the task. A write the store answers as unavailable
is made again after 50 ms, then twice as long each time up to 1 s, for up to
`HandlerLimits::store_write_retry` (30 s by default; set it with
`RequestHandlerBuilder::with_handler_limits(HandlerLimits::default().with_store_write_retry(…))`).
The events after it wait, so the log keeps their order; each retry is counted
with `cause="unavailable"`. A write that still fails is logged and counted and
its event is lost; a `store_write_retry` of zero writes each event once. During an outage sqlx
itself waits up to its acquire timeout (30 s) for a connection, so one write
can take about a minute at the defaults; set `store_write_retry` past the
longest failover you expect beyond that. The measurements behind these
numbers are in
[Running More Than One Replica](../../deployment/horizontal-scaling.md).
