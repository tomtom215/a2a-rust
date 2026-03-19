// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Schema versioning and migration support for [`SqliteTaskStore`](super::SqliteTaskStore).
//!
//! This module provides a lightweight, forward-only migration runner that tracks
//! applied schema versions in a `schema_versions` table. Migrations are defined
//! as plain SQL strings and are executed inside transactions for atomicity.
//!
//! # Concurrency
//!
//! Each migration runs inside a `BEGIN EXCLUSIVE` transaction, which acquires
//! a database-level write lock before reading. This prevents concurrent
//! migration runners from both seeing the same version as unapplied and
//! attempting to apply it simultaneously.
//!
//! # Built-in migrations
//!
//! | Version | Description |
//! |---------|-------------|
//! | 1 | Initial schema — `tasks` table with indexes on `context_id` and `state` |
//! | 2 | Add `created_at` column to `tasks` table |
//! | 3 | Add composite index on `(context_id, state)` for combined filter queries |
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_server::store::migration::MigrationRunner;
//! use sqlx::sqlite::SqlitePoolOptions;
//!
//! # async fn example() -> Result<(), sqlx::Error> {
//! let pool = SqlitePoolOptions::new()
//!     .connect("sqlite:tasks.db")
//!     .await?;
//!
//! let runner = MigrationRunner::new(pool);
//! let applied = runner.run_pending().await?;
//! println!("Applied migrations: {applied:?}");
//! # Ok(())
//! # }
//! ```

use sqlx::sqlite::SqlitePool;
use sqlx::Row;

/// A single schema migration.
///
/// Each migration has a unique monotonically increasing version number, a
/// human-readable description, and one or more SQL statements to execute.
#[derive(Debug, Clone)]
pub struct Migration {
    /// Unique version number. Must be greater than zero and monotonically
    /// increasing across the migration list.
    pub version: u32,
    /// Short human-readable description of the migration.
    pub description: &'static str,
    /// SQL statements to execute. Multiple statements can be separated by
    /// semicolons; they run inside a single transaction.
    pub sql: &'static str,
}

/// Built-in migrations for the `SqliteTaskStore` schema.
///
/// These are applied in order by [`MigrationRunner::run_pending`].
pub static BUILTIN_MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema: tasks table with context_id and state indexes",
        sql: "\
CREATE TABLE IF NOT EXISTS tasks (
    id         TEXT PRIMARY KEY,
    context_id TEXT NOT NULL,
    state      TEXT NOT NULL,
    data       TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_tasks_context_id ON tasks(context_id);
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);",
    },
    Migration {
        version: 2,
        description: "Add created_at column to tasks table",
        sql: "ALTER TABLE tasks ADD COLUMN created_at TEXT NOT NULL DEFAULT (datetime('now'));",
    },
    Migration {
        version: 3,
        description: "Add composite index on (context_id, state) for combined filter queries",
        sql: "CREATE INDEX IF NOT EXISTS idx_tasks_context_id_state ON tasks(context_id, state);",
    },
];

/// Runs schema migrations against a `SQLite` database.
///
/// `MigrationRunner` tracks which migrations have been applied in a
/// `schema_versions` table and only executes those that have not yet been
/// applied. Migrations are executed in version order inside transactions.
///
/// # Thread safety
///
/// The runner is safe to use from multiple tasks. Concurrent calls to
/// [`run_pending`](Self::run_pending) are safe because each migration
/// runs inside a `BEGIN EXCLUSIVE` transaction, which serializes access
/// at the database level.
#[derive(Debug, Clone)]
pub struct MigrationRunner {
    pool: SqlitePool,
    migrations: &'static [Migration],
}

