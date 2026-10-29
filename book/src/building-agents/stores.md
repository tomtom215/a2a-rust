# Task & Config Stores

a2a-rust uses pluggable storage backends for tasks and push notification configs. The built-in in-memory stores work for development and testing. For production, use the SQLite or PostgreSQL stores that ship with the crate (features `sqlite` / `postgres`), or implement the traits for your own database.

## TaskStore Trait

The `TaskStore` trait defines how tasks are persisted:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::types::error::A2aResult;
# use a2a_protocol_sdk::types::params::ListTasksParams;
# use a2a_protocol_sdk::types::responses::TaskListResponse;
# use a2a_protocol_sdk::types::task::{Task, TaskId};
pub trait TaskStore: Send + Sync + 'static {
    fn save<'a>(&'a self, task: &'a Task)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    fn get<'a>(&'a self, id: &'a TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>;

    fn list<'a>(&'a self, params: &'a ListTasksParams)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>>;

    fn insert_if_absent<'a>(&'a self, task: &'a Task)
        -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>>;

    fn delete<'a>(&'a self, id: &'a TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Returns the total number of tasks. Default returns 0.
    fn count<'a>(&'a self)
        -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>>;

    // Further provided methods have defaults; see the API docs: idempotency
    // keys, delta saves, `isolates_tenants`, `get_with_position`, `database`,
    // an event log for stream resumption, and
    // `save_appending_history(task, messages, limit: HistoryLimit)`. A store
    // that overrides the last one trims with `limit.trim(&mut history)`, which
    // bounds the history by messages and by bytes.
}
# // The signatures above are the real trait's, and they are all it requires:
# // this impl of it defines exactly these methods.
# struct Probe;
# impl a2a_protocol_sdk::server::store::TaskStore for Probe {
#     fn save<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
#     fn get<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> { Box::pin(async { Ok(None) }) }
#     fn list<'a>(&'a self, _: &'a ListTasksParams) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> { Box::pin(async { Ok(TaskListResponse::new(vec![])) }) }
#     fn insert_if_absent<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> { Box::pin(async { Ok(true) }) }
#     fn delete<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
#     fn count<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> { Box::pin(async { Ok(0) }) }
# }
```

An error a store returns answers the request: `A2aError::internal` as HTTP
`500` and gRPC `INTERNAL`, which tell a client not to retry. When a retry may
get past it, because the database is busy, has no free connection or is
restarting, return `A2aError::unavailable` instead: HTTP `503` with status
`UNAVAILABLE`, gRPC `UNAVAILABLE` (§3.3.2), which a client retries. The SQL
stores do. A send and a push config's creation answer `500` either way: a
client re-sends those on `503` as refused before any work began, and a store
failure can come after the executor ran.

### InMemoryTaskStore

The default implementation with optional TTL and capacity limits:

```rust,no_run
use a2a_protocol_sdk::server::{InMemoryTaskStore, TaskStoreConfig};
use std::time::Duration;

// Default: 1hr TTL, 10k capacity
let store = InMemoryTaskStore::new();

