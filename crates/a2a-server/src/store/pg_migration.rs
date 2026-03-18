// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Schema versioning and migration support for [`PostgresTaskStore`](super::PostgresTaskStore).
//!
//! This module provides a lightweight, forward-only migration runner that tracks
//! applied schema versions in a `schema_versions` table. Migrations are defined
//! as plain SQL strings and are executed inside transactions for atomicity.
//!
//! # Built-in migrations
//!
//! | Version | Description |
//! |---------|-------------|
//! | 1 | Initial schema — `tasks` table with indexes on `context_id` and `state` |
//! | 2 | Add composite index on `(context_id, state)` for combined filter queries |
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_server::store::pg_migration::PgMigrationRunner;
//! use sqlx::postgres::PgPoolOptions;
//!
//! # async fn example() -> Result<(), sqlx::Error> {
//! let pool = PgPoolOptions::new()
//!     .connect("postgres://user:pass@localhost/a2a")
//!     .await?;
//!
//! let runner = PgMigrationRunner::new(pool);
//! let applied = runner.run_pending().await?;
//! println!("Applied migrations: {applied:?}");
//! # Ok(())
//! # }
//! ```

use sqlx::postgres::PgPool;
use sqlx::Row;

/// A single schema migration.
#[derive(Debug, Clone)]
pub struct PgMigration {
    /// Unique version number. Must be greater than zero and monotonically
    /// increasing across the migration list.
    pub version: u32,
    /// Short human-readable description of the migration.
    pub description: &'static str,
    /// SQL statements to execute. Multiple statements can be separated by
    /// semicolons; they run inside a single transaction.
    pub sql: &'static str,
}

/// Built-in migrations for the `PostgresTaskStore` schema.
pub static BUILTIN_PG_MIGRATIONS: &[PgMigration] = &[
    PgMigration {
        version: 1,
        description: "Initial schema: tasks table with indexes",
        sql: "\
CREATE TABLE IF NOT EXISTS tasks (
    id         TEXT PRIMARY KEY,
    context_id TEXT NOT NULL,
    state      TEXT NOT NULL,
    data       JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);\
CREATE INDEX IF NOT EXISTS idx_tasks_context_id ON tasks(context_id);\
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state)",
    },
    PgMigration {
        version: 2,
        description: "Add composite index on (context_id, state) for combined filter queries",
        sql: "CREATE INDEX IF NOT EXISTS idx_tasks_context_id_state ON tasks(context_id, state)",
    },
];

/// Runs schema migrations against a `PostgreSQL` database.
///
/// Tracks which migrations have been applied in a `schema_versions` table and
/// only executes those that have not yet been applied. Migrations are executed
/// in version order inside transactions.
///
/// # Concurrency safety
///
/// Uses `LOCK TABLE schema_versions IN EXCLUSIVE MODE` within transactions to
/// prevent concurrent migration runners from applying the same migration twice.
#[derive(Debug, Clone)]
pub struct PgMigrationRunner {
    pool: PgPool,
    migrations: &'static [PgMigration],
}

impl PgMigrationRunner {
    /// Creates a new runner with the built-in migrations.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            migrations: BUILTIN_PG_MIGRATIONS,
        }
    }

    /// Creates a new runner with a custom set of migrations.
    #[must_use]
    pub const fn with_migrations(pool: PgPool, migrations: &'static [PgMigration]) -> Self {
        Self { pool, migrations }
    }

    /// Ensures the `schema_versions` tracking table exists.
    async fn ensure_version_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_versions (
                version     INTEGER PRIMARY KEY,
                description TEXT        NOT NULL,
                applied_at  TIMESTAMPTZ NOT NULL DEFAULT now()
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
    pub async fn pending_migrations(&self) -> Result<Vec<&PgMigration>, sqlx::Error> {
        let current = self.current_version().await?;
        Ok(self
            .migrations
            .iter()
            .filter(|m| m.version > current)
            .collect())
    }

    /// Applies all pending migrations in version order.
    ///
    /// Each migration runs inside its own transaction with an exclusive lock on
    /// the `schema_versions` table to prevent concurrent application. If a
    /// migration fails, the transaction is rolled back and the error is returned.
    ///
    /// Returns the list of version numbers that were applied.
    ///
    /// # Errors
    ///
    /// Returns an error if any migration fails to apply.
    pub async fn run_pending(&self) -> Result<Vec<u32>, sqlx::Error> {
        self.ensure_version_table().await?;

        let current = self.current_version().await?;
        let mut applied = Vec::new();

        for migration in self.migrations {
            if migration.version <= current {
                continue;
            }

            let mut tx = self.pool.begin().await?;

            // Lock the version table to prevent concurrent migration application.
            sqlx::query("LOCK TABLE schema_versions IN EXCLUSIVE MODE")
                .execute(&mut *tx)
                .await?;

            // Re-check the version inside the transaction (double-check locking).
            let row = sqlx::query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_versions")
                .fetch_one(&mut *tx)
                .await?;
            let current_in_tx: i32 = row.get("v");
            #[allow(clippy::cast_sign_loss)]
            if migration.version <= current_in_tx as u32 {
                // Already applied by another runner.
                tx.rollback().await?;
                continue;
            }

            for statement in migration.sql.split(';') {
                let trimmed = statement.trim();
                if trimmed.is_empty() {
                    continue;
                }
                sqlx::query(trimmed).execute(&mut *tx).await?;
            }

            #[allow(clippy::cast_possible_wrap)] // migration versions are small constants (<100)
            let version_i32 = migration.version as i32;
            sqlx::query("INSERT INTO schema_versions (version, description) VALUES ($1, $2)")
                .bind(version_i32)
                .bind(migration.description)
                .execute(&mut *tx)
                .await?;

            tx.commit().await?;
            applied.push(migration.version);
        }

        Ok(applied)
    }
}
