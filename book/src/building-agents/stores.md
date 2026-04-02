# Task & Config Stores

a2a-rust uses pluggable storage backends for tasks and push notification configs. The built-in in-memory stores work for development and testing. For production, implement the traits for your database.

## TaskStore Trait

The `TaskStore` trait defines how tasks are persisted:

```rust
pub trait TaskStore: Send + Sync + 'static {
    fn save<'a>(&'a self, task: Task)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    fn get<'a>(&'a self, id: &'a TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>;

    fn list<'a>(&'a self, params: &'a ListTasksParams)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>>;

    fn insert_if_absent<'a>(&'a self, task: Task)
        -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>>;

    fn delete<'a>(&'a self, id: &'a TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;

    /// Returns the total number of tasks. Default returns 0.
    fn count<'a>(&'a self)
        -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>>;
}
```

### InMemoryTaskStore

The default implementation with optional TTL and capacity limits:

```rust
use a2a_protocol_sdk::server::{InMemoryTaskStore, TaskStoreConfig};
use std::time::Duration;

// Default: 1hr TTL, 10k capacity
let store = InMemoryTaskStore::new();

// With custom limits
let store = InMemoryTaskStore::with_config(TaskStoreConfig {
    task_ttl: Some(Duration::from_secs(7200)),  // 2 hour TTL
    max_capacity: Some(50_000),
    ..Default::default()
});
```

Features:
- Thread-safe (`RwLock` — concurrent readers, exclusive writers)
- Pre-allocated `HashMap::with_capacity(max_capacity)` — eliminates resize-induced latency spikes under load
- O(1) amortized `save()`/`get()`/`delete()` via `HashMap` (no log(n) tree traversal overhead)
- **O(log n + page\_size) `list()` with secondary indexes**:
  - `BTreeSet<TaskId>` sorted index — eliminates O(n log n) per-call sort
  - `HashMap<String, BTreeSet<TaskId>>` context index — O(log m + page\_size) filtered queries
  - Uses `BTreeSet::range()` for O(log n) cursor positioning
- Automatic TTL eviction on access (maintains all indexes)
- Capacity eviction (oldest terminal tasks first; falls back to non-terminal tasks when needed) when limit exceeded — hard capacity guarantee
- Cursor-based pagination with deterministic ordering
- Filtering by `context_id` (index-accelerated) and `status`

### SqliteTaskStore (feature-gated)

Enable the `sqlite` feature for a production-ready persistent store:

```toml
[dependencies]
a2a-protocol-server = { version = "0.5", features = ["sqlite"] }
```

```rust
use a2a_protocol_server::store::SqliteTaskStore;

let store = SqliteTaskStore::new("sqlite:tasks.db").await?;
// Or use an in-memory database for testing:
let store = SqliteTaskStore::new("sqlite::memory:").await?;
```

Features:
- Auto-creates schema on first use
- Stores tasks as JSON blobs with indexed `context_id` and `state` columns
- Cursor-based pagination via `id > ?` ordering
- Atomic `insert_if_absent` via `INSERT OR IGNORE`
- Upsert via `ON CONFLICT DO UPDATE`
- **Production-ready defaults:** WAL journal mode, `busy_timeout=5000ms`,
  `synchronous=NORMAL`, `foreign_keys=ON`, pool size of 8

### TenantAwareInMemoryTaskStore

For multi-tenant deployments, use `TenantAwareInMemoryTaskStore` which provides full tenant isolation using `tokio::task_local!`:

```rust
use a2a_protocol_server::store::{TenantAwareInMemoryTaskStore, TenantContext};
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
```

The `TenantContext::scope()` pattern uses `tokio::task_local!` to thread the tenant ID through the async call stack without passing it as a parameter. The `RequestHandler` automatically sets the tenant scope when `params.tenant` is populated.

### TenantAwareSqliteTaskStore (feature-gated)

For persistent multi-tenant storage, enable the `sqlite` feature:

