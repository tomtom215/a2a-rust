// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The PostgreSQL outbox against a live database (`A2A_TEST_POSTGRES_URL`;
//! `scripts/run_postgres_suites.py` runs it).
//!
//! One test, run in order, because every case uses the one table: the
//! contract on an emptied table each time, two replicas claiming at once,
//! and rows that outlive the pool that recorded them.

#![cfg(feature = "postgres")]

use std::time::Duration;

use a2a_protocol_types::task::TaskState;

use super::contract::{self, delivery};
use super::*;

async fn emptied(url: &str) -> PostgresPushOutbox {
    let outbox = PostgresPushOutbox::new(url).await.expect("outbox");
    crate::store::pg_retry::retry(&crate::metrics::NoopMetrics, || {
        sqlx::query("TRUNCATE a2a_push_outbox").execute(&outbox.pool)
    })
    .await
    .expect("truncate");
    outbox
}

async fn two_replicas_claim_at_once(url: &str) {
    // Two replicas claim at once: every row is claimed by exactly one.
    let a = emptied(url).await;
    let b = PostgresPushOutbox::new(url).await.expect("outbox");
    let rows: Vec<_> = (0..40)
        .map(|i| delivery("", &format!("c{i}"), TaskState::Working))
        .collect();
    a.record(rows, 10).await.unwrap();
    let lease = Duration::from_secs(60);
    let (mine, theirs) = tokio::join!(a.claim("a", 40, lease), b.claim("b", 40, lease));
    let (mine, theirs) = (mine.unwrap(), theirs.unwrap());
    let mut seqs: Vec<i64> = mine.iter().chain(&theirs).map(|c| c.seq).collect();
    seqs.sort_unstable();
    seqs.dedup();
    assert_eq!(seqs.len(), 40, "every row claimed once");
    assert_eq!(mine.len() + theirs.len(), 40, "and no row twice");
}

async fn a_later_row_waits_for_the_other_replicas_claim(url: &str) {
    let lease = Duration::from_secs(60);
    // A webhook's later row is not claimable while its earlier one is
    // claimed by the other replica.
    let a = emptied(url).await;
    let b = PostgresPushOutbox::new(url).await.expect("outbox");
    a.record(
        vec![
            delivery("", "w", TaskState::Working),
            delivery("", "w", TaskState::Completed),
        ],
        10,
    )
    .await
    .unwrap();
    assert_eq!(a.claim("a", 10, lease).await.unwrap().len(), 1);
    assert!(b.claim("b", 10, lease).await.unwrap().is_empty());
}

async fn rows_outlive_the_pool_that_recorded_them(url: &str) {
    let lease = Duration::from_secs(60);
    // What one pool recorded, a new pool claims once the lease is gone.
    let a = emptied(url).await;
    let row = delivery("t", "a", TaskState::Completed);
    let id = row.delivery_id.clone();
    a.record(vec![row], 10).await.unwrap();
    a.claim("gone", 10, Duration::ZERO).await.unwrap();
    a.pool.close().await;
    let later = PostgresPushOutbox::new(url).await.expect("outbox");
    let claimed = later.claim("next", 10, lease).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].delivery.delivery_id, id);
    assert!(claimed[0].age < Duration::from_secs(60));
    assert!(format!("{later:?}").starts_with("PostgresPushOutbox"));
}

async fn another_replica_settles_a_stopped_replicas_reservation(url: &str) {
    let lease = Duration::from_secs(60);
    // A replica reserves, then stops: another replica, not the one that
    // reserved, finds the reservation held, arms it, and claims the row, with
    // the delivery id the first replica gave it (audit H14).
    let a = emptied(url).await;
    let row = delivery("t", "a", TaskState::Working);
    let id = row.delivery_id.clone();
    a.reserve("r1", 5, vec![row], 10, Duration::ZERO)
        .await
        .unwrap();
    a.pool.close().await;
    let b = PostgresPushOutbox::new(url).await.expect("outbox");
    assert!(b.claim("b", 10, lease).await.unwrap().is_empty());
    let held = b.held(10).await.unwrap();
    assert_eq!((held[0].reservation.as_str(), held[0].log_seq), ("r1", 5));
    assert_eq!(b.arm("r1").await.unwrap(), 1);
    assert_eq!(
        b.claim("b", 10, lease).await.unwrap()[0]
            .delivery
            .delivery_id,
        id
    );
}

async fn two_replicas_settle_one_reservation(url: &str) {
    // Two replicas settle one reservation at once: arming is idempotent and
    // releasing leaves what is armed.
    let a = emptied(url).await;
    let b = PostgresPushOutbox::new(url).await.expect("outbox");
    a.reserve(
        "r2",
        6,
        vec![delivery("", "a", TaskState::Working)],
        10,
        Duration::ZERO,
    )
    .await
    .unwrap();
    let (armed_a, armed_b) = tokio::join!(a.arm("r2"), b.arm("r2"));
    assert_eq!((armed_a.unwrap(), armed_b.unwrap()), (1, 1));
    b.release("r2").await.unwrap();
    assert_eq!(a.pending().await.unwrap(), 1, "armed rows are not released");
}

async fn an_older_table_is_upgraded(url: &str) {
    let lease = Duration::from_secs(60);
    // A table an earlier build created, without the two-phase columns, is
    // upgraded in place and its rows are armed.
    let a = emptied(url).await;
    let old = delivery("t", "a", TaskState::Working);
    for column in ["armed", "reservation", "log_seq", "hold_until"] {
        let drop_column = format!("ALTER TABLE a2a_push_outbox DROP COLUMN {column}");
        sqlx::query(sqlx::AssertSqlSafe(drop_column))
            .execute(&a.pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO a2a_push_outbox (delivery_id, tenant, task_id, config_id, event, ends_turn)
         VALUES ($1, 't', 't1', 'a', $2, FALSE)",
    )
    .bind(&old.delivery_id)
    .bind(serde_json::to_string(&old.event).unwrap())
    .execute(&a.pool)
    .await
    .unwrap();
    let upgraded = PostgresPushOutbox::new(url).await.expect("outbox");
    assert_eq!(upgraded.claim("u", 10, lease).await.unwrap().len(), 1);
    assert!(upgraded.held(10).await.unwrap().is_empty());
}

#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_postgres_outbox_keeps_the_contract_across_replicas_and_restarts() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");

    contract::run_all(|| emptied(&url)).await;
    two_replicas_claim_at_once(&url).await;
    a_later_row_waits_for_the_other_replicas_claim(&url).await;
    rows_outlive_the_pool_that_recorded_them(&url).await;
    another_replica_settles_a_stopped_replicas_reservation(&url).await;
    two_replicas_settle_one_reservation(&url).await;
    an_older_table_is_upgraded(&url).await;
}
