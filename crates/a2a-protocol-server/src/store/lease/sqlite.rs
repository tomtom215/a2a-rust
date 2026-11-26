// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`LeaseStore`] in SQLite: leases every process that opens the same
//! database file renews and reads (COR-09).
//!
//! # Whose clock
//!
//! The callers'. Every process that opens one SQLite file runs on the host
//! that holds it, so the `deadline_ms` and `now_ms` they pass are read from
//! one clock, the host's, and this store keeps them as given: a grant's
//! deadline is the caller's, a renewal is the caller's `now_ms` plus the
//! lease's time to live, and [`lapsed`](LeaseStore::lapsed) is the trait's
//! default, which compares the deadline with this process's clock. A file
//! shared over a network filesystem by several hosts would break that, and
//! SQLite's locking over one is not to be relied on either; use
//! `PostgresLeaseStore` for replicas on several hosts.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::sqlite::SqlitePool;

use super::LeaseStore;

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_leases (
    tenant       TEXT NOT NULL,
    task_id      TEXT NOT NULL,
    ttl_ms       INTEGER NOT NULL,
    deadline_ms  INTEGER NOT NULL,
    PRIMARY KEY (tenant, task_id)
)";

const GRANT_SQL: &str = "INSERT INTO a2a_leases (tenant, task_id, ttl_ms, deadline_ms)
    VALUES (?1, ?2, ?3, ?4)
    ON CONFLICT (tenant, task_id) DO UPDATE
    SET ttl_ms = excluded.ttl_ms, deadline_ms = excluded.deadline_ms";

/// `now_ms + ttl_ms`, saturating at `i64::MAX` as the in-memory store's
/// `saturating_add` does: SQLite turns an integer sum that overflows into a
/// floating-point one, which would read back as an error.
const RENEW_SQL: &str = "UPDATE a2a_leases SET deadline_ms = CASE
        WHEN ttl_ms > 0 AND ?3 > 9223372036854775807 - ttl_ms THEN 9223372036854775807
        WHEN ttl_ms < 0 AND ?3 < -9223372036854775808 - ttl_ms THEN -9223372036854775808
        ELSE ?3 + ttl_ms
    END
    WHERE tenant = ?1 AND task_id = ?2";

/// Lapsed is `now_ms > deadline_ms`, as `deadline_lapsed` says.
const LAPSED_LEASES_SQL: &str = "SELECT tenant, task_id FROM a2a_leases
    WHERE deadline_ms < ?1 ORDER BY deadline_ms DESC, tenant, task_id LIMIT ?2";

/// Leases in a SQLite table, `a2a_leases`, created if absent.
#[derive(Debug, Clone)]
pub struct SqliteLeaseStore {
    pub(super) pool: SqlitePool,
}

impl SqliteLeaseStore {
    /// Opens (or creates) the database at `url`, as the SQLite task store
    /// does, and creates the table.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the database cannot be opened or the table
    /// cannot be created.
    pub async fn new(url: &str) -> A2aResult<Self> {
        let pool = crate::sqlite_pool::sqlite_pool(url)
            .await
            .map_err(|e| A2aError::internal(format!("lease store open: {e}")))?;
        Self::from_pool(pool).await
    }

    /// Uses `pool`, usually the task store's, and creates the table.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the table cannot be created.
    pub async fn from_pool(pool: SqlitePool) -> A2aResult<Self> {
        sqlx::query(CREATE_TABLE_SQL)
            .execute(&pool)
            .await
            .map_err(|e| A2aError::internal(format!("lease store migrate: {e}")))?;
        Ok(Self { pool })
    }
}

fn failed(what: &str, e: &sqlx::Error) -> A2aError {
    A2aError::internal(format!("lease store {what}: {e}"))
}

impl LeaseStore for SqliteLeaseStore {
    fn is_process_local(&self) -> bool {
        crate::sqlite_pool::is_in_memory(&self.pool)
    }

    fn grant<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
        ttl_ms: i64,
        deadline_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::query(GRANT_SQL)
                .bind(tenant)
                .bind(task)
                .bind(ttl_ms)
                .bind(deadline_ms)
                .execute(&self.pool)
                .await
                .map_err(|e| failed("grant", &e))?;
            Ok(())
        })
    }

    fn renew<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
        now_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<bool>> + Send + 'a>> {
        Box::pin(async move {
            let done = sqlx::query(RENEW_SQL)
                .bind(tenant)
                .bind(task)
                .bind(now_ms)
                .execute(&self.pool)
                .await
                .map_err(|e| failed("renew", &e))?;
            Ok(done.rows_affected() > 0)
        })
    }

    fn deadline<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<Option<i64>>> + Send + 'a>> {
        Box::pin(async move {
            let row: Option<(i64,)> = sqlx::query_as(
                "SELECT deadline_ms FROM a2a_leases WHERE tenant = ?1 AND task_id = ?2",
            )
            .bind(tenant)
            .bind(task)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| failed("read", &e))?;
            Ok(row.map(|(deadline,)| deadline))
        })
    }

    fn release<'a>(
        &'a self,
        tenant: &'a str,
        task: &'a str,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        Box::pin(async move {
            sqlx::query("DELETE FROM a2a_leases WHERE tenant = ?1 AND task_id = ?2")
                .bind(tenant)
                .bind(task)
                .execute(&self.pool)
                .await
                .map_err(|e| failed("release", &e))?;
            Ok(())
        })
    }

    fn sweep(
        &self,
        now_ms: i64,
        lapsed_for_ms: i64,
    ) -> Pin<Box<dyn Future<Output = A2aResult<u64>> + Send + '_>> {
        let cutoff = now_ms.saturating_sub(lapsed_for_ms);
        Box::pin(async move {
            let done = sqlx::query("DELETE FROM a2a_leases WHERE deadline_ms < ?1")
                .bind(cutoff)
                .execute(&self.pool)
                .await
                .map_err(|e| failed("sweep", &e))?;
            Ok(done.rows_affected())
        })
    }

    fn lapsed_leases(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = A2aResult<super::LeaseKeys>> + Send + '_>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        Box::pin(async move {
            sqlx::query_as(LAPSED_LEASES_SQL)
                .bind(now_ms)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| failed("read", &e))
        })
    }
}
