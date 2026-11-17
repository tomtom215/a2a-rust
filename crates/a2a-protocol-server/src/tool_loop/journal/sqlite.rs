// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! [`StepJournal`] in SQLite: runs kept across a restart of the process that
//! holds the database file.

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::Row;
use sqlx::sqlite::{SqlitePool, SqliteRow};

use super::{JournalEntry, JournalError, JournalFuture, RunOwner, RunSummary, StepJournal};

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_run_journal (
    run_key    TEXT PRIMARY KEY,
    revision   INTEGER NOT NULL,
    body       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
)";

/// Columns added on 2026-11-13 (N07): who the run is for, and whether it is
/// finished. Added to a table an earlier build created, too.
const OWNER_COLUMNS: [(&str, &str); 4] = [
    ("tenant", "TEXT"),
    ("principal", "TEXT"),
    ("task", "TEXT"),
    ("completed", "INTEGER NOT NULL DEFAULT 0"),
];

const INDEX_SQL: [&str; 2] = [
    "CREATE INDEX IF NOT EXISTS idx_a2a_run_journal_principal ON a2a_run_journal(principal, tenant)",
    "CREATE INDEX IF NOT EXISTS idx_a2a_run_journal_updated ON a2a_run_journal(completed, updated_at)",
];

const COLUMNS: &str = "run_key, revision, body, updated_at, tenant, principal, task, completed";

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

    /// Uses `pool`, usually the task store's, and creates the table, adding
    /// the owner columns to one an earlier build made.
    ///
    /// # Errors
    ///
    /// [`A2aError::internal`] if the table cannot be created or altered.
    pub async fn from_pool(pool: SqlitePool) -> A2aResult<Self> {
        let migrate = |e: sqlx::Error| A2aError::internal(format!("run journal migrate: {e}"));
        sqlx::query(CREATE_TABLE_SQL)
            .execute(&pool)
            .await
            .map_err(migrate)?;
        for (column, kind) in OWNER_COLUMNS {
            let present: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM pragma_table_info('a2a_run_journal') WHERE name = ?1",
            )
            .bind(column)
            .fetch_one(&pool)
            .await
            .map_err(migrate)?;
            if present == 0 {
                // `column` and `kind` are constants of this file.
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "ALTER TABLE a2a_run_journal ADD COLUMN {column} {kind}"
                )))
                .execute(&pool)
                .await
                .map_err(migrate)?;
            }
        }
        for ddl in INDEX_SQL {
            sqlx::query(ddl).execute(&pool).await.map_err(migrate)?;
        }
        Ok(Self { pool })
    }
}

fn failed(what: &str, e: &sqlx::Error) -> JournalError {
    JournalError::Store(format!("{what}: {e}"))
}

fn revision_of(r: u64) -> Result<i64, JournalError> {
    i64::try_from(r).map_err(|_| JournalError::Store("a revision past i64".to_owned()))
}

/// The key and the entry a row holds.
fn entry_of(row: &SqliteRow) -> (String, JournalEntry) {
    let entry = JournalEntry::new(
        u64::try_from(row.get::<i64, _>(1)).unwrap_or(0),
        RunOwner::new(row.get(4), row.get(5), row.get(6)),
        row.get::<i64, _>(7) != 0,
        row.get(3),
        row.get(2),
    );
    (row.get(0), entry)
}

impl StepJournal for SqliteStepJournal {
    fn load<'a>(&'a self, key: &'a str) -> JournalFuture<'a, Option<JournalEntry>> {
        Box::pin(async move {
            let row = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {COLUMNS} FROM a2a_run_journal WHERE run_key = ?1"
            )))
            .bind(key)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| failed("load", &e))?;
            Ok(row.map(|r| entry_of(&r).1))
        })
    }

    fn save<'a>(&'a self, key: &'a str, entry: &'a JournalEntry) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let revision = revision_of(entry.revision)?;
            let owner = &entry.owner;
            // The first revision inserts and loses to a row already there; a later
            // one updates only the row it follows. One statement each: atomic.
            let written = if revision == 1 {
                sqlx::query(
                    "INSERT INTO a2a_run_journal
                        (run_key, revision, body, updated_at, tenant, principal, task, completed)
                     VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT (run_key) DO NOTHING",
                )
                .bind(key)
                .bind(&entry.body)
                .bind(entry.updated_at_ms)
                .bind(&owner.tenant)
                .bind(&owner.principal)
                .bind(&owner.task)
                .bind(i64::from(entry.completed))
                .execute(&self.pool)
                .await
            } else {
                sqlx::query(
                    "UPDATE a2a_run_journal SET revision = ?2, body = ?3, updated_at = ?4,
                        tenant = ?5, principal = ?6, task = ?7, completed = ?8
                     WHERE run_key = ?1 AND revision = ?9",
                )
                .bind(key)
                .bind(revision)
                .bind(&entry.body)
                .bind(entry.updated_at_ms)
                .bind(&owner.tenant)
                .bind(&owner.principal)
                .bind(&owner.task)
                .bind(i64::from(entry.completed))
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

    fn clear<'a>(&'a self, key: &'a str, revision: u64) -> JournalFuture<'a, ()> {
        Box::pin(async move {
            let revision = revision_of(revision)?;
            // One statement: the row goes only at the revision given. When
            // nothing went, a row at another revision is a conflict.
            let done =
                sqlx::query("DELETE FROM a2a_run_journal WHERE run_key = ?1 AND revision = ?2")
                    .bind(key)
                    .bind(revision)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| failed("clear", &e))?;
            if done.rows_affected() == 1 {
                return Ok(());
            }
            let other: Option<i64> =
                sqlx::query_scalar("SELECT revision FROM a2a_run_journal WHERE run_key = ?1")
                    .bind(key)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| failed("clear", &e))?;
            other.map_or(Ok(()), |_| Err(JournalError::Conflict))
        })
    }

    fn sweep(&self, completed_before_ms: i64, unfinished_before_ms: i64) -> JournalFuture<'_, u64> {
        Box::pin(async move {
            let done = sqlx::query(
                "DELETE FROM a2a_run_journal
                 WHERE (completed != 0 AND updated_at < ?1) OR (completed = 0 AND updated_at < ?2)",
            )
            .bind(completed_before_ms)
            .bind(unfinished_before_ms)
            .execute(&self.pool)
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
            let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
                "SELECT {COLUMNS} FROM a2a_run_journal
                 WHERE principal = ?1 AND (?2 IS NULL OR tenant = ?2) ORDER BY run_key"
            )))
            .bind(principal)
            .bind(tenant)
            .fetch_all(&self.pool)
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
            let mut conn = self.pool.acquire().await.map_err(|e| failed("erase", &e))?;
            // Overwrite the freed pages: a run's content is what is erased.
            sqlx::query("PRAGMA secure_delete = ON")
                .execute(&mut *conn)
                .await
                .map_err(|e| failed("erase", &e))?;
            let done = sqlx::query(
                "DELETE FROM a2a_run_journal WHERE principal = ?1 AND (?2 IS NULL OR tenant = ?2)",
            )
            .bind(principal)
            .bind(tenant)
            .execute(&mut *conn)
            .await
            .map_err(|e| failed("erase", &e))?;
            Ok(done.rows_affected())
        })
    }
}
