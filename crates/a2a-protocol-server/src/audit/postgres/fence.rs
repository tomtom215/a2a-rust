// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The append, and the key deletions, that share the chain's advisory lock:
//! a record is stored only while the keys it is sealed under exist, and a key
//! is deleted only while no record stored since its scan names it, or no
//! hold is placed (adversarial review N03 and S8, 2026-11-13). Every statement
//! runs on the one connection of its transaction: none asks the pool for a
//! second (A2).

use a2a_protocol_types::audit::AuditRecord;
use a2a_protocol_types::error::A2aResult;
use sqlx::Row;
use sqlx::postgres::PgPool;

use super::super::store::{NamedAppend, Sealer, record_millis, sid_marker};
use super::sql::{HEAD_AND_CLOCK_SQL, INSERT_SQL, LOCK_CHAIN_SQL, db, i64_of, json, u64_of};

/// Reads the head and stores the record that follows it in one transaction
/// that holds the chain's advisory lock; with `sids`, only if every key named
/// is in the table, read under the same lock that a key deletion takes.
pub(super) async fn append_next(
    pool: &PgPool,
    chain: &str,
    sids: &[String],
    seal: &Sealer<'_>,
) -> A2aResult<NamedAppend> {
    let mut tx = pool.begin().await.map_err(|e| db(&e))?;
    sqlx::query(LOCK_CHAIN_SQL)
        .bind(chain)
        .execute(&mut *tx)
        .await
        .map_err(|e| db(&e))?;
    if !sids.is_empty() {
        let present: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM a2a_audit_subject_keys WHERE chain = $1 AND sid = ANY($2)",
        )
        .bind(chain)
        .bind(sids)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| db(&e))?;
        if usize::try_from(present).unwrap_or(0) < sids.len() {
            tx.rollback().await.map_err(|e| db(&e))?;
            return Ok(NamedAppend::KeyGone);
        }
    }
    let row = sqlx::query(HEAD_AND_CLOCK_SQL)
        .bind(chain)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| db(&e))?;
    let head = row
        .get::<Option<i64>, _>(0)
        .zip(row.get::<Option<String>, _>(1))
        .map(|(seq, hash)| (u64_of(seq), hash));
    let record: AuditRecord = seal(head, Some(row.get::<i64, _>(2))).await?;
    let done = sqlx::query(INSERT_SQL)
        .bind(&record.chain)
        .bind(i64_of(record.seq))
        .bind(record_millis(&record))
        .bind(&record.hash)
        .bind(json(&record)?)
        .execute(&mut *tx)
        .await
        .map_err(|e| db(&e))?;
    tx.commit().await.map_err(|e| db(&e))?;
    Ok(if done.rows_affected() == 1 {
        NamedAppend::Stored(Box::new(record))
    } else {
        NamedAppend::Lost
    })
}

/// Deletes the key under the chain's advisory lock unless a record above
/// `scanned_through` names its `sid`.
pub(super) async fn delete_key_unless_named(
    pool: &PgPool,
    chain: &str,
    lookup: &str,
    sid: &str,
    scanned_through: u64,
) -> A2aResult<bool> {
    let mut tx = pool.begin().await.map_err(|e| db(&e))?;
    sqlx::query(LOCK_CHAIN_SQL)
        .bind(chain)
        .execute(&mut *tx)
        .await
        .map_err(|e| db(&e))?;
    let done = sqlx::query(
        "DELETE FROM a2a_audit_subject_keys WHERE chain = $1 AND lookup = $2
         AND NOT EXISTS (SELECT 1 FROM a2a_audit_records
                         WHERE chain = $1 AND seq > $3 AND strpos(record, $4) > 0)",
    )
    .bind(chain)
    .bind(lookup)
    .bind(i64_of(scanned_through))
    .bind(sid_marker(sid))
    .execute(&mut *tx)
    .await
    .map_err(|e| db(&e))?;
    tx.commit().await.map_err(|e| db(&e))?;
    Ok(done.rows_affected() == 1)
}

/// Deletes the keys under the chain's advisory lock, the one `place_hold`
/// takes, unless a hold is placed: all of them or none.
pub(super) async fn delete_keys_unless_held(
    pool: &PgPool,
    chain: &str,
    lookups: &[String],
) -> A2aResult<Option<Vec<String>>> {
    let mut tx = pool.begin().await.map_err(|e| db(&e))?;
    sqlx::query(LOCK_CHAIN_SQL)
        .bind(chain)
        .execute(&mut *tx)
        .await
        .map_err(|e| db(&e))?;
    let held: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM a2a_audit_holds WHERE chain = $1)")
            .bind(chain)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| db(&e))?;
    if held {
        tx.rollback().await.map_err(|e| db(&e))?;
        return Ok(None);
    }
    let sids: Vec<String> = sqlx::query_scalar(
        "DELETE FROM a2a_audit_subject_keys WHERE chain = $1 AND lookup = ANY($2) RETURNING sid",
    )
    .bind(chain)
    .bind(lookups)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| db(&e))?;
    tx.commit().await.map_err(|e| db(&e))?;
    Ok(Some(sids))
}
