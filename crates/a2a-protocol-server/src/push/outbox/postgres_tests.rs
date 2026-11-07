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

#[tokio::test]
#[ignore = "needs PostgreSQL at A2A_TEST_POSTGRES_URL"]
async fn the_postgres_outbox_keeps_the_contract_across_replicas_and_restarts() {
    let url = std::env::var("A2A_TEST_POSTGRES_URL").expect("A2A_TEST_POSTGRES_URL");

    contract::run_all(|| emptied(&url)).await;

    // Two replicas claim at once: every row is claimed by exactly one.
    let a = emptied(&url).await;
    let b = PostgresPushOutbox::new(&url).await.expect("outbox");
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

    // A webhook's later row is not claimable while its earlier one is
    // claimed by the other replica.
    let a = emptied(&url).await;
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

    // What one pool recorded, a new pool claims once the lease is gone.
    let a = emptied(&url).await;
    let row = delivery("t", "a", TaskState::Completed);
    let id = row.delivery_id.clone();
    a.record(vec![row], 10).await.unwrap();
    a.claim("gone", 10, Duration::ZERO).await.unwrap();
    a.pool.close().await;
    let later = PostgresPushOutbox::new(&url).await.expect("outbox");
    let claimed = later.claim("next", 10, lease).await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].delivery.delivery_id, id);
    assert!(claimed[0].age < Duration::from_secs(60));
    assert!(format!("{later:?}").starts_with("PostgresPushOutbox"));
}
