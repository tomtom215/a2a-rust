// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The append, and the key deletions, that `SQLite` serialises: each check is
//! inside the one write statement that acts on it, and `SQLite` runs one
//! writer at a time, so a record is stored only while the keys it is sealed
//! under exist, and a key is deleted only while no record stored since its
//! scan names it, or no hold is placed (adversarial review N03 and S8,
//! 2026-11-13).

use a2a_protocol_types::audit::AuditRecord;
use a2a_protocol_types::error::A2aResult;
use sqlx::Row;
use sqlx::sqlite::SqlitePool;

use super::super::store::{NamedAppend, Sealer, record_millis, sid_marker};
use super::sql::{db, i64_of, json, u64_of};
use super::{Truncation, truncate_log};

/// Reads the head, seals the next record, and stores it only if the
/// position is free and every key in `sids` is in the table, in one
/// `INSERT … SELECT … WHERE`.
pub(super) async fn append_next_naming(
    pool: &SqlitePool,
    chain: &str,
    sids: &[String],
    seal: &Sealer<'_>,
) -> A2aResult<NamedAppend> {
    let head = sqlx::query(
        "SELECT seq, hash FROM a2a_audit_records WHERE chain = ?1 ORDER BY seq DESC LIMIT 1",
    )
    .bind(chain)
    .fetch_optional(pool)
    .await
    .map_err(|e| db(&e))?
    .map(|r| (u64_of(r.get::<i64, _>(0)), r.get::<String, _>(1)));
    let record: AuditRecord = seal(head, None).await?;
    let wanted = json(&sids)?;
    let done = sqlx::query(
        "INSERT INTO a2a_audit_records (chain, seq, time_ms, hash, record)
         SELECT ?1, ?2, ?3, ?4, ?5
         WHERE (SELECT count(*) FROM a2a_audit_subject_keys
                WHERE chain = ?1 AND sid IN (SELECT value FROM json_each(?6))) = ?7
         ON CONFLICT (chain, seq) DO NOTHING",
    )
    .bind(&record.chain)
    .bind(i64_of(record.seq))
    .bind(record_millis(&record))
    .bind(&record.hash)
    .bind(json(&record)?)
    .bind(&wanted)
    .bind(i64::try_from(sids.len()).unwrap_or(i64::MAX))
    .execute(pool)
    .await
    .map_err(|e| db(&e))?;
    if done.rows_affected() == 1 {
        return Ok(NamedAppend::Stored(Box::new(record)));
    }
    // Nothing stored: a key is gone, or the position was taken. Which one is
    // read after the fact; either way the caller seals afresh and asks again.
    let present: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM a2a_audit_subject_keys
         WHERE chain = ?1 AND sid IN (SELECT value FROM json_each(?2))",
    )
    .bind(chain)
    .bind(&wanted)
    .fetch_one(pool)
    .await
    .map_err(|e| db(&e))?;
    Ok(if usize::try_from(present).unwrap_or(0) < sids.len() {
        NamedAppend::KeyGone
    } else {
        NamedAppend::Lost
    })
}

/// Deletes the key unless a record above `scanned_through` names its `sid`,
/// in one `DELETE … WHERE NOT EXISTS`.
pub(super) async fn delete_key_unless_named(
    pool: &SqlitePool,
    chain: &str,
    lookup: &str,
    sid: &str,
    scanned_through: u64,
) -> A2aResult<bool> {
    let mut conn = pool.acquire().await.map_err(|e| db(&e))?;
    sqlx::query("PRAGMA secure_delete = ON")
        .execute(&mut *conn)
        .await
        .map_err(|e| db(&e))?;
    let done = sqlx::query(
        "DELETE FROM a2a_audit_subject_keys WHERE chain = ?1 AND lookup = ?2
         AND NOT EXISTS (SELECT 1 FROM a2a_audit_records
                         WHERE chain = ?1 AND seq > ?3 AND instr(record, ?4) > 0)",
    )
    .bind(chain)
    .bind(lookup)
    .bind(i64_of(scanned_through))
    .bind(sid_marker(sid))
    .execute(&mut *conn)
    .await
    .map_err(|e| db(&e))?;
    let deleted = done.rows_affected() == 1;
    if deleted {
        truncate_after_key_deletion(&mut conn).await;
    }
    Ok(deleted)
}

/// Deletes the keys unless a hold is placed, in one `DELETE … WHERE NOT
/// EXISTS` that takes the write lock a hold's insert needs too.
pub(super) async fn delete_keys_unless_held(
    pool: &SqlitePool,
    chain: &str,
    lookups: &[String],
) -> A2aResult<Option<Vec<String>>> {
    let mut conn = pool.acquire().await.map_err(|e| db(&e))?;
    sqlx::query("PRAGMA secure_delete = ON")
        .execute(&mut *conn)
        .await
        .map_err(|e| db(&e))?;
    let mut tx = sqlx::Connection::begin(&mut *conn)
        .await
        .map_err(|e| db(&e))?;
    let sids: Vec<String> = sqlx::query_scalar(
        "DELETE FROM a2a_audit_subject_keys
         WHERE chain = ?1 AND lookup IN (SELECT value FROM json_each(?2))
         AND NOT EXISTS (SELECT 1 FROM a2a_audit_holds WHERE chain = ?1)
         RETURNING sid",
    )
    .bind(chain)
    .bind(json(&lookups)?)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| db(&e))?;
    // The write lock is held: the answer cannot change before the commit.
    let held: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM a2a_audit_holds WHERE chain = ?1)")
            .bind(chain)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| db(&e))?;
    tx.commit().await.map_err(|e| db(&e))?;
    if held {
        return Ok(None);
    }
    if !sids.is_empty() {
        truncate_after_key_deletion(&mut conn).await;
    }
    Ok(Some(sids))
}

/// After a key's row is deleted: copies the write-ahead log back and
/// truncates it, so the deleted page's earlier images go too. The row is gone
/// either way, so no outcome of the truncation fails the call; the two that
/// leave the key's bytes on disk are logged.
pub(super) async fn truncate_after_key_deletion(conn: &mut sqlx::SqliteConnection) {
    match truncate_log(conn).await {
        Truncation::Done => {}
        Truncation::KeptByReader => {
            trace_warn!(
                "audit: a subject key was deleted, but a reader kept the write-ahead \
                 log from being truncated; its bytes stay there until a later checkpoint"
            );
        }
        Truncation::Failed => {
            trace_warn!("audit: a subject key was deleted, but the checkpoint after it failed");
        }
    }
}