// With custom limits
let store = InMemoryTaskStore::with_config(
    TaskStoreConfig::default()
        .with_task_ttl(Some(Duration::from_secs(7200))) // 2 hour TTL
        .with_max_capacity(Some(50_000)),
);
```

Features:
- Thread-safe (`RwLock` — concurrent readers, exclusive writers)
- Pre-allocated `HashMap::with_capacity(max_capacity)` — eliminates resize-induced latency spikes under load
- O(1) `get()` via `HashMap`; `save()` and `delete()` also maintain the ordered indexes, so they are O(log n)
- **O(log n + page\_size) `list()` with secondary indexes**:
  - `BTreeMap<(i64, u64), TaskId>` order index keyed by the task's status
    timestamp (Unix milliseconds) and a monotonic per-write sequence that
    breaks ties — iterated in reverse to return tasks most-recently-updated
    first (spec §3.1.4: sorted by status timestamp, descending), with no
    O(n log n) per-call sort
  - `HashMap<String, BTreeMap<(i64, u64), TaskId>>` context index — O(log m + page\_size) filtered queries
  - The pair is a collision-free pagination cursor (the page token is
    `millis:seq`); a write that does not change the status timestamp, such as
    an artifact append, keeps the task's place in the list
- Automatic TTL eviction on write (every `eviction_interval` writes, or when you
  call `run_eviction()`); a read never evicts, so `get` can still return a
  terminal task past its TTL until the next sweep (maintains all indexes)
- Capacity eviction (oldest terminal tasks first; falls back to non-terminal tasks when needed) when limit exceeded — `save` does not return with the store over capacity. Measured with 16 concurrent writers against a capacity of 100: a concurrent reader saw a peak of 102 and a final count of exactly 100, so the transient overshoot is bounded by the number of writes in flight
- Cursor-based pagination, most-recently-updated first
- Filtering by `context_id` (index-accelerated) and `status`

### SqliteTaskStore (feature-gated)

Enable the `sqlite` feature for a production-ready persistent store:

```toml
[dependencies]
a2a-protocol-server = { version = "0.15", features = ["sqlite"] }
```

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::SqliteTaskStore;

let store = SqliteTaskStore::new("sqlite:tasks.db").await?;
// Or use an in-memory database for testing:
let store = SqliteTaskStore::new("sqlite::memory:").await?;
# Ok(())
# }
```

`SqliteTaskStore::with_migrations(url)` runs the versioned migrations (tracked
in a `schema_versions` table) instead of `CREATE TABLE IF NOT EXISTS`; use it
in production. `from_pool` takes a `sqlx` 0.9 `SqlitePool` (`PgPool` for
PostgreSQL), so an application that builds its own pool, or writes a custom
store with `sqlx` as below, depends on `sqlx = "0.9"` (Rust 1.94 or later).

Features:
- Auto-creates schema on first use
- Stores tasks as JSON blobs with indexed `context_id` and `state` columns
- Cursor-based pagination ordered by `(updated_at DESC, id DESC)` — tasks
  most-recently-updated first (spec §3.1.4), with a composite row-value cursor
  that never drops or repeats a row even when timestamps tie
- Atomic `insert_if_absent` via `INSERT OR IGNORE`
- Upsert via `ON CONFLICT DO UPDATE`
- **Production-ready defaults** on every connection the store opens: WAL
  journal mode, `busy_timeout=5000ms`, `synchronous=NORMAL`, `foreign_keys=ON`,
  `secure_delete=ON`, and a pool of 8. `secure_delete` zeroes deleted content in
  the database file, so `delete` and `purge_expired` do not leave it in a free
  page; it does not reach the write-ahead log until a checkpoint truncates it,
  nor a backup. A pool you pass to `from_pool` keeps your own pragmas.

### TenantAwareInMemoryTaskStore

For multi-tenant deployments, use `TenantAwareInMemoryTaskStore` which provides full tenant isolation using `tokio::task_local!`:

```rust
# use a2a_protocol_sdk::types::task::{ContextId, Task, TaskId, TaskState, TaskStatus};
# #[tokio::main]
# async fn main() -> () {
# let task_id = TaskId::new("task-1");
# let task = Task { id: task_id.clone(), context_id: ContextId::new("ctx-1"),
#     status: TaskStatus::new(TaskState::Submitted), history: None, artifacts: None, metadata: None };
use a2a_protocol_server::store::{TaskStore, TenantAwareInMemoryTaskStore, TenantContext};
use std::sync::Arc;

let store = Arc::new(TenantAwareInMemoryTaskStore::new());

// Each tenant gets an independent store instance.
// Use TenantContext::scope() to set the active tenant:
TenantContext::scope("tenant-alpha".to_string(), {
    let store = store.clone();
    async move {
        store.save(&task).await.unwrap();
    }
}).await;

// Tasks saved under one tenant are invisible to others:
TenantContext::scope("tenant-beta".to_string(), {
    let store = store.clone();
    async move {
        let result = store.get(&task_id).await.unwrap();
        assert!(result.is_none()); // tenant-beta can't see tenant-alpha's task
    }
}).await;

// Track tenant count for capacity monitoring:
let count = store.tenant_count().await;
# }
```

