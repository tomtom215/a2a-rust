<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215) -->

# SQLite task store

`SqliteTaskStore` keeps tasks in one SQLite file: durable across restarts, with no database server to run. It suits one process, or processes on one host sharing the file; for replicas on several hosts, use [PostgreSQL](./postgres.md). For the tenant-aware variant, see [Tenant-aware stores](./tenants.md).

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
store with `sqlx` ([Implementing a store](./custom.md)), depends on `sqlx = "0.9"` (Rust 1.94 or later).

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
