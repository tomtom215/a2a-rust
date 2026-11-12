// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`StepJournal`] in SQLite: runs kept across a restart of the process that
//! holds the database file.

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::sqlite::SqlitePool;

use super::{JournalError, JournalFuture, RunCheckpoint, StepJournal, from_text, to_text};

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_run_journal (
    run_key    TEXT PRIMARY KEY,
    revision   INTEGER NOT NULL,
    body       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
)";

/// Checkpoints in a SQLite table, `a2a_run_journal`, created if absent.
#[derive(Debug, Clone)]
pub struct SqliteStepJournal {
    pool: SqlitePool,
}

impl SqliteStepJournal {
    /// Opens (or creates) the database at `url`, as the SQLite task store does,
    /// and creates the table.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the database cannot be opened or the table
    /// cannot be created.
    pub async fn new(url: &str) -> A2aResult<Self> {
        let pool = crate::sqlite_pool::sqlite_pool(url)
            .await
            .map_err(|e| A2aError::internal(format!("run journal open: {e}")))?;
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
            .map_err(|e| A2aError::internal(format!("run journal migrate: {e}")))?;
        Ok(Self { pool })
    }
}

fn failed(what: &str, e: &sqlx::Error) -> JournalError {
    JournalError::Store(format!("{what}: {e}"))
}

impl StepJournal for SqliteStepJournal {
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<RunCheckpoint>> {
        Box::pin(async move {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT body FROM a2a_run_journal WHERE run_key = ?1")
                    .bind(key)
                    .fetch_optional(&self.pool)
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
            // The first revision inserts and loses to a row already there; a later
            // one updates only the row it follows. One statement each: atomic.
            let written = if revision == 1 {
                sqlx::query(
                    "INSERT INTO a2a_run_journal (run_key, revision, body, updated_at)
                     VALUES (?1, 1, ?2, ?3) ON CONFLICT (run_key) DO NOTHING",
                )
                .bind(key)
                .bind(&body)
                .bind(now)
                .execute(&self.pool)
                .await
            } else {
                sqlx::query(
                    "UPDATE a2a_run_journal SET revision = ?2, body = ?3, updated_at = ?4
                     WHERE run_key = ?1 AND revision = ?5",
                )
                .bind(key)
                .bind(revision)
                .bind(&body)
                .bind(now)
                .bind(revision - 1)
                .execute(&self.pool)
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
            sqlx::query("DELETE FROM a2a_run_journal WHERE run_key = ?1")
                .bind(key)
                .execute(&self.pool)
                .await
                .map_err(|e| failed("clear", &e))?;
            Ok(())
        })
    }
}
