// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! One place where `SQLite` connection pragmas are decided.
//!
//! The same four pragmas were written out four times — in `store::sqlite_store`,
//! `store::tenant_sqlite_store`, `push::sqlite_config_store` and
//! `push::tenant_sqlite_config_store` — byte-identical, with nothing asserting
//! they agreed. Three of the four also hard-coded the same pool size.
//!
//! That is the shape a silent divergence takes: correcting `synchronous` or
//! raising `busy_timeout` in the store a bug report names leaves the other
//! three at the old value, and every test still passes, because each pool is
//! only ever exercised against its own copy. The repository has already paid
//! for this once with `DEFAULT_MAX_PAGE_SIZE`. A comment asking the next
//! person to keep four copies in step is not a guard; one function is.
//!
//! Three of the five change behaviour, and two currently restate a `sqlx`
//! default. Measured 2026-08-26 by deleting each line in turn and re-running
//! the test below, rather than read off anyone's documentation:
//!
//! | pragma | removing it | why it is set |
//! |---|---|---|
//! | `journal_mode=WAL` | **test fails** | readers do not block the writer, which is what makes a pool of 8 worth having at all; `sqlx` leaves this at `DELETE` |
//! | `synchronous=NORMAL` | **test fails** | the documented companion to WAL — durable across process crash, trading only the OS-crash window for throughput; `sqlx` leaves this at `FULL` |
//! | `busy_timeout=5000` | test still passes | `sqlx` already defaults to 5s. Kept because the value this code depends on should be stated where it is depended on, not inherited silently |
//! | `foreign_keys=ON` | test still passes | `sqlx` already turns it on, unlike raw `SQLite`. Kept for the same reason |
//! | `secure_delete=ON` | **test fails** | a deleted or rewritten cell is overwritten with zeros in the database file, instead of staying in the freed page until something reuses it (GDPR Art. 17, CRA Annex I Part I (2)(m)); `SQLite` leaves this off |
//!
//! The two that "still pass" are not dead weight and are not proof either. They
//! pin an effective configuration this code relies on; if `sqlx` ever changed
//! one of those defaults, the test below would catch it and these lines would
//! start doing the work they currently only describe.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::str::FromStr;

/// The pool size every caller used before this module existed.
const DEFAULT_MAX_CONNECTIONS: u32 = 8;

/// Creates a `SqlitePool` with production-ready defaults and the default size.
pub async fn sqlite_pool(url: &str) -> Result<SqlitePool, sqlx::Error> {
    sqlite_pool_with_size(url, DEFAULT_MAX_CONNECTIONS).await
}

