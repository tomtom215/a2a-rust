<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Sharing the Store

The one change that makes several replicas possible: every replica uses the
same PostgreSQL database. This page covers setting it up, what the stores do
when a connection to the database drops, and running through PgBouncer.

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
# use a2a_protocol_server::push::PostgresPushConfigStore;
# use a2a_protocol_server::store::PostgresTaskStore;
# use a2a_protocol_server::{agent_executor, RequestHandlerBuilder};
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _q| async { Ok(()) });
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let url = std::env::var("DATABASE_URL")?;
let handler = RequestHandlerBuilder::new(MyExecutor)
    .with_agent_card(card())
    .with_task_store(PostgresTaskStore::new(&url).await?)
    // Push configs too: one registered through one replica must be found by
    // the replica running the task. `build()` refuses a card that declares
    // push notifications over a shared task store without this.
    .with_push_config_store(PostgresPushConfigStore::new(&url).await?)
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

## When a connection drops

A statement whose connection the server ended (a restart, a failover,
`pg_terminate_backend`, an idle-session timeout) runs once more on a fresh
connection when a second run is harmless, and every other statement is
pinged first (`store::pg_retry`); with every backend ended mid-turn nothing
was lost (3 runs of 3). A write of a turn's event that the store refuses for
now (a lock or statement timeout, a deadlock) is made again, from 50 ms
doubling to 1 s, for up to `HandlerLimits::store_write_retry` (30 s); under
a 200 ms `statement_timeout` with the task row locked for 2 s, it lost three
parts and four log positions before, and none after. During an outage sqlx
itself waits up to its acquire timeout (30 s) for a connection before a
statement fails, so set `store_write_retry` past the longest failover you
expect beyond that.

```rust
use std::time::Duration;
use a2a_protocol_server::HandlerLimits;

// Past the longest failover you expect, on top of sqlx's acquire timeout.
let limits = HandlerLimits::default().with_store_write_retry(Duration::from_secs(60));
# let _ = limits;
```

Hand it to the handler with `RequestHandlerBuilder::with_handler_limits`.

Every retry is counted in `a2a.server.store_retries`, labelled by
`operation` and `cause`: `cause="unavailable"` for a turn's event write the
store refused for now (counted by the handler's metrics), and
`operation="statement"`, `cause="connection_lost"` for a statement run again
on a fresh connection (counted by the store's own metrics, which
`PostgresTaskStore::with_metrics` sets; a store you construct reports to
nothing until you do). A retry that succeeded lost nothing, so these are not
persistence errors; a rising rate is the database degrading before it starts
losing data.

A pool you build yourself and pass to `from_pool` keeps its own settings:
with sqlx's defaults it pings each connection before every statement, one
round trip more per statement. `PostgresTaskStore::pool_options()` returns
the settings `new` uses (ping only a connection idle for a second or more),
for a pool you size yourself.

## Behind PgBouncer

**Behind PgBouncer.** On 2026-10-29 the claim, replica and store suites
(`turn_claims`, `multi_replica`, `cross_replica_cancel`,
`replica_processes`, `stale_write_position`, `continuation_save_vs_cancel`,
`retried_statements`, `statement_count`, `postgres_store_tests`) ran with
every connection through PgBouncer 1.26.1 in transaction mode, with
`max_prepared_statements = 200` (sqlx prepares its statements, and a
transaction-mode pooler needs that setting to carry them), in front of
PostgreSQL 17.11, and so did a 30 s soak (`soak_multi_replica`, release
build, claims and the upgrade guard on): 20,807 request pairs, 694 a second,
0 failures, 0 cross-replica misses, 0 claims left. All passed but one test,
and that one is a setting, not the claims: a value set with `ALTER DATABASE … SET` reaches only server
connections opened after it, and PgBouncer hands clients connections it
opened before. So after setting `a2a.legacy_claim_ttl` (or a
`statement_timeout`) that way, have the pooler reconnect (`RECONNECT` on its
admin console) or wait for `server_lifetime`; until then the old value
holds. The claims, the upgrade guard's mark and the schema lock each last
one transaction, which a transaction-mode pooler keeps on one server
connection. A pooler does stop the halt store's `LISTEN`, which then falls
back to its poll.
