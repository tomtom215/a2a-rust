// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`StepJournal`] in PostgreSQL: runs every replica sharing the database can
//! resume, kept across restarts.

use std::sync::Arc;

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::Row;
use sqlx::postgres::{PgPool, PgRow};

use super::{JournalEntry, JournalError, JournalFuture, RunOwner, RunSummary, StepJournal};
use crate::metrics::{Metrics, NoopMetrics};
use crate::store::pg_retry;

/// The table, and what 2026-11-13 added to it (N07): who the run is for and
/// whether it is finished, added to a table an earlier build created too.
const SCHEMA: [&str; 7] = [
    "CREATE TABLE IF NOT EXISTS a2a_run_journal (
        run_key    TEXT PRIMARY KEY,
        revision   BIGINT NOT NULL,
        body       TEXT NOT NULL,
        updated_at BIGINT NOT NULL
    )",
    "ALTER TABLE a2a_run_journal ADD COLUMN IF NOT EXISTS tenant TEXT",
    "ALTER TABLE a2a_run_journal ADD COLUMN IF NOT EXISTS principal TEXT",
    "ALTER TABLE a2a_run_journal ADD COLUMN IF NOT EXISTS task TEXT",
    "ALTER TABLE a2a_run_journal ADD COLUMN IF NOT EXISTS completed BOOLEAN NOT NULL DEFAULT FALSE",
    "CREATE INDEX IF NOT EXISTS idx_a2a_run_journal_principal ON a2a_run_journal(principal, tenant)",
    "CREATE INDEX IF NOT EXISTS idx_a2a_run_journal_updated ON a2a_run_journal(completed, updated_at)",
];

const COLUMNS: &str = "run_key, revision, body, updated_at, tenant, principal, task, completed";

/// Checkpoints in a PostgreSQL table, `a2a_run_journal`, created if absent.
///
/// A read runs again once if its connection was lost
/// (`pg_retry::retry`). A write does not: a lost connection does not prove it
/// did not commit, and a second run of a compare-and-set on the revision would
/// then report a conflict for a checkpoint that was written, so a write is
/// run on a connection pinged first (`pg_retry::checked`) and its failure is
/// the caller's to see.
#[derive(Clone)]
pub struct PostgresStepJournal {
    pool: PgPool,
    metrics: Arc<dyn Metrics>,
}

impl std::fmt::Debug for PostgresStepJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresStepJournal")
            .finish_non_exhaustive()
    }
}

impl PostgresStepJournal {
    /// Uses `pool`, usually the task store's, and creates the table under the
    /// crate's schema lock, so replicas starting together do not race.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the table cannot be created.
    pub async fn from_pool(pool: PgPool) -> A2aResult<Self> {
        crate::store::pg_schema::apply(&pool, &SCHEMA)
            .await
            .map_err(|e| A2aError::internal(format!("run journal migrate: {e}")))?;
        Ok(Self {
            pool,
            metrics: Arc::new(NoopMetrics),
        })
    }

    /// Reports a read run again after a lost connection to `metrics`
    /// (`Metrics::on_store_retry`).
    #[must_use]
    pub fn with_metrics(mut self, metrics: Arc<dyn Metrics>) -> Self {
        self.metrics = metrics;
        self
    }
}

fn failed(what: &str, e: &sqlx::Error) -> JournalError {
    JournalError::Store(format!("{what}: {e}"))
}

fn revision_of(r: u64) -> Result<i64, JournalError> {
    i64::try_from(r).map_err(|_| JournalError::Store("a revision past i64".to_owned()))
}

/// The key and the entry a row holds.
fn entry_of(row: &PgRow) -> (String, JournalEntry) {
    let entry = JournalEntry::new(
        u64::try_from(row.get::<i64, _>(1)).unwrap_or(0),
        RunOwner::new(row.get(4), row.get(5), row.get(6)),
        row.get(7),
        row.get(3),
        row.get(2),
    );
    (row.get(0), entry)
}

