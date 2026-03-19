// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tenant-scoped `PostgreSQL`-backed [`TaskStore`] implementation.
//!
//! Adds a `tenant_id` column to the `tasks` table for full tenant isolation
//! at the database level. Uses [`TenantContext`] to scope all operations.
//!
//! Requires the `postgres` feature flag.
//!
//! # Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS tenant_tasks (
//!     tenant_id  TEXT NOT NULL DEFAULT '',
//!     id         TEXT NOT NULL,
//!     context_id TEXT NOT NULL,
//!     state      TEXT NOT NULL,
//!     data       JSONB NOT NULL,
//!     created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
//!     PRIMARY KEY (tenant_id, id)
//! );
//! ```

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::responses::TaskListResponse;
use a2a_protocol_types::task::{Task, TaskId};
use sqlx::postgres::{PgPool, PgPoolOptions};

use super::task_store::TaskStore;
use super::tenant::TenantContext;

/// Tenant-scoped `PostgreSQL`-backed [`TaskStore`].
///
/// Each operation is scoped to the tenant from [`TenantContext`]. Tasks are
/// stored with a `tenant_id` column for database-level isolation, enabling
/// efficient per-tenant queries and deletion.
#[derive(Debug, Clone)]
pub struct TenantAwarePostgresTaskStore {
    pool: PgPool,
}

impl TenantAwarePostgresTaskStore {
    /// Opens a `PostgreSQL` connection pool and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or migration fails.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;
        Self::from_pool(pool).await
    }

    /// Creates a store from an existing connection pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema migration fails.
    pub async fn from_pool(pool: PgPool) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tenant_tasks (
                tenant_id  TEXT NOT NULL DEFAULT '',
                id         TEXT NOT NULL,
                context_id TEXT NOT NULL,
                state      TEXT NOT NULL,
                data       JSONB NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                PRIMARY KEY (tenant_id, id)
            )",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tenant_tasks_ctx ON tenant_tasks(tenant_id, context_id)",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_tenant_tasks_state ON tenant_tasks(tenant_id, state)",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

fn to_a2a_error(e: &sqlx::Error) -> A2aError {
    A2aError::internal(format!("postgres error: {e}"))
}

#[allow(clippy::manual_async_fn)]
impl TaskStore for TenantAwarePostgresTaskStore {
    fn save<'a>(&'a self, task: Task) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let id = task.id.0.as_str();
            let context_id = task.context_id.0.as_str();
            let state = task.status.state.to_string();
            let data = serde_json::to_value(&task)
                .map_err(|e| A2aError::internal(format!("failed to serialize task: {e}")))?;

            sqlx::query(
                "INSERT INTO tenant_tasks (tenant_id, id, context_id, state, data, updated_at)
                 VALUES ($1, $2, $3, $4, $5, now())
                 ON CONFLICT(tenant_id, id) DO UPDATE SET
                     context_id = EXCLUDED.context_id,
                     state = EXCLUDED.state,
                     data = EXCLUDED.data,
                     updated_at = now()",
            )
            .bind(&tenant)
            .bind(id)
            .bind(context_id)
            .bind(&state)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            Ok(())
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<Task>>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let row: Option<(serde_json::Value,)> =
                sqlx::query_as("SELECT data FROM tenant_tasks WHERE tenant_id = $1 AND id = $2")
                    .bind(&tenant)
                    .bind(id.0.as_str())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| to_a2a_error(&e))?;

            match row {
                Some((data,)) => {
                    let task: Task = serde_json::from_value(data)
                        .map_err(|e| A2aError::internal(format!("deserialize: {e}")))?;
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
            let tenant = TenantContext::current();
            let mut conditions = vec!["tenant_id = $1".to_string()];
            let mut bind_values: Vec<String> = vec![tenant];

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

            let where_clause = format!("WHERE {}", conditions.join(" AND "));

            let page_size = match params.page_size {
                Some(0) | None => 50_u32,
                Some(n) => n.min(1000),
            };

            let limit = page_size + 1;
            let sql = format!(
                "SELECT data FROM tenant_tasks {where_clause} ORDER BY id ASC LIMIT {limit}"
            );

            let mut query = sqlx::query_as::<_, (serde_json::Value,)>(&sql);
            for val in &bind_values {
                query = query.bind(val);
            }

            let rows: Vec<(serde_json::Value,)> = query
                .fetch_all(&self.pool)
                .await
                .map_err(|e| to_a2a_error(&e))?;

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
            let tenant = TenantContext::current();
            let id = task.id.0.as_str();
            let context_id = task.context_id.0.as_str();
            let state = task.status.state.to_string();
            let data = serde_json::to_value(&task)
                .map_err(|e| A2aError::internal(format!("serialize: {e}")))?;

            let result = sqlx::query(
                "INSERT INTO tenant_tasks (tenant_id, id, context_id, state, data, updated_at)
                 VALUES ($1, $2, $3, $4, $5, now())
                 ON CONFLICT(tenant_id, id) DO NOTHING",
            )
            .bind(&tenant)
            .bind(id)
            .bind(context_id)
            .bind(&state)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            Ok(result.rows_affected() > 0)
        })
    }

    fn delete<'a>(
        &'a self,
        id: &'a TaskId,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            sqlx::query("DELETE FROM tenant_tasks WHERE tenant_id = $1 AND id = $2")
                .bind(&tenant)
                .bind(id.0.as_str())
                .execute(&self.pool)
                .await
                .map_err(|e| to_a2a_error(&e))?;
            Ok(())
        })
    }

    fn count<'a>(&'a self) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let row: (i64,) =
                sqlx::query_as("SELECT COUNT(*) FROM tenant_tasks WHERE tenant_id = $1")
                    .bind(&tenant)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| to_a2a_error(&e))?;
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
        let a2a_err = to_a2a_error(&pg_err);
        let msg = format!("{a2a_err}");
        assert!(
            msg.contains("postgres error"),
            "error message should contain 'postgres error': {msg}"
        );
    }
}