```rust
use a2a_protocol_server::store::TenantAwareSqliteTaskStore;

let store = TenantAwareSqliteTaskStore::new("sqlite:tasks.db").await?;
```

This variant partitions data by a `tenant_id` column instead of using task-local storage, making it suitable for production deployments where tenants may span multiple server instances.

> **Note:** Corresponding `TenantAwareInMemoryPushConfigStore` and `TenantAwareSqlitePushConfigStore` variants exist for push notification config storage.

### Custom Implementation

```rust
struct PostgresTaskStore {
    pool: sqlx::PgPool,
}

impl TaskStore for PostgresTaskStore {
    fn get<'a>(&'a self, id: &'a TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>>
    {
        Box::pin(async move {
            let row = sqlx::query_as("SELECT data FROM tasks WHERE id = $1")
                .bind(id.as_ref())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?;

            Ok(row.map(|r| serde_json::from_value(r.data).unwrap()))
        })
    }

    // ... implement save, list, delete, insert_if_absent, count similarly
}
```

## PushConfigStore Trait

The `PushConfigStore` trait manages push notification configurations:

```rust
pub trait PushConfigStore: Send + Sync + 'static {
    fn set<'a>(&'a self, config: TaskPushNotificationConfig)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>>;

    fn get<'a>(&'a self, task_id: &'a str, id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>;

    fn list<'a>(&'a self, task_id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>>;

    fn delete<'a>(&'a self, task_id: &'a str, id: &'a str)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>>;
}
```

### InMemoryPushConfigStore

The default implementation stores configs in a `HashMap` with a secondary index for efficient per-task counting:

```rust
use a2a_protocol_sdk::server::InMemoryPushConfigStore;

let store = InMemoryPushConfigStore::new();
```

Features:
- Server-assigned config IDs (UUIDs)
- Per-task config limits (prevents abuse) — uses a secondary index (`task_counts`) for O(1) per-task count lookups instead of scanning all keys
- Global config limit (`max_total_configs`, default 100,000) — prevents unbounded memory growth across all tasks
- Thread-safe access

## Wiring Custom Stores

```rust
let handler = RequestHandlerBuilder::new(executor)
    .with_task_store(PostgresTaskStore::new(pool.clone()))
    .with_push_config_store(PostgresPushConfigStore::new(pool))
    .build()
    .unwrap();
```

## Design Considerations

### Object Safety

Both traits use `Pin<Box<dyn Future>>` return types for object safety. This allows the handler to store them as `Arc<dyn TaskStore>`.

### Tenant Isolation

Tenant isolation uses `tokio::task_local!` via `TenantContext::scope()`, not method parameters. For in-memory stores, `TenantAwareInMemoryTaskStore` automatically partitions data by tenant. For SQL stores, `TenantAwareSqliteTaskStore` partitions by a `tenant_id` column. If you implement a custom store, use `TenantContext::current()` to retrieve the active tenant within the async call stack.

### TaskStoreConfig Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_capacity` | `Option<usize>` | `Some(10_000)` | Max tasks in store; oldest evicted when exceeded |
| `task_ttl` | `Option<Duration>` | `Some(1 hour)` | TTL for terminal-state tasks; `None` disables eviction |
| `eviction_interval` | `u64` | 64 | Writes between automatic eviction sweeps (amortizes O(n) cost) |
| `max_page_size` | `u32` | 1000 | Maximum allowed page size for list queries |

### Pagination

The `list` method receives `ListTasksParams` with:
- `page_size` — Number of results per page (capped by `TaskStoreConfig::max_page_size`, default 1,000)
- `page_token` — Opaque cursor for the next page
- Various filter fields

Your implementation should return a `TaskListResponse` with a `next_page_token` if more results exist.

### Concurrency

Both traits require `Send + Sync`. Use connection pools, not single connections:

```rust
// Good
struct MyStore { pool: sqlx::PgPool }

// Bad — not Send + Sync
struct MyStore { conn: sqlx::PgConnection }
```

## Next Steps

- **[Production Hardening](../deployment/production.md)** — Deployment checklist
- **[Configuration Reference](../