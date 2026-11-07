// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Replicas appending to one audit chain over one PostgreSQL database
//! (audit E-F18).
//!
//! Each "replica" is an `AuditLog` over a `PostgresAuditStore` of its own, with
//! its own pool and its own in-process chain lock, so the only thing between
//! two of them is the database. Every test is `#[ignore]`d for want of a live
//! server and creates and drops a scratch database of its own:
//!
//! ```bash
//! A2A_TEST_POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres \
//!   cargo test -p a2a-protocol-server --features postgres,audit \
//!   --test postgres_audit_append -- --ignored
//! ```

#![cfg(all(feature = "postgres", feature = "audit"))]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use a2a_protocol_server::audit::record::{AuditRecord, kind};
use a2a_protocol_server::audit::{AuditLog, AuditStore, PostgresAuditStore};
use a2a_protocol_types::error::A2aResult;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokio::sync::Barrier;

const URL_ENV: &str = "A2A_TEST_POSTGRES_URL";

/// A scratch database for one test.
struct Scratch {
    admin_url: String,
    name: String,
    url: String,
}

impl Scratch {
    async fn create(tag: &str) -> Self {
        let admin_url = std::env::var(URL_ENV)
            .unwrap_or_else(|_| panic!("{URL_ENV} must point at a live PostgreSQL server"));
        let (base, _) = admin_url.rsplit_once('/').expect("a database in the URL");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let name = format!("a2a_test_audit_{tag}_{nanos}");
        let admin = PgPool::connect(&admin_url).await.expect("admin");
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&admin)
            .await
            .expect("create database");
        admin.close().await;
        let url = format!("{base}/{name}");
        Self {
            admin_url,
            name,
            url,
        }
    }

    /// A replica: a store with a pool of its own (at least two connections,
    /// as `PostgresAuditStore` documents) over this database.
    async fn store(&self) -> PostgresAuditStore {
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect(&self.url)
            .await
            .expect("connect");
        PostgresAuditStore::from_pool(pool).await.expect("schema")
    }

    async fn drop_db(self) {
        let admin = PgPool::connect(&self.admin_url).await.expect("admin");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.name
        )))
        .execute(&admin)
        .await
        .expect("drop database");
        admin.close().await;
    }
}

fn record(task: &str) -> AuditRecord {
    let mut r = AuditRecord::new("c", kind::CALL);
    r.task_id = Some(task.to_owned());
    r
}

/// The database's clock in Unix milliseconds.
async fn database_now_ms(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT floor(extract(epoch from clock_timestamp()) * 1000)::bigint")
        .fetch_one(pool)
        .await
        .expect("clock")
}

/// Six replicas append forty records each to one chain at once. Every append
/// is stored the first time (none is refused for losing a position eight times
/// running, as the optimistic protocol did at this contention), the chain is
/// exactly 1..=240 and verifies, and its times never run backwards.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn replicas_appending_to_one_chain_at_once_all_succeed_and_the_chain_verifies() {
    const REPLICAS: usize = 6;
    const EACH: usize = 40;
    let db = Scratch::create("contend").await;

    let mut workers = Vec::new();
    for replica in 0..REPLICAS {
        let log = AuditLog::new(Arc::new(db.store().await));
        workers.push(tokio::spawn(async move {
            for i in 0..EACH {
                log.append(record(&format!("r{replica}-{i}")))
                    .await
                    .unwrap_or_else(|e| panic!("replica {replica} append {i}: {e}"));
            }
        }));
    }
    for w in workers {
        w.await.expect("a replica finished");
    }

    let reader = AuditLog::new(Arc::new(db.store().await));
    let records = reader.export("c").await.unwrap();
    assert_eq!(
        records.iter().map(|r| r.seq).collect::<Vec<_>>(),
        (1..=u64::try_from(REPLICAS * EACH).unwrap()).collect::<Vec<_>>()
    );
    let report = reader.verify("c").await.unwrap();
    assert!(report.is_intact(), "{report:?}");
    let times: Vec<i64> = records
        .iter()
        .map(|r| a2a_protocol_types::parse_iso8601_to_unix_millis(&r.time).expect("a time"))
        .collect();
    assert!(
        times.windows(2).all(|w| w[0] <= w[1]),
        "one clock times one chain, so its times do not run backwards"
    );

    db.drop_db().await;
}

/// The store hands the sealer its own clock, which is the database's, and the
/// head under the lock.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn the_sealer_is_given_the_head_and_the_databases_clock() {
    let db = Scratch::create("clock").await;
    let store = db.store().await;
    let probe = PgPool::connect(&db.url).await.unwrap();

    let before = database_now_ms(&probe).await;
    let seen = std::sync::Mutex::new(Vec::new());
    let seal = |head: Option<(u64, String)>, ms: Option<i64>| {
        seen.lock().unwrap().push((head.clone(), ms));
        let (seq, prev) = match head {
            Some((seq, hash)) => (seq + 1, Some(hash)),
            None => (1, None),
        };
        Box::pin(async move {
            let mut r = record("t");
            r.seal(seq, prev, "2026-01-01T00:00:00.000Z".to_owned())?;
            A2aResult::Ok(r)
        }) as a2a_protocol_server::audit::BoxFuture<'_, _>
    };
    let first = store.append_next("c", &seal).await.unwrap().unwrap();
    let second = store.append_next("c", &seal).await.unwrap().unwrap();
    let after = database_now_ms(&probe).await;

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, None, "an empty chain has no head");
    assert_eq!(seen[1].0, Some((1, first.hash.clone())));
    assert_eq!(second.prev.as_deref(), Some(first.hash.as_str()));
    for (_, ms) in &seen {
        let ms = ms.expect("the store gives its clock");
        assert!(
            before <= ms && ms <= after,
            "{ms} is not the database's time ({before}..={after})"
        );
    }
    assert!(seen[0].1 <= seen[1].1);

    probe.close().await;
    db.drop_db().await;
}