The `TenantContext::scope()` pattern uses `tokio::task_local!` to thread the tenant ID through the async call stack without passing it as a parameter. The `RequestHandler` sets the tenant scope for every request from `params.tenant`, or from the `TenantResolver` when one is configured (the resolver's tenant wins, and a different `params.tenant` is refused).

### TenantAwareSqliteTaskStore (feature-gated)

For persistent multi-tenant storage, enable the `sqlite` feature:

```rust,no_run
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
use a2a_protocol_server::store::TenantAwareSqliteTaskStore;

let store = TenantAwareSqliteTaskStore::new("sqlite:tasks.db").await?;
# Ok(())
# }
```

This variant reads the tenant from the same `TenantContext` and stores it in a `tenant_id` column (under caller isolation, the tenant and the caller), so its data lives in the database rather than in process memory, which suits production deployments where tenants may span multiple server instances.

> **Note:** Corresponding `TenantAwareInMemoryPushConfigStore` and `TenantAwareSqlitePushConfigStore` variants exist for push notification config storage.

### PostgresTaskStore (`postgres` feature)

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

**Behind an authenticating interceptor** the handler gives each identified
caller its own partition (spec §13.1), and `build()` refuses a task store and
push-config store that do not partition by tenant. `SqliteTaskStore` and
`PostgresTaskStore` do not, so use `TenantAwareSqliteTaskStore` or
`TenantAwarePostgresTaskStore` with the matching `TenantAware…PushConfigStore`,
or call `RequestHandlerBuilder::share_tasks_across_callers()` to keep one set
of records per tenant. Left unset, both default to the tenant-aware in-memory
stores, whose per-partition capacity (`with_task_store_config`, 10,000 tasks by
default) applies per caller. See
[Multi-Tenancy](../deployment/multi-tenancy.md#callers-within-one-tenant).

### Custom Implementation

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::types::error::A2aResult;
# use a2a_protocol_sdk::types::params::ListTasksParams;
# use a2a_protocol_sdk::types::responses::TaskListResponse;
# use a2a_protocol_sdk::types::task::{Task, TaskId};
# use a2a_protocol_sdk::types::error::A2aError;
# use a2a_protocol_sdk::server::store::TaskStore;
struct PgJsonTaskStore {
    pool: sqlx::PgPool,
}

impl TaskStore for PgJsonTaskStore {
    fn get<'a>(&'a self, id: &'a TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>
    {
        Box::pin(async move {
            let data: Option<serde_json::Value> =
                sqlx::query_scalar("SELECT data FROM tasks WHERE id = $1")
                    .bind(id.as_ref())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| A2aError::internal(e.to_string()))?;

            data.map(serde_json::from_value)
                .transpose()
                .map_err(|e| A2aError::internal(format!("stored task does not parse: {e}")))
        })
    }

    // ... implement save, list, delete, insert_if_absent similarly
#     fn save<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
#     fn list<'a>(&'a self, _: &'a ListTasksParams) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> { unimplemented!() }
#     fn insert_if_absent<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> { unimplemented!() }
#     fn delete<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
}
```

## PushConfigStore Trait

The `PushConfigStore` trait manages push notification configurations:

```rust
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::types::error::A2aResult;
# use a2a_protocol_sdk::types::push::TaskPushNotificationConfig;
pub trait PushConfigStore: Send + Sync + 'static {
    fn set<'a>(&'a self, config: TaskPushNotificationConfig)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>>;

    fn get<'a>(&'a self, task_id: &'a str, id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>;

    fn list<'a>(&'a self, task_id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>>;

    fn delete<'a>(&'a self, task_id: &'a str, id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    // Optional: fn isolates_tenants(&self) -> bool, false by default (see
    // Tenant Isolation below).

    // Optional: total stored configs, counted against the handler's global
    // push-config ceiling. The default implementation returns `None`
    // (only the per-task cap is then enforced), so existing custom
    // implementations keep compiling unchanged.
    fn count(&self)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<usize>>> + Send + '_>> {
        Box::pin(async { Ok(None) })
    }
}
# // The real trait requires exactly these four; this impl of it compiles.
# struct Probe;
# impl a2a_protocol_sdk::server::PushConfigStore for Probe {
#     fn set<'a>(&'a self, c: TaskPushNotificationConfig) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> { Box::pin(async { Ok(c) }) }
#     fn get<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>> { Box::pin(async { Ok(None) }) }
#     fn list<'a>(&'a self, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> { Box::pin(async { Ok(vec![]) }) }
#     fn delete<'a>(&'a self, _: &'a str, _: &'a str) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { Box::pin(async { Ok(()) }) }
# }
```

### InMemoryPushConfigStore

The default implementation stores configs in a `HashMap` with a secondary index for efficient per-task counting:

```rust,no_run
use a2a_protocol_sdk::server::InMemoryPushConfigStore;

