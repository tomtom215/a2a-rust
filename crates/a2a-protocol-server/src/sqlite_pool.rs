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
//! Two of the four change behaviour, and two currently restate a `sqlx`
//! default. Measured 2026-08-26 by deleting each line in turn and re-running
//! the test below, rather than read off anyone's documentation:
//!
//! | pragma | removing it | why it is set |
//! |---|---|---|
//! | `journal_mode=WAL` | **test fails** | readers do not block the writer, which is what makes a pool of 8 worth having at all; `sqlx` leaves this at `DELETE` |
//! | `synchronous=NORMAL` | **test fails** | the documented companion to WAL — durable across process crash, trading only the OS-crash window for throughput; `sqlx` leaves this at `FULL` |
//! | `busy_timeout=5000` | test still passes | `sqlx` already defaults to 5s. Kept because the value this code depends on should be stated where it is depended on, not inherited silently |
//! | `foreign_keys=ON` | test still passes | `sqlx` already turns it on, unlike raw `SQLite`. Kept for the same reason |
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

        pool.close().await;
        cleanup(&path);
    }
}