/// Two appenders to one chain are serialised by the database: while one is
/// sealing, holding the chain's lock, the other has not been handed the head.
/// Appenders to different chains are not held up.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn appenders_to_one_chain_are_serialised_and_to_different_chains_are_not() {
    let db = Scratch::create("serial").await;

    async fn run(
        store: PostgresAuditStore,
        chain: &'static str,
        in_flight: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        meet: Option<Arc<Barrier>>,
    ) {
        let seal = |head: Option<(u64, String)>, _ms: Option<i64>| {
            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            let in_flight = Arc::clone(&in_flight);
            let meet = meet.clone();
            Box::pin(async move {
                match meet {
                    // Two chains: each sealer holds the seal until the other
                    // is inside it too, however long its connection and its
                    // lock took to get there. A fixed pause here made the
                    // overlap depend on the second appender's round trips
                    // fitting in it, which a starved CPU defeats (30 of 40
                    // runs, one CPU, eight at once). If the chains were
                    // serialised the other could never arrive, the wait would
                    // end at the deadline, and the peak below would be 1.
                    Some(both) => {
                        let _ = tokio::time::timeout(Duration::from_secs(30), both.wait()).await;
                    }
                    // One chain: nobody else can get in while this one holds
                    // it, whatever the pause, so a fixed pause is enough.
                    None => tokio::time::sleep(Duration::from_millis(300)).await,
                }
                in_flight.fetch_sub(1, Ordering::SeqCst);
                let (seq, prev) = match head {
                    Some((seq, hash)) => (seq + 1, Some(hash)),
                    None => (1, None),
                };
                let mut r = AuditRecord::new(chain, kind::CALL);
                r.seal(seq, prev, "2026-01-01T00:00:00.000Z".to_owned())?;
                A2aResult::Ok(r)
            }) as a2a_protocol_server::audit::BoxFuture<'_, _>
        };
        store.append_next(chain, &seal).await.unwrap().unwrap();
    }

    for (chains, expected_peak) in [(["c", "c"], 1), (["a", "b"], 2)] {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (one, other) = (db.store().await, db.store().await);
        let meet = (chains[0] != chains[1]).then(|| Arc::new(Barrier::new(2)));
        tokio::join!(
            run(
                one,
                chains[0],
                Arc::clone(&in_flight),
                Arc::clone(&peak),
                meet.clone()
            ),
            run(
                other,
                chains[1],
                Arc::clone(&in_flight),
                Arc::clone(&peak),
                meet.clone()
            ),
        );
        assert_eq!(
            peak.load(Ordering::SeqCst),
            expected_peak,
            "sealers at once on {chains:?}"
        );
    }

    db.drop_db().await;
}

/// An append cancelled while it holds the chain's lock does not leave it held:
/// the dropped transaction is rolled back when its connection returns to the
/// pool, and the next appender gets through.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn an_append_cancelled_under_the_lock_releases_it() {
    let db = Scratch::create("cancel").await;
    let store = db.store().await;

    // Sealing never finishes, so the cancel lands with the lock held.
    let stuck = |_head: Option<(u64, String)>, _ms: Option<i64>| {
        Box::pin(std::future::pending::<A2aResult<AuditRecord>>())
            as a2a_protocol_server::audit::BoxFuture<'_, _>
    };
    let cancelled =
        tokio::time::timeout(Duration::from_millis(300), store.append_next("c", &stuck)).await;
    assert!(
        cancelled.is_err(),
        "the sealer never returns, so the append times out"
    );

    let log = AuditLog::new(Arc::new(db.store().await));
    let sealed = tokio::time::timeout(Duration::from_secs(20), log.append(record("after")))
        .await
        .expect("the lock was released when the cancelled append was dropped")
        .unwrap();
    assert_eq!(sealed.seq, 1, "the cancelled append stored nothing");

    db.drop_db().await;
}

/// A writer that does not take the lock (SQL of one's own, or another
/// implementation) can still take the position the locked append was about to
/// store; the primary key turns the locked append away, and it says so with
/// `None` and stores nothing, so the log asks again from the new head.
#[tokio::test]
#[ignore = "requires a live PostgreSQL server (set A2A_TEST_POSTGRES_URL)"]
async fn a_position_taken_behind_the_locks_back_is_none() {
    let db = Scratch::create("behind").await;
    let locked = db.store().await;
    let intruder = Arc::new(db.store().await);

    let seal = |head: Option<(u64, String)>, _ms: Option<i64>| {
        assert_eq!(head, None, "the chain is empty when sealing starts");
        let intruder = Arc::clone(&intruder);
        Box::pin(async move {
            let mut taken = record("taken");
            taken.seal(1, None, "2026-01-01T00:00:00.000Z".to_owned())?;
            assert_eq!(
                intruder.append(&taken).await?,
                a2a_protocol_server::audit::Appended::Stored,
                "the unlocked writer is not held up by the lock"
            );
            let mut ours = record("ours");
            ours.seal(1, None, "2026-01-01T00:00:00.000Z".to_owned())?;
            A2aResult::Ok(ours)
        }) as a2a_protocol_server::audit::BoxFuture<'_, _>
    };
    assert_eq!(locked.append_next("c", &seal).await.unwrap(), None);

    let stored = locked.read("c", 0, 10).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].task_id.as_deref(), Some("taken"));

    db.drop_db().await;
}