let store = InMemoryPushConfigStore::new();
```

Features:
- Config IDs (a UUID is assigned when the config names none)
- Per-task config limits (prevents abuse) — uses a secondary index (`task_counts`) for O(1) per-task count lookups instead of scanning all keys
- Global config limit (`max_total_configs`, default 100,000) — prevents unbounded memory growth across all tasks
- Thread-safe access

## Wiring Custom Stores

```rust,no_run
# fn card() -> a2a_protocol_types::agent_card::AgentCard {
#     use a2a_protocol_types::agent_card::{AgentCapabilities, AgentCard, AgentInterface, AgentSkill};
#     AgentCard::new("my-agent", "1.0.0", AgentInterface::jsonrpc("http://localhost:3000"))
#         .with_description("An example agent")
#         .with_skill(AgentSkill::new("echo", "Echo", "Echoes text").with_tags(["echo"]))
#         .with_capabilities(AgentCapabilities::none().with_streaming(true))
# }
# use std::future::Future;
# use std::pin::Pin;
# use a2a_protocol_sdk::prelude::*;
# use a2a_protocol_sdk::types::task::TaskId;
# use a2a_protocol_sdk::server::store::TaskStore;
# struct MyExecutor;
# agent_executor!(MyExecutor, |_ctx, _queue| async { Ok(()) });
# struct PgJsonTaskStore { pool: sqlx::PgPool }
# impl TaskStore for PgJsonTaskStore {
#     fn save<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
#     fn get<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> { unimplemented!() }
#     fn list<'a>(&'a self, _: &'a ListTasksParams) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> { unimplemented!() }
#     fn insert_if_absent<'a>(&'a self, _: &'a Task) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> { unimplemented!() }
#     fn delete<'a>(&'a self, _: &'a TaskId) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> { unimplemented!() }
# }
# #[tokio::main]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
# let executor = MyExecutor;
# let database_url = "postgres://user:pass@localhost/a2a";
use a2a_protocol_server::PostgresPushConfigStore;

let pool = sqlx::PgPool::connect(database_url).await?;
let handler = RequestHandlerBuilder::new(executor)
    .with_agent_card(card())
    .with_task_store(PgJsonTaskStore { pool })
    .with_push_config_store(PostgresPushConfigStore::new(database_url).await?)
    .build()?;
