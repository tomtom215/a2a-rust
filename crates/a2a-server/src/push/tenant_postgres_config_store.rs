// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tenant-scoped `PostgreSQL`-backed [`PushConfigStore`] implementation.
//!
//! Adds a `tenant_id` column to the `push_configs` table for full tenant
//! isolation. Uses [`TenantContext`] to scope all operations.
//!
//! Requires the `postgres` feature flag.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use a2a_protocol_types::push::TaskPushNotificationConfig;
use sqlx::postgres::PgPool;

use super::config_store::PushConfigStore;
use crate::store::tenant::TenantContext;

/// Tenant-scoped `PostgreSQL`-backed [`PushConfigStore`].
///
/// Each operation is scoped to the tenant from [`TenantContext`].
///
/// # Schema
///
/// ```sql
/// CREATE TABLE IF NOT EXISTS tenant_push_configs (
///     tenant_id TEXT NOT NULL DEFAULT '',
///     task_id   TEXT NOT NULL,
///     id        TEXT NOT NULL,
///     data      JSONB NOT NULL,
///     PRIMARY KEY (tenant_id, task_id, id)
/// );
/// ```
#[derive(Debug, Clone)]
pub struct TenantAwarePostgresPushConfigStore {
    pool: PgPool,
}

fn to_a2a_error(e: &sqlx::Error) -> A2aError {
    A2aError::internal(format!("postgres error: {e}"))
}

impl TenantAwarePostgresPushConfigStore {
    /// Opens a `PostgreSQL` connection pool and initializes the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the database cannot be opened or migration fails.
    pub async fn new(url: &str) -> Result<Self, sqlx::Error> {
        let pool = sqlx::postgres::PgPoolOptions::new()
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
            "CREATE TABLE IF NOT EXISTS tenant_push_configs (
                tenant_id TEXT NOT NULL DEFAULT '',
                task_id   TEXT NOT NULL,
                id        TEXT NOT NULL,
                data      JSONB NOT NULL,
                PRIMARY KEY (tenant_id, task_id, id)
            )",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }
}

#[allow(clippy::manual_async_fn)]
impl PushConfigStore for TenantAwarePostgresPushConfigStore {
    fn set<'a>(
        &'a self,
        mut config: TaskPushNotificationConfig,
    ) -> Pin<Box<dyn Future<Output = A2aResult<TaskPushNotificationConfig>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let id = config
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            config.id = Some(id.clone());

            let data = serde_json::to_value(&config)
                .map_err(|e| A2aError::internal(format!("serialize: {e}")))?;

            sqlx::query(
                "INSERT INTO tenant_push_configs (tenant_id, task_id, id, data)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT(tenant_id, task_id, id) DO UPDATE SET data = EXCLUDED.data",
            )
            .bind(&tenant)
            .bind(&config.task_id)
            .bind(&id)
            .bind(&data)
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            Ok(config)
        })
    }

    fn get<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<TaskPushNotificationConfig>>> + Send + 'a>>
    {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let row: Option<(serde_json::Value,)> = sqlx::query_as(
                "SELECT data FROM tenant_push_configs WHERE tenant_id = $1 AND task_id = $2 AND id = $3",
            )
            .bind(&tenant)
            .bind(task_id)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            match row {
                Some((data,)) => {
                    let config: TaskPushNotificationConfig = serde_json::from_value(data)
                        .map_err(|e| A2aError::internal(format!("deserialize: {e}")))?;
                    Ok(Some(config))
                }
                None => Ok(None),
            }
        })
    }

    fn list<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Vec<TaskPushNotificationConfig>>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            let rows: Vec<(serde_json::Value,)> = sqlx::query_as(
                "SELECT data FROM tenant_push_configs WHERE tenant_id = $1 AND task_id = $2",
            )
            .bind(&tenant)
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;

            rows.into_iter()
                .map(|(data,)| {
                    serde_json::from_value(data)
                        .map_err(|e| A2aError::internal(format!("deserialize: {e}")))
                })
                .collect()
        })
    }

    fn delete<'a>(
        &'a self,
        task_id: &'a str,
        id: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let tenant = TenantContext::current();
            sqlx::query(
                "DELETE FROM tenant_push_configs WHERE tenant_id = $1 AND task_id = $2 AND id = $3",
            )
            .bind(&tenant)
            .bind(task_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| to_a2a_error(&e))?;
            Ok(())
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
