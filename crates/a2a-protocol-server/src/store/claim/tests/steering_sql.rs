// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The steering cases that need a SQL database's own tables: the table is made
//! on first use, and messages for claims that are gone are swept.

use std::time::Duration;

use crate::store::claim::Handed;
use crate::store::claim::steering_tests::{claimed, every_case, fresh};

#[cfg(feature = "sqlite")]
mod sqlite {
    use super::*;
    use crate::store::TaskStore;

    fn url() -> (String, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("a2a-steering-{}.db", uuid::Uuid::new_v4()));
        (format!("sqlite:{}?mode=rwc", path.display()), path)
    }

    fn remove(path: &std::path::Path) {
        for suffix in ["", "-wal", "-shm"] {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            let _ = std::fs::remove_file(std::path::PathBuf::from(name));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn sqlite_hands_steering_over() {
        let (url, path) = url();
        let plain = crate::store::SqliteTaskStore::new(&url).await.unwrap();
        every_case(plain.turn_claims().unwrap()).await;
        let tenants = crate::store::TenantAwareSqliteTaskStore::new(&url)
            .await
            .unwrap();
        let claims = tenants.turn_claims().unwrap();
        assert!(
            claims.steering_signals().is_none(),
            "SQLite sends no signal"
        );
        every_case(claims).await;
        remove(&path);
    }

    /// Messages for claims that no longer exist are swept, and a message for
    /// a live claim is not; deleting a claim's row without `release` (a
    /// replica that died) is how an orphan is made.
    #[tokio::test]
    async fn sqlite_sweeps_the_messages_of_claims_that_are_gone() {
        let (url, path) = url();
        let store = crate::store::SqliteTaskStore::new(&url).await.unwrap();
        let claims = store.turn_claims().unwrap();
        let (live, dead, stale) = (fresh(), fresh(), fresh());
        let live_token = claimed(&*claims, "", &live).await;
        claimed(&*claims, "", &dead).await;
        claimed(&*claims, "", &stale).await;
        for ids in [&live, &dead, &stale] {
            assert_eq!(
                claims.hand_steering("", &ids.task, "x", 10).await.unwrap(),
                Handed::Queued
            );
        }
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        // One claim is gone; another ran out long ago, which the sweep deletes.
        sqlx::query("DELETE FROM a2a_task_claims WHERE task_id = ?1")
            .bind(&dead.task)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE a2a_task_claims SET expires_at_ms = 0 WHERE task_id = ?1")
            .bind(&stale.task)
            .execute(&pool)
            .await
            .unwrap();
        let before: (i64,) = sqlx::query_as("SELECT count(*) FROM a2a_task_steering")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(before.0, 3);
        let swept = claims.sweep(Duration::from_secs(3600)).await.unwrap();
        assert_eq!(
            swept, 3,
            "the one claim it deleted and the two messages left without a claim"
        );
        let rows: Vec<(String,)> = sqlx::query_as("SELECT token FROM a2a_task_steering")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            rows,
            [(live_token,)],
            "only the live claim's message is left"
        );
        pool.close().await;
        remove(&path);
    }

    /// Releasing a claim deletes what was handed to it, and only that.
    #[tokio::test]
    async fn sqlite_releasing_a_claim_deletes_its_messages() {
        let (url, path) = url();
        let store = crate::store::SqliteTaskStore::new(&url).await.unwrap();
        let claims = store.turn_claims().unwrap();
        let (gone, kept) = (fresh(), fresh());
        let gone_token = claimed(&*claims, "", &gone).await;
        let kept_token = claimed(&*claims, "", &kept).await;
        for ids in [&gone, &kept] {
            assert_eq!(
                claims.hand_steering("", &ids.task, "x", 10).await.unwrap(),
                Handed::Queued
            );
        }
        claims.release(&gone_token).await.unwrap();
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let rows: Vec<(String,)> = sqlx::query_as("SELECT token FROM a2a_task_steering")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            rows,
            [(kept_token,)],
            "the released claim's message went with it, the other claim's stays"
        );
        pool.close().await;
        remove(&path);
    }

    /// The table is made when steering is first used, not before.
    #[tokio::test]
    async fn sqlite_makes_the_steering_table_when_it_is_first_used() {
        let (url, path) = url();
        let store = crate::store::SqliteTaskStore::new(&url).await.unwrap();
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        let exists = || async {
            let n: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM sqlite_master WHERE name = 'a2a_task_steering'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            n.0
        };
        assert_eq!(exists().await, 0, "a store that never steers has no table");
        let claims = store.turn_claims().unwrap();
        // Releasing, sweeping and renewing do not need it.
        claims.sweep(Duration::from_secs(1)).await.unwrap();
        claims.release("nothing").await.unwrap();
        assert_eq!(exists().await, 0);
        claims.hand_steering("", "t", "x", 1).await.unwrap();
        assert_eq!(exists().await, 1);
        pool.close().await;
        remove(&path);
    }
}

#[cfg(feature = "postgres")]
mod postgres {
    use super::*;
    use crate::store::TaskStore;

    fn url() -> String {
        std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL")
    }

    /// A database of its own for one test: counts and sessions are only exact
    /// where no other test writes (the mutation job runs them all at once).
    struct Scratch {
        admin: sqlx::postgres::PgPool,
        name: String,
        url: String,
    }

    impl Scratch {
        async fn new() -> Self {
            let admin_url = url();
            let admin = sqlx::postgres::PgPool::connect(&admin_url).await.unwrap();
            let name = format!("a2a_steer_test_{}", uuid::Uuid::new_v4().simple());
            sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
                .execute(&admin)
                .await
                .unwrap();
            let url = format!("{}/{name}", admin_url.rsplit_once('/').expect("path").0);
            Self { admin, name, url }
        }

