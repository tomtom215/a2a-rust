// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! `PostgreSQL`-backed [`TaskStore`] implementation.
//!
//! Requires the `postgres` feature flag. Uses `sqlx` for async `PostgreSQL` access.
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_server::store::PostgresTaskStore;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = PostgresTaskStore::new("postgres://user:pass@localhost/a2a").await?;
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};
use sqlx::postgres::{PgPool, PgPoolOptions};

use super::task_store::TaskStore;

/// `PostgreSQL`-backed [`TaskStore`].
///
/// Stores tasks as JSONB blobs in a `tasks` table. Suitable for multi-node
/// production deployments that need shared persistence and horizontal scaling.
///
/// # Schema
///
/// The store auto-creates the following table on first use:
///
/// ```sql
/// CREATE TABLE IF NOT EXISTS tasks (
///     id         TEXT PRIMARY KEY,
///     context_id TEXT NOT NULL,
///     state      TEXT NOT NULL,
///     data       JSONB NOT NULL,
///     created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
///     updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
/// );
/// ```
#[derive(Debug, Clone)]
pub struct PostgresTaskStore {
    pool: PgPool,
}

impl PostgresTaskStore {
    /// Opens a `PostgreSQL` connection pool and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or the schema migration fails.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let pool = pg_pool(url).await?;
        Self::from_pool(pool).await
    }

    /// Opens a `PostgreSQL` database with automatic schema migration.
    ///
    /// Runs all pending migrations before returning the store. This is the
    /// recommended constructor for production deployments because it ensures
    /// the schema is always up to date without duplicating DDL statements.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or any migration fails.
    pub async fn with_migrations(url: &str) -> Result<Self, sqlx::Error> {
        let pool = pg_pool(url).await?;

        let runner = super::pg_migration::PgMigrationRunner::new(pool.clone());
        runner.run_pending().await?;

        Ok(Self { pool })
    }

    /// Creates a store from an existing connection pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema migration fails.
    pub async fn from_pool(pool: PgPool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tasks (
                id         TEXT PRIMARY KEY,
                context_id TEXT NOT NULL,
                state      TEXT NOT NULL,
                data       JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_context_id ON tasks(context_id)")
            .execute(&pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state)")
            .execute(&pool)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tasks_context_id_state ON tasks(context_id, state)",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

/// Creates a `PgPool` with production-ready defaults.
async fn pg_pool(url: &str) -> Result<PgPool, sqlx::Error> {
    pg_pool_with_size(url, 10).await
}

/// Creates a `PgPool` with a specific max connection count.
async fn pg_pool_with_size(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(url)
        .await
}

/// Converts a `sqlx::Error` to an `A2aError`.
#[allow(clippy::needless_pass_by_value)]
fn to_a2a_error(e: sqlx::Error) -> A2aError {
    A2aError::internal(format!("postgres error: {e}"))
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for PostgresTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let id = task.id.0.as_str();
            let context_id = task.context_id.0.as_str();
            let state = task.status.state.to_string();
            let data = serde_json::to_value(&task)
                .map_err(|e| A2aError::internal(format!("failed to serialize task: {e}")))?;

            sqlx::query(
                "INSERT INTO tasks (id, context_id, state, data, updated_at)
                 VALUES ($1, $2, $3, $4, now())
                 ON CONFLICT(id) DO UPDATE SET
                     context_id = EXCLUDED.context_id,
                     state = EXCLUDED.state,
                     data = EXCLUDED.data,
                     updated_at = now()",
            )
            .bind(id)
            .bind(context_id)
            .bind(&state)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(to_a2a_error)?;

            Ok(())
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        Box::pin(async move {
            let row: Option<(serde_json::Value,)> =
                sqlx::query_as("SELECT data FROM tasks WHERE id = $1")
                    .bind(id.0.as_str())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(to_a2a_error)?;

            match row {
                Some((data,)) => {
                    let task: Task = serde_json::from_value(data).map_err(|e| {
                        A2aError::internal(format!("failed to deserialize task: {e}"))
                    })?;
                    Ok(Some(task))
                }
                None => Ok(None),
            }
        })
    }

    fn list<'a>(
        &'a self,
        params: &'a ListTasksParams,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskListResponse>> + Send + 'a>> {
        Box::pin(async move {
            // Build dynamic query with optional filters.
            let mut conditions = Vec::new();
            let mut bind_values: Vec<String> = Vec::new();

            if let Some(ref ctx) = params.context_id {
                bind_values.push(ctx.clone());
                conditions.push(format!("context_id = ${}", bind_values.len()));
            }
            if let Some(ref status) = params.status {
                bind_values.push(status.to_string());
                conditions.push(format!("state = ${}", bind_values.len()));
            }
            if let Some(ref token) = params.page_token {
                bind_values.push(token.clone());
                conditions.push(format!("id > ${}", bind_values.len()));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let page_size = match params.page_size {
                Some(0) | None => 50_u32,
                Some(n) => n.min(1000),
            };

            // Fetch one extra to detect next page.
            let limit = page_size + 1;
            let sql =
                format!("SELECT data FROM tasks {where_clause} ORDER BY id ASC LIMIT {limit}");

            let mut query = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
            for val in &bind_values {
                query = query.bind(val);
            }

            let rows: Vec<(serde_json::Value,)> =
                query.fetch_all(&self.pool).await.map_err(to_a2a_error)?;

            let mut tasks: Vec<Task> = rows
                .into_iter()
                .map(|(data,)| {
                    serde_json::from_value(data)
                        .map_err(|e| A2aError::internal(format!("deserialize: {e}")))
                })
                .collect::<A2aResult<Vec<_>>>()?;

            let next_page_token = if tasks.len() > page_size as usize {
                tasks.truncate(page_size as usize);
                tasks.last().map(|t| t.id.0.clone())
            } else {
                None
            };

            let mut response = TaskListResponse::new(tasks);
            response.next_page_token = next_page_token;
            Ok(response)
        })
    }

    fn insert_if_absent<'a>(
        &'a self,
        task: Task,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async move {
            let id = task.id.0.as_str();
            let context_id = task.context_id.0.as_str();
            let state = task.status.state.to_string();
            let data = serde_json::to_value(&task)
                .map_err(|e| A2aError::internal(format!("failed to serialize task: {e}")))?;

            let result = sqlx::query(
                "INSERT INTO tasks (id, context_id, state, data, updated_at)
                 VALUES ($1, $2, $3, $4, now())
                 ON CONFLICT(id) DO NOTHING",
            )
            .bind(id)
            .bind(context_id)
            .bind(&state)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(to_a2a_error)?;

            Ok(result.rows_affected() > 0)
        })
    }

    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::query("DELETE FROM tasks WHERE id = $1")
                .bind(id.0.as_str())
                .execute(&self.pool)
                .await
                .map_err(to_a2a_error)?;
            Ok(())
        })
    }

    fn count<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
        Box::pin(async move {
            let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks")
                .fetch_one(&self.pool)
                .await
                .map_err(to_a2a_error)?;
            #[allow(clippy::cast_sign_loss)]
            Ok(row.0 as u64)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_a2a_error_formats_message() {
        let pg_err = sqlx::Error::RowNotFound;
        let a2a_err = to_a2a_error(pg_err);
        let msg = format!("{a2a_err}");
        assert!(
            msg.contains("postgres error"),
            "error message should contain 'postgres error': {msg}"
        );
    }
}