# Ok(())
# }
```

## Retention, Erasure and Audit

The in-memory store forgets: terminal tasks expire after `task_ttl` (one hour by
default). The SQL stores keep every task until you say otherwise. Call
`purge_expired(&RetentionPolicy::new(max_age))` on a concrete store from your
own scheduler; it deletes only `Completed`, `Failed`, `Canceled` and `Rejected`
tasks, in batches, and is safe to run from several replicas
([Running More Than One Replica](../deployment/horizontal-scaling.md)).

The tenant-aware stores also have `erase_caller(identity)`, which removes a
caller's partitions with their tasks, logged events and idempotency keys (the
push-config stores, its webhooks); the SQL stores do it in one transaction.
`migrate_caller_keys(map)` (SQLite and PostgreSQL, task and push-config stores)
moves partitions stored under the bare caller names earlier releases used to
`bearer:<label>`, `api-key:<label>` or `jwt:<issuer>:<sub>`; run it once per
store before serving 0.15
([Upgrading](../reference/upgrading.md#a-caller-is-named-by-how-it-authenticated)).

With `audit`, a task deleted through the audited task store is recorded in the
hash chain. `purge_expired` and `erase_caller` are calls you make outside any
request, so you report what they returned yourself
([Audit Trail](../deployment/audit.md#erasure-of-tasks-is-on-record-too)). With
`encryption`, `store::encrypted::EncryptedTaskStore` seals the parts of a
task's history, status message and artifacts before they reach the wrapped
store.

## Design Considerations

### Object Safety

Both traits use `Pin<Box<dyn Future>>` return types for object safety. This allows the handler to store them as `Arc<dyn TaskStore>`.

### Tenant Isolation

Tenant isolation uses `tokio::task_local!` via `TenantContext::scope()`, not method parameters. For in-memory stores, `TenantAwareInMemoryTaskStore` automatically partitions data by tenant. For SQL stores, `TenantAwareSqliteTaskStore` partitions by a `tenant_id` column. If you implement a custom store that partitions by tenant, key by `TenantContext::partition()` (the tenant, plus the caller when the handler isolates callers), and override `isolates_tenants()` to return `true` on both `TaskStore` and `PushConfigStore`. A store that leaves it `false` makes the handler refuse every request that names a tenant (unless the builder was told `accept_unisolated_tenants()`), and `build()` refuse the store behind an authenticating interceptor. A wrapper store must forward it.

### TaskStoreConfig Fields

**`TaskStoreConfig` configures `InMemoryTaskStore` only.** The SQL stores take
a connection URL, not a config — `max_capacity`, `task_ttl` and
`eviction_interval` have no meaning for them (the database holds the rows, and
retention is `purge_expired`; see
[Retention, Erasure and Audit](#retention-erasure-and-audit)), and each carries
its own page-size cap via
`with_max_page_size`. Until 2026-08-19 that cap was a hardcoded `1000` that
happened to equal this table's default, so setting `max_page_size` to something
tighter changed the in-memory store and nothing else: measured with the cap at
10 and a client asking for 100, the in-memory store returned 10 and SQLite
returned all 60 rows it held.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_capacity` | `Option<usize>` | `Some(10_000)` | Max tasks in store; oldest evicted when exceeded |
| `task_ttl` | `Option<Duration>` | `Some(1 hour)` | TTL for terminal-state tasks; `None` disables eviction |
| `eviction_interval` | `u64` | 64 | Writes between automatic eviction sweeps (amortizes the sweep's cost) — see the note below |
| `max_page_size` | `u32` | 1000 | Maximum allowed page size for list queries |
| `max_events_per_task` | `Option<usize>` | `Some(512)` | Events one task's log keeps for stream resumption; `None` removes the bound |
| `max_held_events_factor` | `Option<usize>` | `Some(8)` | Times `max_events_per_task` a log may hold while an attached stream still owes its oldest events |
| `idempotency_key_ttl` | `Option<Duration>` | `Some(24 h)` | How long a client's idempotency key is honoured; `None` keeps keys for the life of the process |
| `max_idempotency_keys` | `Option<usize>` | `Some(100_000)` | Keys held at once; a send with a new key past it is refused as temporarily unavailable (HTTP `503`; gRPC `RESOURCE_EXHAUSTED`) |

#### `eviction_interval` is a tail-latency knob

The TTL sweep is not spawned: it is awaited inside `save`, and it holds the
store's write lock for its whole duration, so the write that triggers it pays
for it and every concurrent writer waits.

The sweep walks only the tasks last written at least `task_ttl` ago (an index
ordered by write time) and every held idempotency key (`idempotency_key_ttl`),
so what it costs grows with the expired tasks and the held keys, not with the
store size. The interval keeps it off most writes. The cost of one sweep has
not been measured on this code.

If p99.9 write latency matters to you, raise `eviction_interval` and drive
cleanup yourself with `run_eviction()` on your own schedule, so the stall lands
where you chose it. The capacity pass is separate: it runs on any write that
leaves the store over `max_capacity` and removes only the overflow, oldest
first, rather than scanning for it.

### Pagination

The `list` method receives `ListTasksParams` with:
- `page_size` — Number of results per page. The handler refuses a value
  outside 1 to 100 before your store sees it, and an unset value lists 50. A
  store may cap it lower: `TaskStoreConfig::max_page_size` on
  `InMemoryTaskStore` and `with_max_page_size` on each SQL store (default
  `DEFAULT_MAX_PAGE_SIZE`, 1,000, so the cap binds only when set below 100)
- `page_token` — Opaque cursor for the next page
- Various filter fields

Your implementation should return a `TaskListResponse` with a `next_page_token`
if more results exist. Per spec §3.1.4, tasks **must** be returned
most-recently-updated first; the built-in stores order by last-update time
(descending) with a stable cursor, and a custom store should do the same so
pagination is deterministic across updates. The `page_token` is opaque — encode
whatever your ordering needs into it; treat a token you did not issue as an
empty page rather than scanning from the top.

### Concurrency

Both traits require `Send + Sync`, and every method takes `&self`. Use a
connection pool rather than a single connection: a query needs `&mut` access
to a connection, so a store holding one has to put it behind a lock, and every
call then waits for the one before it.

```rust
// Good — the pool hands each call its own connection
struct MyStore { pool: sqlx::PgPool }

// Compiles, but serializes every store call behind one connection
struct MySerialStore { conn: tokio::sync::Mutex<sqlx::PgConnection> }
# fn assert_send_sync<T: Send + Sync>() {}
# assert_send_sync::<MyStore>();
# assert_send_sync::<MySerialStore>();
```

### Terminal states are final

Every store shipped here refuses a write — `save` or any of the delta
methods — that would move a stored `Completed`, `Failed`, `Canceled` or
`Rejected` task to a different state, and applies the check inside the write
itself: a `WHERE` condition on the SQL `UPDATE` or upsert, the write lock for
the in-memory stores. Re-writing the same terminal state is allowed. The
refusal is an `UnsupportedOperation` error carrying a `TerminalStateConflict`,
and the server reacts to it: `CancelTask` answers `TaskNotCancelable`, and the
event processors adopt the stored state and cancel the local executor.

This is what makes a `CancelTask` on one replica stick against an executor
still running on another (see
[Running More Than One Replica](../deployment/horizontal-scaling.md)). A custom
store does not get it for free. To give the same guarantee, apply
`store::refuses_write(stored, written)` atomically with each write, and report
a refusal with `TerminalStateConflict::into_error`. A read followed by a write
is not enough — that is the race the rule exists to close.

### The log position a stored task reflects

A `SubscribeToTask` on a replica with no live queue for the task starts from
the stored task and replays the task's event log after the position that task
reflects. A processor logs each event before it stores the state the event
leads to, so on another replica the log's end can be one event ahead of the
stored task. Only the store can say which events a stored task already holds:
`TaskStore::get_with_position` returns the task and that position, read
together.

Every store shipped here keeps it. The processors run each event's writes
inside `store::AppliedPosition::scope(seq, …)`; a store reads
`AppliedPosition::current()` in each write method and stores it in the same
atomic write (the SQL stores' `applied_seq` column), keeping the stored value
when it is `None`. A write outside a scope, such as the send path's history
append, changes no logged event's effect, so it keeps the stored position.

Two cases it does not cover. A write that read the task before another
replica's write landed and saved over it keeps that write's position while
losing its effect; that lost update predates the position, and turn claims
(below) are what prevent it. And a replica on
an earlier release does not write the position, so during a rolling upgrade a
subscriber can receive events its snapshot already holds a second time.
Duplicates, not losses, and they stop once every replica runs this release.

A custom store that keeps a log should do the same, and override
`get_with_position` to read the task and its position together. One that does
not keeps the trait's default, no position, and a subscription falls back to
reading the log's end and then the task, which can lose the one event another
replica has logged and not yet stored, or repeat one it logged and stored
between the two reads.

The SQL stores add the column themselves: migration 8 (`SQLite`) and 7
(`PostgreSQL`) for `with_migrations`, and `from_pool` for a database an earlier
version created.

### Turn claims: one replica at a time per task

Several replicas sharing a store must not run one task at once, and a
replica that stalled must not write over the turn that another replica took
over meanwhile ([Running More Than One Replica](../deployment/horizontal-scaling.md#one-turn-of-a-task-at-a-time-on-whichever-replica)).
The handler handles the first with a claim per turn, taken from
`TaskStore::turn_claims`, and needs the store for the second.

The SQL stores provide both. A custom store that several replicas share
should:

1. **Override `turn_claims`** to return a `store::TurnClaimStore` over storage
   every replica shares, and in the same database as its tasks, since the
   second step reads it. `PostgresTurnClaims::from_pool` and
   `SqliteTurnClaims::from_pool` are there to reuse when the store already
   runs on one of those. One of your own must renew only the tokens
   `renew` names, never every claim of the owner: a claim the handler no
   longer tracks has to expire.
2. **Fence every task write and event-log append.** Read
   `store::claim::current_fence()` at the start of each write method, before
   anything is spawned (it is a task-local, like the tenant). When it is
   `Some(token)`, make the write conditional, in the same atomic step, on that
   token still holding a claim (the SQL stores add `EXISTS (SELECT 1 FROM
   a2a_task_claims WHERE token = $n)` to the statement), and when it refuses,
   return `ClaimLost::new(task_id).into_error()`. A write with no fence (a
   `CancelTask`, which must win wherever the task runs) is not checked. On a
   database whose statements read a snapshot taken when they start
   (PostgreSQL's READ COMMITTED), lock the claim row in that condition (the
   PostgreSQL stores add `FOR KEY SHARE`), or a write that waited on the task
   row through a takeover passes on the claim as it was.

A store that takes the default, `None`, gets neither: the handler takes no
claims and refuses a second send to a running task only within its own
process. That is right for a store one process uses; with several replicas,
route every turn of a task to one of them. The guard that keeps a replica of
an earlier release off a task during a rolling upgrade
([Running More Than One Replica](../deployment/horizontal-scaling.md#upgrading-a-fleet-to-this-release))
belongs to the bundled PostgreSQL stores; a custom store's own upgrade is its
own. `with_turn_claims` on the builder
supplies claims for a store that does not provide them, but the fence is still
the store's to apply. A wrapper that forwards to another store must forward
`turn_claims` (`scripts/check_store_delta_forwarding.py` checks the ones in
this repository).

The SQL stores create the claims table themselves: migration 9 (`SQLite`) and
8 (`PostgreSQL`) for `with_migrations`, and `from_pool` otherwise.

## Next Steps

- **[Production Hardening](../deployment/production.md)** — Deployment checklist
- **[Configuration Reference](../reference/configuration.md)** — Every builder and store default