impl StepJournal for PostgresStepJournal {
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<JournalEntry>> {
        Box::pin(async move {
            let sql = format!("SELECT {COLUMNS} FROM a2a_run_journal WHERE run_key = $1");
            let row = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(key)
                    .fetch_optional(&self.pool)
            })
            .await
            .map_err(|e| failed("load", &e))?;
            Ok(row.map(|r| entry_of(&r).1))
        })
    }

    fn save<'a>(&'a self, key: &'a str, entry: &'a JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let revision = revision_of(entry.revision)?;
            let owner = &entry.owner;
            let written = if revision == 1 {
                pg_retry::checked(
                    sqlx::query(
                        "INSERT INTO a2a_run_journal
                            (run_key, revision, body, updated_at, tenant, principal, task, completed)
                         VALUES ($1, 1, $2, $3, $4, $5, $6, $7) ON CONFLICT (run_key) DO NOTHING",
                    )
                    .bind(key)
                    .bind(&entry.body)
                    .bind(entry.updated_at_ms)
                    .bind(&owner.tenant)
                    .bind(&owner.principal)
                    .bind(&owner.task)
                    .bind(entry.completed)
                    .execute(&self.pool),
                )
                .await
            } else {
                pg_retry::checked(
                    sqlx::query(
                        "UPDATE a2a_run_journal SET revision = $2, body = $3, updated_at = $4,
                            tenant = $5, principal = $6, task = $7, completed = $8
                         WHERE run_key = $1 AND revision = $9",
                    )
                    .bind(key)
                    .bind(revision)
                    .bind(&entry.body)
                    .bind(entry.updated_at_ms)
                    .bind(&owner.tenant)
                    .bind(&owner.principal)
                    .bind(&owner.task)
                    .bind(entry.completed)
                    .bind(revision - 1)
                    .execute(&self.pool),
                )
                .await
            }
            .map_err(|e| failed("save", &e))?;
            if written.rows_affected() == 1 {
                Ok(())
            } else {
                Err(JournalError::Conflict)
            }
        })
    }

    fn clear<'a>(&'a self, key: &'a str, revision: u64) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let revision = revision_of(revision)?;
            // Checked, not retried; and only at the revision given, so a stale
            // run's delete cannot remove a newer run's checkpoint (N04).
            let done = pg_retry::checked(
                sqlx::query("DELETE FROM a2a_run_journal WHERE run_key = $1 AND revision = $2")
                    .bind(key)
                    .bind(revision)
                    .execute(&self.pool),
            )
            .await
            .map_err(|e| failed("clear", &e))?;
            if done.rows_affected() == 1 {
                return Ok(());
            }
            let other: Option<i64> = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query_scalar("SELECT revision FROM a2a_run_journal WHERE run_key = $1")
                    .bind(key)
                    .fetch_optional(&self.pool)
            })
            .await
            .map_err(|e| failed("clear", &e))?;
            other.map_or(Ok(()), |_| Err(JournalError::Conflict))
        })
    }

    fn sweep(&self, completed_before_ms: i64, unfinished_before_ms: i64) -> JournalFuture<'_, u64> {
        Box::pin(async move {
            // Retried: a second run of a cutoff delete deletes nothing more.
            let done = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(
                    "DELETE FROM a2a_run_journal
                     WHERE (completed AND updated_at < $1) OR (NOT completed AND updated_at < $2)",
                )
                .bind(completed_before_ms)
                .bind(unfinished_before_ms)
                .execute(&self.pool)
            })
            .await
            .map_err(|e| failed("sweep", &e))?;
            Ok(done.rows_affected())
        })
    }

    fn runs_of<'a>(
        &'a self,
        principal: &'a str,
        tenant: Option<&'a str>,
    ) -> JournalFuture<'a, Vec<RunSummary>> {
        Box::pin(async move {
            let sql = format!(
                "SELECT {COLUMNS} FROM a2a_run_journal
                 WHERE principal = $1 AND ($2::TEXT IS NULL OR tenant = $2) ORDER BY run_key"
            );
            let rows = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
                    .bind(principal)
                    .bind(tenant)
                    .fetch_all(&self.pool)
            })
            .await
            .map_err(|e| failed("runs_of", &e))?;
            Ok(rows
                .iter()
                .map(|r| {
                    let (key, entry) = entry_of(r);
                    entry.summary(&key)
                })
                .collect())
        })
    }

    fn erase<'a>(&'a self, principal: &'a str, tenant: Option<&'a str>) -> JournalFuture<'a, u64> {
        Box::pin(async move {
            // Retried: a second run of the delete deletes nothing more.
            let done = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query(
                    "DELETE FROM a2a_run_journal
                     WHERE principal = $1 AND ($2::TEXT IS NULL OR tenant = $2)",
                )
                .bind(principal)
                .bind(tenant)
                .execute(&self.pool)
            })
            .await
            .map_err(|e| failed("erase", &e))?;
            Ok(done.rows_affected())
        })
    }
}