/// Creates a `SqlitePool` with production-ready defaults and a specific size.
///
/// Private: nothing outside this module has ever asked for a non-default size.
/// It exists so the default above is a named value rather than a literal at the
/// one call site.
async fn sqlite_pool_with_size(url: &str, max_connections: u32) -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(url)?
        .pragma("journal_mode", "WAL")
        .pragma("busy_timeout", "5000")
        .pragma("synchronous", "NORMAL")
        .pragma("foreign_keys", "ON")
        // Deleted content is zeroed in the main database file, not left in the
        // free page. It does not reach the write-ahead log, which holds the
        // earlier image of a page until a checkpoint truncates it, nor a
        // backup or a copy of the file.
        .pragma("secure_delete", "ON")
        .create_if_missing(true);

    SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect_with(opts)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Centralising the pragmas is only worth anything if they take effect, so
    /// this asks `SQLite` itself rather than asserting the builder was called.
    ///
    /// On a real file, not `sqlite::memory:`, which every other test here uses:
    /// an in-memory database reports `journal_mode = memory` and can never be
    /// in WAL, so the assertion that matters most would have passed on a
    /// database that could not have been wrong.
    ///
    /// What this does and does not prove, measured rather than assumed:
    /// deleting `journal_mode` or `synchronous` from the builder fails it;
    /// deleting `busy_timeout` or `foreign_keys` does not, because `sqlx`
    /// already defaults both to the values asked for. Those two assertions
    /// therefore guard the *effective* configuration — including against a
    /// future `sqlx` changing its mind — not the two lines above them.
    #[tokio::test]
    async fn every_pragma_is_in_force_on_a_pooled_connection() {
        let path = std::env::temp_dir().join(format!("a2a-pragmas-{}.db", std::process::id()));
        let cleanup = |p: &std::path::Path| {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
            }
        };
        cleanup(&path);

        let pool = sqlite_pool(&format!("sqlite://{}", path.display()))
            .await
            .expect("pool");

        let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .expect("journal_mode");
        assert_eq!(
            journal.to_lowercase(),
            "wal",
            "WAL is what makes a pool worth having"
        );

        let busy: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
            .fetch_one(&pool)
            .await
            .expect("busy_timeout");
        assert_eq!(
            busy, 5000,
            "without this a contended write is a spurious internal error"
        );

        // 1 == NORMAL.
        let sync: i64 = sqlx::query_scalar("PRAGMA synchronous")
            .fetch_one(&pool)
            .await
            .expect("synchronous");
        assert_eq!(sync, 1);

        // SQLite defaults this OFF, so the schema's referential integrity is
        // not enforced unless every connection asks. That is the whole reason
        // this pragma is in the list.
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .expect("foreign_keys");
        assert_eq!(fk, 1);

        // 1 == ON.
        let secure: i64 = sqlx::query_scalar("PRAGMA secure_delete")
            .fetch_one(&pool)
            .await
            .expect("secure_delete");
        assert_eq!(secure, 1, "deleted content must not stay in the free page");

        pool.close().await;
        cleanup(&path);
    }

    /// Whether `MARKER` is still in the database file after its row was
    /// deleted, with `secure_delete` set as asked on the one connection.
    ///
    /// Reads the file's bytes, which is the thing erasure is about: that the
    /// pragma is set says nothing about whether the page was overwritten.
    /// Checkpointed with `TRUNCATE` before and after, so the main file, not
    /// the write-ahead log, holds the pages being read.
    async fn marker_survives_a_delete(secure: bool) -> bool {
        const MARKER: &str = "erase-me-3f9c1a7e-marker";
        const INSERT: &str = "INSERT INTO t (body) VALUES ('erase-me-3f9c1a7e-marker')";
        assert!(INSERT.contains(MARKER));
        let path = std::env::temp_dir().join(format!(
            "a2a-secure-delete-{}-{secure}.db",
            std::process::id()
        ));
        let cleanup = |p: &std::path::Path| {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
            }
        };
        cleanup(&path);
        let pool = sqlite_pool_with_size(&format!("sqlite://{}", path.display()), 1)
            .await
            .expect("pool");
        {
            let mut conn = pool.acquire().await.expect("connection");
            let pragma = if secure {
                "PRAGMA secure_delete = ON"
            } else {
                "PRAGMA secure_delete = OFF"
            };
            for statement in [
                pragma,
                "CREATE TABLE t (id INTEGER PRIMARY KEY, body TEXT NOT NULL)",
                INSERT,
                "PRAGMA wal_checkpoint(TRUNCATE)",
                "DELETE FROM t",
                "PRAGMA wal_checkpoint(TRUNCATE)",
            ] {
                sqlx::query(statement)
                    .execute(&mut *conn)
                    .await
                    .unwrap_or_else(|e| panic!("{statement}: {e}"));
            }
        }
        pool.close().await;
        let bytes = std::fs::read(&path).expect("the database file");
        cleanup(&path);
        bytes
            .windows(MARKER.len())
            .any(|window| window == MARKER.as_bytes())
    }

    /// Erasure that leaves the bytes in the file is not erasure (GDPR
    /// Art. 17; CRA Annex I Part I (2)(m)). The control, with the pragma off,
    /// finds the marker where the row was; with it on, it is gone.
    #[tokio::test]
    async fn a_deleted_row_does_not_stay_in_the_database_file() {
        assert!(
            marker_survives_a_delete(false).await,
            "the control: without secure_delete the deleted text stays in the file"
        );
        assert!(
            !marker_survives_a_delete(true).await,
            "with secure_delete the deleted text must not stay in the file"
        );
    }
}
