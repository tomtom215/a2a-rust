// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! SQLite-backed [`TaskStore`] implementation.
//!
//! Requires the `sqlite` feature flag. Uses `sqlx` for async `SQLite` access.
//!
//! # Example
//!
//! ```rust,no_run
//! use a2a_protocol_server::store::SqliteTaskStore;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = SqliteTaskStore::new("sqlite:tasks.db").await?;
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

use super::task_store::TaskStore;

/// SQLite-backed [`TaskStore`].
///
/// Stores tasks as JSON blobs in a `tasks` table. Suitable for single-node
/// production deployments that need persistence across restarts.
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
///     data       TEXT NOT NULL,
///     updated_at TEXT NOT NULL DEFAULT (datetime('now'))
/// );
/// ```
#[derive(Debug, Clone)]
pub struct SqliteTaskStore {
    pool: SqlitePool,
}

impl SqliteTaskStore {
    /// Opens (or creates) a `SQLite` database and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or the schema migration fails.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect(url)
            .await?;
        Self::from_pool(pool).await
    }

    /// Creates a store from an existing connection pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema migration fails.
    pub async fn from_pool(pool: SqlitePool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tasks (
                id         TEXT PRIMARY KEY,
                context_id TEXT NOT NULL,
                state      TEXT NOT NULL,
                data       TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
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

        Ok(Self { pool })
    }
}

/// Converts a `sqlx::Error` to an `A2aError`.
#[allow(clippy::needless_pass_by_value)]
fn to_a2a_error(e: sqlx::Error) -> A2aError {
    A2aError::internal(format!("sqlite error: {e}"))
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for SqliteTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let id = task.id.0.as_str();
            let context_id = task.context_id.0.as_str();
            let state = task.status.state.to_string();
            let data = serde_json::to_string(&task)
                .map_err(|e| A2aError::internal(format!("failed to serialize task: {e}")))?;

            sqlx::query(
                "INSERT INTO tasks (id, context_id, state, data, updated_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                     context_id = excluded.context_id,
                     state = excluded.state,
                     data = excluded.data,
                     updated_at = datetime('now')",
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
            let row: Option<(String,)> = sqlx::query_as("SELECT data FROM tasks WHERE id = ?1")
                .bind(id.0.as_str())
                .fetch_optional(&self.pool)
                .await
                .map_err(to_a2a_error)?;

            match row {
                Some((data,)) => {
                    let task: Task = serde_json::from_str(&data).map_err(|e| {
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
                conditions.push(format!("context_id = ?{}", bind_values.len() + 1));
                bind_values.push(ctx.clone());
            }
            if let Some(ref status) = params.status {
                conditions.push(format!("state = ?{}", bind_values.len() + 1));
                bind_values.push(status.to_string());
            }
            if let Some(ref token) = params.page_token {
                conditions.push(format!("id > ?{}", bind_values.len() + 1));
                bind_values.push(token.clone());
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

            let mut query = sqlx::query_as::<_, (String,)>(&sql);
            for val in &bind_values {
                query = query.bind(val);
            }

            let rows: Vec<(String,)> = query.fetch_all(&self.pool).await.map_err(to_a2a_error)?;

            let mut tasks: Vec<Task> = rows
                .into_iter()
                .map(|(data,)| {
                    serde_json::from_str(&data)
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
            let data = serde_json::to_string(&task)
                .map_err(|e| A2aError::internal(format!("failed to serialize task: {e}")))?;

            let result = sqlx::query(
                "INSERT OR IGNORE INTO tasks (id, context_id, state, data, updated_at)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))",
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
            sqlx::query("DELETE FROM tasks WHERE id = ?1")
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
