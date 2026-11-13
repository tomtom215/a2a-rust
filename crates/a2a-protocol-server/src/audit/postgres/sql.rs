// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tables, statements and conversions of [`PostgresAuditStore`](super::PostgresAuditStore).

use a2a_protocol_types::error::{A2aError, A2aResult};

pub(super) const SCHEMA: [&str; 5] = [
    "CREATE TABLE IF NOT EXISTS a2a_audit_records (
        chain   TEXT    NOT NULL,
        seq     BIGINT  NOT NULL CHECK (seq > 0),
        time_ms BIGINT  NOT NULL,
        hash    TEXT    NOT NULL,
        record  TEXT    NOT NULL,
        PRIMARY KEY (chain, seq)
    )",
    "CREATE TABLE IF NOT EXISTS a2a_audit_checkpoints (
        chain      TEXT    NOT NULL,
        seq        BIGINT  NOT NULL,
        kind       TEXT    NOT NULL,
        checkpoint TEXT    NOT NULL,
        PRIMARY KEY (chain, seq, kind)
    )",
    "CREATE TABLE IF NOT EXISTS a2a_audit_holds (
        chain     TEXT PRIMARY KEY,
        reason    TEXT NOT NULL,
        placed_at TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_a2a_audit_records_time ON a2a_audit_records(chain, time_ms)",
    // One row per person per chain, for AuditPrivacy; deleting a row is
    // what erases that person from the records sealed under its key.
    "CREATE TABLE IF NOT EXISTS a2a_audit_subject_keys (
        chain   TEXT NOT NULL,
        lookup  TEXT NOT NULL,
        sid     TEXT NOT NULL,
        wrapped TEXT NOT NULL,
        PRIMARY KEY (chain, lookup),
        UNIQUE (chain, sid)
    )",
];

/// Stores a sealed record unless its `(chain, seq)` is taken.
pub(super) const INSERT_SQL: &str =
    "INSERT INTO a2a_audit_records (chain, seq, time_ms, hash, record)
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT (chain, seq) DO NOTHING";

/// Takes the chain's append lock: one per chain, held to the end of the
/// transaction, named like the turn locks in `store::claim`. `$1` is the chain.
pub(super) const LOCK_CHAIN_SQL: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('a2a.audit:' || $1, 0))";

/// The chain's head, if it has one, with the database's clock in Unix
/// milliseconds, in the one row this always returns. `clock_timestamp()`, not
/// `now()`: the time of this statement, not of the transaction's start, which
/// is before the wait for the lock. `$1` is the chain.
pub(super) const HEAD_AND_CLOCK_SQL: &str = "SELECT h.seq, h.hash,
        floor(extract(epoch from clock_timestamp()) * 1000)::bigint AS now_ms
     FROM (SELECT 1) AS one
     LEFT JOIN LATERAL (
        SELECT seq, hash FROM a2a_audit_records WHERE chain = $1 ORDER BY seq DESC LIMIT 1
     ) AS h ON true";

pub(super) fn db(e: &sqlx::Error) -> A2aError {
    crate::sql_error::sql_error(crate::sql_error::Db::Postgres, "postgres audit store", e)
}

pub(super) fn json<T: serde::Serialize>(v: &T) -> A2aResult<String> {
    serde_json::to_string(v).map_err(|e| A2aError::internal(format!("audit serialization: {e}")))
}

pub(super) fn parse<T: serde::de::DeserializeOwned>(s: &str) -> A2aResult<T> {
    serde_json::from_str(s).map_err(|e| A2aError::internal(format!("audit row is not valid: {e}")))
}

#[allow(clippy::cast_possible_wrap)]
pub(super) const fn i64_of(seq: u64) -> i64 {
    seq as i64
}

#[allow(clippy::cast_sign_loss)]
pub(super) const fn u64_of(seq: i64) -> u64 {
    seq as u64
}
