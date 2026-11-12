// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`StepJournal`] in PostgreSQL: runs every replica sharing the database can
//! resume, kept across restarts.

use std::sync::Arc;

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::postgres::PgPool;

use super::{JournalError, JournalFuture, RunCheckpoint, StepJournal, from_text, to_text};
use crate::metrics::{Metrics, NoopMetrics};
use crate::store::pg_retry;

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_run_journal (
    run_key    TEXT PRIMARY KEY,
    revision   BIGINT NOT NULL,
    body       TEXT NOT NULL,
    updated_at BIGINT NOT NULL
)";

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
        crate::store::pg_schema::apply(&pool, &[CREATE_TABLE_SQL])
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

impl StepJournal for PostgresStepJournal {
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<RunCheckpoint>> {
        Box::pin(async move {
            let row: Option<(String,)> = pg_retry::retry(self.metrics.as_ref(), || {
                sqlx::query_as("SELECT body FROM a2a_run_journal WHERE run_key = $1")
                    .bind(key)
                    .fetch_optional(&self.pool)
            })
            .await
            .map_err(|e| failed("load", &e))?;
            row.map(|(body,)| from_text(&body)).transpose()
        })
    }

    fn save<'a>(&'a self, key: &'a str, checkpoint: &'a RunCheckpoint) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let body = to_text(checkpoint)?;
            let revision = i64::try_from(checkpoint.revision)
                .map_err(|_| JournalError::Store("a revision past i64".to_owned()))?;
            let now = i64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis()),
            )
            .unwrap_or(i64::MAX);
            let written = if revision == 1 {
                pg_retry::checked(
                    sqlx::query(
                        "INSERT INTO a2a_run_journal (run_key, revision, body, updated_at)
                         VALUES ($1, 1, $2, $3) ON CONFLICT (run_key) DO NOTHING",
                    )
                    .bind(key)
                    .bind(&body)
                    .bind(now)
                    .execute(&self.pool),
                )
                .await
            } else {
                pg_retry::checked(
                    sqlx::query(
                        "UPDATE a2a_run_journal SET revision = $2, body = $3, updated_at = $4
                         WHERE run_key = $1 AND revision = $5",
                    )
                    .bind(key)
                    .bind(revision)
                    .bind(&body)
                    .bind(now)
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

    fn clear<'a>(&'a self, key: &'a str) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            // Checked, not retried: a second delete could remove the checkpoint
            // of a run another replica began under this key in between.
            pg_retry::checked(
                sqlx::query("DELETE FROM a2a_run_journal WHERE run_key = $1")
                    .bind(key)
                    .execute(&self.pool),
            )
            .await
            .map_err(|e| failed("clear", &e))?;
            Ok(())
        })
    }
}
