// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Steering messages in SQLite (ADR 0036): the table, and the statements that
//! hand a message to the replica holding a claim. Processes share the file
//! only on one host, and SQLite has no notification, so a handler finds a
//! message at its next look, every turn-claim renewal interval.

use a2a_protocol_types::error::{A2aError, A2aResult};
use sqlx::sqlite::SqlitePool;
use tokio::sync::OnceCell;

use super::{Handed, PendingSteering};

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS a2a_task_steering (
    seq      INTEGER PRIMARY KEY AUTOINCREMENT,
    token    TEXT NOT NULL,
    message  TEXT NOT NULL
)";

/// Reads and deletes go by claim.
const CREATE_INDEX_SQL: &str =
    "CREATE INDEX IF NOT EXISTS idx_a2a_task_steering_token ON a2a_task_steering(token, seq)";

fn failed(what: &str, e: &sqlx::Error) -> A2aError {
    A2aError::internal(format!("turn claims steering {what}: {e}"))
}

/// The table, created once per store: a failure is retried by the next use.
pub(super) async fn ensure(pool: &SqlitePool, ready: &OnceCell<()>) -> A2aResult<()> {
    ready
        .get_or_try_init(|| async {
            for statement in [CREATE_TABLE_SQL, CREATE_INDEX_SQL] {
                sqlx::query(statement)
                    .execute(pool)
                    .await
                    .map_err(|e| failed("migrate", &e))?;
            }
            Ok::<(), A2aError>(())
        })
        .await?;
    Ok(())
}

/// Stores `message` for the live claim on `task_id`, unless that claim's queue
/// holds `capacity` messages. One statement, so the claim is read and the
/// message stored under one write lock: the queue cannot go past `capacity`,
/// and nothing is stored for a claim that was released first.
pub(super) async fn hand(
    pool: &SqlitePool,
    scope: &str,
    task_id: &str,
    message: &str,
    capacity: usize,
    now_ms: i64,
) -> A2aResult<Handed> {
    let capacity = i64::try_from(capacity).unwrap_or(i64::MAX);
    let stored: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO a2a_task_steering (token, message)
         SELECT c.token, ?3 FROM a2a_task_claims c
         WHERE c.scope = ?1 AND c.task_id = ?2 AND c.expires_at_ms > ?4
           AND (SELECT count(*) FROM a2a_task_steering s WHERE s.token = c.token) < ?5
         RETURNING seq",
    )
    .bind(scope)
    .bind(task_id)
    .bind(message)
    .bind(now_ms)
    .bind(capacity)
    .fetch_optional(pool)
    .await
    .map_err(|e| failed("hand", &e))?;
    if stored.is_some() {
        return Ok(Handed::Queued);
    }
    // Nothing was stored: either no live claim, or a queue at capacity.
    let live: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM a2a_task_claims
         WHERE scope = ?1 AND task_id = ?2 AND expires_at_ms > ?3",
    )
    .bind(scope)
    .bind(task_id)
    .bind(now_ms)
    .fetch_optional(pool)
    .await
    .map_err(|e| failed("hand", &e))?;
    Ok(if live.is_some() {
        Handed::Full
    } else {
        Handed::NoClaim
    })
}

/// What is stored for the live claims among `tokens` that `owner` holds.
pub(super) async fn peek(
    pool: &SqlitePool,
    owner: &str,
    tokens: &[String],
    limit: usize,
    now_ms: i64,
) -> A2aResult<Vec<PendingSteering>> {
    let wanted = serde_json::to_string(tokens)
        .map_err(|e| A2aError::internal(format!("turn claims steering peek: {e}")))?;
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT s.seq, s.token, s.message
         FROM a2a_task_steering s JOIN a2a_task_claims c ON c.token = s.token
         WHERE c.owner = ?1 AND c.expires_at_ms > ?3
           AND c.token IN (SELECT value FROM json_each(?2))
         ORDER BY s.seq LIMIT ?4",
    )
    .bind(owner)
    .bind(wanted)
    .bind(now_ms)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| failed("peek", &e))?;
    Ok(rows
        .into_iter()
        .map(|(seq, token, message)| PendingSteering {
            seq,
            token,
            message,
        })
        .collect())
}

/// Deletes the messages `seqs` names.
pub(super) async fn ack(pool: &SqlitePool, seqs: &[i64]) -> A2aResult<()> {
    let names = serde_json::to_string(seqs)
        .map_err(|e| A2aError::internal(format!("turn claims steering ack: {e}")))?;
    sqlx::query("DELETE FROM a2a_task_steering WHERE seq IN (SELECT value FROM json_each(?1))")
        .bind(names)
        .execute(pool)
        .await
        .map_err(|e| failed("ack", &e))?;
    Ok(())
}

/// Deletes what is stored for `token`, a claim that ended.
pub(super) async fn forget(pool: &SqlitePool, token: &str) -> A2aResult<()> {
    sqlx::query("DELETE FROM a2a_task_steering WHERE token = ?1")
        .bind(token)
        .execute(pool)
        .await
        .map_err(|e| failed("release", &e))?;
    Ok(())
}

/// Deletes what is stored for claims that no longer exist.
pub(super) async fn sweep(pool: &SqlitePool) -> A2aResult<u64> {
    let done = sqlx::query(
        "DELETE FROM a2a_task_steering
         WHERE NOT EXISTS (SELECT 1 FROM a2a_task_claims c WHERE c.token = a2a_task_steering.token)",
    )
    .execute(pool)
    .await
    .map_err(|e| failed("sweep", &e))?;
    Ok(done.rows_affected())
}
