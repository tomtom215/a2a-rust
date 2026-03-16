# Task & Config Stores

a2a-rust uses pluggable storage backends for tasks and push notification configs. The built-in in-memory stores work for development and testing. For production, implement the traits for your database.

## TaskStore Trait

The `TaskStore` trait defines how tasks are persisted:

```rust
pub trait TaskStore: Send + Sync + 'static {
    fn save(&self, task: Task)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + '_>>;

    fn get(&self, id: &TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + '_>>;

    fn list(&self, params: &ListTasksParams)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + '_>>;

    fn insert_if_absent(&self, task: Task)
        -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + '_>>;

    fn delete(&self, id: &TaskId)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + '_>>;

    /// Returns the total number of tasks. Default returns 0.
    fn count(&self)
        -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + '_>>;
}
```

### InMemoryTaskStore

The default implementation with optional TTL and capacity limits:

```rust
use a2a_protocol_sdk::server::{InMemoryTaskStore, TaskStoreConfig};
use std::time::Duration;

// Default: no limits
let store = InMemoryTaskStore::new();

// With limits
let store = InMemoryTaskStore::with_config(TaskStoreConfig {
    ttl: Some(Duration::from_secs(3600)),
    max_capacity: Some(10_000),
});
```

Features:
- Thread-safe (`DashMap`-based or `RwLock<HashMap>`)
- Automatic TTL eviction on access
- Capacity eviction (oldest first) when limit exceeded
- Pagination support with cursor tokens
- Filtering by `context_id`, `status`, and `timestamp`

### SqliteTaskStore (feature-gated)

Enable the `sqlite` feature for a production-ready persistent store:

```toml
[dependencies]
a2a-protocol-server = { version = "0.2", features = ["sqlite"] }
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
        store.save(task).await.unwrap();
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
    fn get(&self, tenant: Option<&str>, id: &str)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + '_>>
    {
        Box::pin(async move {
            let row = sqlx::query_as("SELECT data FROM tasks WHERE id = $1 AND tenant = $2")
                .bind(id)
                .bind(tenant)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| A2aError::internal(e.to_string()))?;

            Ok(row.map(|r| serde_json::from_value(r.data).unwrap()))
        })
    }

    // ... implement put, list, delete similarly
}
```

## PushConfigStore Trait

The `PushConfigStore` trait manages push notification configurations:

```rust
pub trait PushConfigStore: Send + Sync + 'static {
    fn create(&self, tenant: Option<&str>, config: TaskPushNotificationConfig)
        -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + '_>>;

    fn get(&self, tenant: Option<&str>, task_id: &str, id: &str)
        -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + '_>>;

    fn list(&self, tenant: Option<&str>, task_id: &str)
        -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + '_>>;

    fn delete(&self, tenant: Option<&str>, task_id: &str, id: &str)
        -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + '_>>;
}
```

### InMemoryPushConfigStore

The default implementation stores configs in a `HashMap`:

```rust
use a2a_protocol_sdk::server::InMemoryPushConfigStore;

let store = InMemoryPushConfigStore::new();
```

Features:
- Server-assigned config IDs (UUIDs)
- Per-task config limits (prevents abuse)
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

The `tenant` parameter is passed to every store operation. Your implementation should use it to partition data:

```sql
-- Good: tenant-aware query
SELECT * FROM tasks WHERE tenant = $1 AND id = $2;

-- Bad: no tenant isolation
SELECT * FROM tasks WHERE id = $1;
```

### Pagination

The `list` method receives `ListTasksParams` with:
- `page_size` — Number of results per page (1-100, default 50)
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
- **[Configuration Reference](../reference/configuration.md)** — All configuration options