impl MigrationRunner {
    /// Creates a new runner with the built-in migrations.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self {
            pool,
            migrations: BUILTIN_MIGRATIONS,
        }
    }

    /// Creates a new runner with a custom set of migrations.
    ///
    /// This is primarily useful for testing. In production, prefer [`new`](Self::new).
    #[must_use]
    pub const fn with_migrations(pool: SqlitePool, migrations: &'static [Migration]) -> Self {
        Self { pool, migrations }
    }

    /// Ensures the `schema_versions` tracking table exists.
    async fn ensure_version_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_versions (
                version     INTEGER PRIMARY KEY,
                description TEXT    NOT NULL,
                applied_at  TEXT    NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Returns the highest migration version that has been applied, or `0` if
    /// no migrations have been applied yet.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be queried.
    pub async fn current_version(&self) -> Result<u32, sqlx::Error> {
        self.ensure_version_table().await?;
        let row = sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_versions")
            .fetch_one(&self.pool)
            .await?;
        let version: i32 = row.get("v");
        #[allow(clippy::cast_sign_loss)]
        Ok(version as u32)
    }

    /// Returns the list of migrations that have not yet been applied.
    ///
    /// # Errors
    ///
    /// Returns an error if the current version cannot be determined.
    pub async fn pending_migrations(&self) -> Result<Vec<&Migration>, sqlx::Error> {
        let current = self.current_version().await?;
        Ok(self
            .migrations
            .iter()
            .filter(|m| m.version > current)
            .collect())
    }

    /// Applies all pending migrations in version order.
    ///
    /// Each migration runs inside its own transaction. If a migration fails,
    /// the transaction is rolled back and the error is returned; previously
    /// applied migrations in this call remain committed.
    ///
    /// Returns the list of version numbers that were applied.
    ///
    /// # Errors
    ///
    /// Returns an error if any migration fails to apply.
    pub async fn run_pending(&self) -> Result<Vec<u32>, sqlx::Error> {
        self.ensure_version_table().await?;

        let mut applied = Vec::new();

        for migration in self.migrations {
            // Acquire a raw connection and use BEGIN EXCLUSIVE to prevent
            // concurrent migration runners from both seeing the same version
            // as unapplied. The exclusive lock serializes the version check +
            // migration apply into a single atomic operation.
            let mut conn = self.pool.acquire().await?;
            sqlx::query("BEGIN EXCLUSIVE").execute(&mut *conn).await?;

            // Re-check the current version inside the exclusive lock to
            // prevent TOCTOU races with concurrent runners.
            let row = sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_versions")
                .fetch_one(&mut *conn)
                .await?;
            let current: i32 = row.get("v");
            #[allow(clippy::cast_sign_loss)]
            let current = current as u32;

            if migration.version <= current {
                // Already applied by a concurrent runner; roll back and skip.
                sqlx::query("ROLLBACK").execute(&mut *conn).await?;
                continue;
            }

            // Execute each statement in the migration SQL separately inside
            // the transaction. SQLite does not support multiple statements in
            // a single `sqlx::query` call.
            for statement in migration.sql.split(';') {
                let trimmed = statement.trim();
                if trimmed.is_empty() {
                    continue;
                }
                sqlx::query(trimmed).execute(&mut *conn).await?;
            }

            // Record the migration as applied.
            sqlx::query("INSERT INTO schema_versions (version, description) VALUES (?1, ?2)")
                .bind(migration.version)
                .bind(migration.description)
                .execute(&mut *conn)
                .await?;

            sqlx::query("COMMIT").execute(&mut *conn).await?;
            applied.push(migration.version);
        }

        Ok(applied)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Helper to create an in-memory `SQLite` pool.
    async fn memory_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("failed to open in-memory sqlite")
    }

    #[tokio::test]
    async fn current_version_starts_at_zero() {
        let pool = memory_pool().await;
        let runner = MigrationRunner::new(pool);
        assert_eq!(runner.current_version().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn run_pending_applies_all_builtin_migrations() {
        let pool = memory_pool().await;
        let runner = MigrationRunner::new(pool.clone());

        let applied = runner.run_pending().await.unwrap();
        assert_eq!(applied, vec![1, 2, 3]);
        assert_eq!(runner.current_version().await.unwrap(), 3);

        // Verify the tasks table exists with the expected columns.
        let row = sqlx::query("PRAGMA table_info(tasks)")
            .fetch_all(&pool)
            .await
            .unwrap();
        let columns: Vec<String> = row.iter().map(|r| r.get::<String, _>("name")).collect();
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"context_id".to_string()));
        assert!(columns.contains(&"state".to_string()));
        assert!(columns.contains(&"data".to_string()));
        assert!(columns.contains(&"updated_at".to_string()));
        assert!(columns.contains(&"created_at".to_string()));
    }

    #[tokio::test]
    async fn run_pending_is_idempotent() {
        let pool = memory_pool().await;
        let runner = MigrationRunner::new(pool);

        let first = runner.run_pending().await.unwrap();
        assert_eq!(first, vec![1, 2, 3]);

        let second = runner.run_pending().await.unwrap();
        assert!(second.is_empty());

        assert_eq!(runner.current_version().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn pending_migrations_returns_unapplied() {
        let pool = memory_pool().await;
        let runner = MigrationRunner::new(pool);

        let pending = runner.pending_migrations().await.unwrap();
        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].version, 1);
        assert_eq!(pending[1].version, 2);
        assert_eq!(pending[2].version, 3);

        runner.run_pending().await.unwrap();

        let pending = runner.pending_migrations().await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn partial_application_tracks_correctly() {
        // Apply only V1 using a custom migration set, then switch to full set.
        let pool = memory_pool().await;

        let v1_only: &[Migration] = &BUILTIN_MIGRATIONS[..1];
        // Safety: we need a 'static reference for the runner. In tests this is
        // fine because the slice is already 'static (subset of BUILTIN_MIGRATIONS).
        let runner = MigrationRunner::with_migrations(pool.clone(), v1_only);
        let applied = runner.run_pending().await.unwrap();
        assert_eq!(applied, vec![1]);
        assert_eq!(runner.current_version().await.unwrap(), 1);

        // Now create a runner with all migrations — only V2 and V3 should be pending.
        let full_runner = MigrationRunner::new(pool);
        let pending = full_runner.pending_migrations().await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].version, 2);
        assert_eq!(pending[1].version, 3);

        let applied = full_runner.run_pending().await.unwrap();
        assert_eq!(applied, vec![2, 3]);
        assert_eq!(full_runner.current_version().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn schema_versions_table_records_metadata() {
        let pool = memory_pool().await;
        let runner = MigrationRunner::new(pool.clone());
        runner.run_pending().await.unwrap();

        let rows = sqlx::query(
            "SELECT version, description, applied_at FROM schema_versions ORDER BY version",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<i32, _>("version"), 1);
        assert!(!rows[0].get::<String, _>("description").is_empty());
        assert!(!rows[0].get::<String, _>("applied_at").is_empty());
    }

    #[tokio::test]
    async fn composite_index_exists_after_v3() {
        let pool = memory_pool().await;
        let runner = MigrationRunner::new(pool.clone());
        runner.run_pending().await.unwrap();

        let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='index' AND name='idx_tasks_context_id_state'")
            .fetch_all(&pool)
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
    }
}
