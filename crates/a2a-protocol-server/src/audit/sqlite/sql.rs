// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The tables and conversions of [`SqliteAuditStore`](super::SqliteAuditStore).

use a2a_protocol_types::error::{A2aError, A2aResult};

pub(super) const SCHEMA: [&str; 5] = [
    "CREATE TABLE IF NOT EXISTS a2a_audit_records (
        chain   TEXT    NOT NULL,
        seq     INTEGER NOT NULL CHECK (seq > 0),
        time_ms INTEGER NOT NULL,
        hash    TEXT    NOT NULL,
        record  TEXT    NOT NULL,
        PRIMARY KEY (chain, seq)
    )",
    "CREATE TABLE IF NOT EXISTS a2a_audit_checkpoints (
        chain      TEXT    NOT NULL,
        seq        INTEGER NOT NULL,
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

pub(super) fn db(e: &sqlx::Error) -> A2aError {
    crate::sql_error::sql_error(crate::sql_error::Db::Sqlite, "sqlite audit store", e)
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