        /// Sessions of this database whose last statement was a `LISTEN`.
        async fn listeners(&self) -> i64 {
            let n: (i64,) = sqlx::query_as(
                "SELECT count(*) FROM pg_stat_activity
                 WHERE datname = $1 AND query ILIKE 'listen%'",
            )
            .bind(&self.name)
            .fetch_one(&self.admin)
            .await
            .unwrap();
            n.0
        }

        /// Waits until `n` sessions of this database are listening, and fails
        /// if that is not so within `secs` (a listener that cannot connect, as
        /// when the server is out of connections, tries again).
        async fn wait_for_listeners(&self, n: i64, secs: u64) {
            let deadline = std::time::Instant::now() + Duration::from_secs(secs);
            while self.listeners().await != n && std::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert_eq!(self.listeners().await, n, "sessions listening");
        }

        async fn drop_database(self) {
            let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                "DROP DATABASE IF EXISTS {} WITH (FORCE)",
                self.name
            )))
            .execute(&self.admin)
            .await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_hands_steering_over() {
        let plain = crate::store::PostgresTaskStore::new(&url()).await.unwrap();
        every_case(plain.turn_claims().unwrap()).await;
        let tenants = crate::store::TenantAwarePostgresTaskStore::new(&url())
            .await
            .unwrap();
        every_case(tenants.turn_claims().unwrap()).await;
    }

    /// The listener tells a handler a message arrived, once it is
    /// listening, and a message handed over after that is announced.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_announces_a_stored_message() {
        let store = crate::store::PostgresTaskStore::new(&url()).await.unwrap();
        let claims = store.turn_claims().unwrap();
        let ids = fresh();
        claimed(&*claims, "", &ids).await;
        // Makes the table, so the listener's first look finds it.
        claims
            .hand_steering("", &ids.task, "first", 10)
            .await
            .unwrap();

        let mut signals = claims.steering_signals().expect("PostgreSQL announces");
        // The listener announces itself once it is listening.
        tokio::time::timeout(Duration::from_secs(10), signals.recv())
            .await
            .expect("the listener connected")
            .expect("a signal");
        claims
            .hand_steering("", &ids.task, "second", 10)
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), signals.recv())
            .await
            .expect("the message was announced")
            .expect("a signal");
    }

    /// Messages for claims that no longer exist are swept, and the count
    /// `sweep` returns is the claims it deleted plus those messages.
    #[tokio::test]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_sweeps_the_messages_of_claims_that_are_gone() {
        let db = Scratch::new().await;
        let store = crate::store::PostgresTaskStore::new(&db.url).await.unwrap();
        let claims = store.turn_claims().unwrap();
        let (live, dead, stale) = (fresh(), fresh(), fresh());
        let live_token = claimed(&*claims, "", &live).await;
        claimed(&*claims, "", &dead).await;
        claimed(&*claims, "", &stale).await;
        for ids in [&live, &dead, &stale] {
            assert_eq!(
                claims.hand_steering("", &ids.task, "x", 10).await.unwrap(),
                Handed::Queued
            );
        }
        let pool = sqlx::postgres::PgPool::connect(&db.url).await.unwrap();
        // One claim is gone; another ran out long ago, which the sweep deletes.
        sqlx::query("DELETE FROM a2a_task_claims WHERE task_id = $1")
            .bind(&dead.task)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE a2a_task_claims SET expires_at = now() - interval '2 hours' WHERE task_id = $1",
        )
        .bind(&stale.task)
        .execute(&pool)
        .await
        .unwrap();
        let swept = claims.sweep(Duration::from_secs(3600)).await.unwrap();
        assert_eq!(
            swept, 3,
            "the one claim it deleted and the two messages left without a claim"
        );
        let rows: Vec<(String,)> = sqlx::query_as("SELECT token FROM a2a_task_steering")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(
            rows,
            [(live_token,)],
            "only the live claim's message is left"
        );
        pool.close().await;
        drop(claims);
        drop(store);
        db.drop_database().await;
    }

    /// Releasing a claim deletes what was handed to it, and only that.
    #[tokio::test]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_releasing_a_claim_deletes_its_messages() {
        let store = crate::store::PostgresTaskStore::new(&url()).await.unwrap();
        let claims = store.turn_claims().unwrap();
        let (gone, kept) = (fresh(), fresh());
        let gone_token = claimed(&*claims, "", &gone).await;
        let kept_token = claimed(&*claims, "", &kept).await;
        for ids in [&gone, &kept] {
            assert_eq!(
                claims.hand_steering("", &ids.task, "x", 10).await.unwrap(),
                Handed::Queued
            );
        }
        claims.release(&gone_token).await.unwrap();
        let pool = sqlx::postgres::PgPool::connect(&url()).await.unwrap();
        // Only these two claims' rows: other tests share this database.
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT token FROM a2a_task_steering WHERE token = $1 OR token = $2")
                .bind(&gone_token)
                .bind(&kept_token)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            rows,
            [(kept_token,)],
            "the released claim's message went with it, the other claim's stays"
        );
        pool.close().await;
    }

    /// The `LISTEN` session belongs to the store: it ends when the store and
    /// its claims are dropped, instead of listening for as long as the
    /// process runs.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
    async fn postgres_stops_listening_when_its_store_is_dropped() {
        let db = Scratch::new().await;
        let store = crate::store::PostgresTaskStore::new(&db.url).await.unwrap();
        let claims = store.turn_claims().unwrap();
        let signals = claims.steering_signals().expect("PostgreSQL announces");
        db.wait_for_listeners(1, 30).await;
        drop(signals);
        drop(claims);
        drop(store);
        db.wait_for_listeners(0, 30).await;
        db.drop_database().await;
    }
}
