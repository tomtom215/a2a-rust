// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The per-origin state of an `HttpPool` (ADR 0032 decision 5): a circuit
//! breaker, an in-flight cap and a retry budget that every client of the
//! pool shares, and the snapshot that shows them.

use std::sync::Arc;
use std::time::Duration;

use a2a_protocol_client::balance::{BalancePolicy, Ejection, StaticEndpoints};
use a2a_protocol_client::{ClientBuilder, ClientError, HttpPool, RetryBudget, RetryPolicy};

use super::harness::{Mode, Replica, get, urls};

const SECOND: Duration = Duration::from_secs(1);

/// Two clients of one pool: the failures of one open the circuit for both,
/// and the refused calls are not sent.
#[tokio::test]
async fn the_breaker_is_shared_by_the_pools_clients() {
    let replica = Replica::start("a").await;
    replica.set(Mode::Status(503));
    let pool = HttpPool::builder()
        .with_ejection(Ejection::new(2, 30 * SECOND, 300 * SECOND))
        .build()
        .unwrap();
    let one = ClientBuilder::new(replica.url())
        .with_pool(&pool)
        .build()
        .unwrap();
    let two = ClientBuilder::new(replica.url())
        .with_pool(&pool)
        .build()
        .unwrap();
    for _ in 0..2 {
        get(&one).await.expect_err("503");
    }
    let err = get(&two).await.expect_err("open");
    assert!(matches!(err, ClientError::CircuitOpen { .. }), "{err:?}");
    assert!(err.is_retryable());
    assert_eq!(replica.requests(), 2);
    let snap = pool.origins();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].origin, format!("http://{}", replica.addr));
    assert_eq!(snap[0].consecutive_ejections, 1);
    assert!(snap[0].ejected_for.is_some_and(|d| d <= 30 * SECOND));

    // After the ejection, one probe; it succeeds and the circuit closes.
    replica.set(Mode::Ok);
    super::harness::elapse(30 * SECOND).await;
    assert_eq!(get(&two).await.expect("probe"), "a");
    assert_eq!(get(&one).await.expect("closed"), "a");
    assert_eq!(pool.origins()[0].ejected_for, None);
}

/// A pool without per-origin policies changes nothing for a client.
#[tokio::test]
async fn a_plain_pool_has_no_breaker() {
    let replica = Replica::start("a").await;
    replica.set(Mode::Status(503));
    let pool = HttpPool::builder().build().unwrap();
    let client = ClientBuilder::new(replica.url())
        .with_pool(&pool)
        .build()
        .unwrap();
    for _ in 0..10 {
        get(&client).await.expect_err("503");
    }
    assert_eq!(replica.requests(), 10);
    assert!(pool.origins().is_empty());
}

/// The cap counts every client's calls to the origin.
#[tokio::test]
async fn the_in_flight_cap_spans_clients() {
    let replica = Replica::start("a").await;
    replica.set(Mode::Stall);
    let pool = HttpPool::builder()
        .with_max_in_flight_per_origin(1)
        .build()
        .unwrap();
    let one = Arc::new(
        ClientBuilder::new(replica.url())
            .with_pool(&pool)
            .build()
            .unwrap(),
    );
    let two = ClientBuilder::new(replica.url())
        .with_pool(&pool)
        .build()
        .unwrap();
    let held = {
        let one = Arc::clone(&one);
        tokio::spawn(async move { get(&one).await })
    };
    while replica.requests() == 0 {
        tokio::task::yield_now().await;
    }
    let err = get(&two).await.expect_err("capped");
    assert!(
        matches!(err, ClientError::TooManyPendingRequests { limit: 1 }),
        "{err:?}"
    );
    assert_eq!(pool.origins()[0].in_flight, 1);
    held.abort();
    let _ = held.await;
    assert_eq!(pool.origins()[0].in_flight, 0);
}

/// The pool's retry budget is one per origin: with a reserve of one and
/// nothing earned, the first client's retry spends it and the second's is not
/// made.
#[tokio::test]
async fn the_retry_budget_spans_clients() {
    let replica = Replica::start("a").await;
    replica.set(Mode::Status(503));
    let pool = HttpPool::builder()
        .with_retry_budget(RetryBudget::new(0.0, 0.0).with_reserve(1.0))
        .build()
        .unwrap();
    let policy = RetryPolicy::default()
        .with_max_retries(3)
        .with_initial_backoff(Duration::from_millis(1))
        .with_max_backoff(Duration::from_millis(1));
    let one = ClientBuilder::new(replica.url())
        .with_pool(&pool)
        .with_retry_policy(policy.clone())
        .build()
        .unwrap();
    let two = ClientBuilder::new(replica.url())
        .with_pool(&pool)
        .with_retry_policy(policy)
        .build()
        .unwrap();
    get(&one).await.expect_err("503");
    assert_eq!(
        replica.requests(),
        2,
        "one attempt and the one retry paid for"
    );
    get(&two).await.expect_err("503");
    assert_eq!(replica.requests(), 3, "no retry left");
}

/// A balanced client on a pool uses the pool's ejection; choosing another is
/// a conflict, refused at `build()`.
#[tokio::test]
async fn a_balanced_client_on_a_pool_takes_the_pools_ejection() {
    let replicas = vec![Replica::start("a").await, Replica::start("b").await];
    let pool = HttpPool::builder()
        .with_ejection(Ejection::new(1, 30 * SECOND, 30 * SECOND))
        .build()
        .unwrap();
    let err = ClientBuilder::new(replicas[0].url())
        .with_pool(&pool)
        .with_balancing(
            StaticEndpoints::new(urls(&replicas)),
            BalancePolicy::new().with_ejection(Ejection::default()),
        )
        .build()
        .expect_err("conflict");
    assert!(
        matches!(&err, ClientError::Config(m) if m.contains("with_ejection")),
        "{err:?}"
    );
    replicas[0].set(Mode::Status(503));
    let client = ClientBuilder::new(replicas[0].url())
        .with_pool(&pool)
        .with_balancing(StaticEndpoints::new(urls(&replicas)), BalancePolicy::new())
        .build()
        .unwrap();
    get(&client).await.expect_err("a fails once and is out");
    for _ in 0..3 {
        assert_eq!(get(&client).await.unwrap(), "b");
    }
    assert_eq!(
        pool.origins()
            .iter()
            .filter(|o| o.ejected_for.is_some())
            .count(),
        1
    );
}

#[test]
fn the_pool_refuses_a_cap_of_zero() {
    let err = HttpPool::builder()
        .with_max_in_flight_per_origin(0)
        .build()
        .expect_err("zero");
    assert!(matches!(&err, ClientError::Config(m) if m.contains("with_max_in_flight_per_origin")));
}
