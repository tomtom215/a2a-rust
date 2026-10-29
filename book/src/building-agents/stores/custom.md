<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# Implementing a store

Implement `TaskStore` (and `PushConfigStore`) for a database the SDK does not ship a store for. This page covers the trait, what an error from it answers, a worked example, and the contract the handler relies on. A store that several replicas share has more to provide; see [Stores shared by replicas](./replicas.md).

## The `TaskStore` trait

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

## What a store error answers

An error a store returns answers the request: `A2aError::internal` as HTTP
`500` and gRPC `INTERNAL`, which tell a client not to retry. When a retry may
get past it, because the database is busy, has no free connection or is
restarting, return `A2aError::unavailable` instead: HTTP `503` with status
`UNAVAILABLE`, gRPC `UNAVAILABLE` (§3.3.2), which a client retries. The SQL
stores do. A send and a push config's creation answer `500` either way: a
client re-sends those on `503` as refused before any work began, and a store
failure can come after the executor ran.

The handler makes a turn's event writes again while the store answers
unavailable, for up to `HandlerLimits::store_write_retry`
([PostgreSQL](./postgres.md#when-the-connection-is-lost-or-the-database-is-busy)).
Every write it makes leaves the store as one would when made again (an
upsert, a compare-and-set, an append keyed by position), so a write that
applied before its answer was lost is not applied twice. A custom store
must keep that true of its own writes.

## A custom implementation

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

## Wiring custom stores

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

## Object safety

Both traits use `Pin<Box<dyn Future>>` return types for object safety. This allows the handler to store them as `Arc<dyn TaskStore>`.

## Pagination

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

## Concurrency

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
