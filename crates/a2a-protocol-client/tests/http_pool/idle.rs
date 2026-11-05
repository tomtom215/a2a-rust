// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An idle connection is closed when the idle timeout passes, without
//! waiting for the next request to find it stale, and a pool can cap how
//! many idle connections it keeps per host (audit D-12, B-07).
//!
//! hyper-util closes an idle connection in the background only when the pool
//! has a timer; a client built without one kept the socket until it was next
//! asked for.

use std::sync::atomic::Ordering;
use std::time::Duration;

use a2a_protocol_client::HttpPool;

use crate::harness::{Tap, call, client, gate, job, serve, until};

#[tokio::test]
async fn an_idle_connection_is_closed_in_the_background_after_the_idle_timeout() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder()
        .with_idle_timeout(Duration::from_millis(200))
        .build()
        .expect("pool");
    let client = client(&tap.url(), Some(&pool));

    call(&client).await;
    assert_eq!(
        tap.closed(),
        0,
        "the connection is open right after the call"
    );

    // No further request: only the idle sweeper can close it.
    until("the idle connection was closed", || tap.closed() == 1).await;
    assert_eq!(tap.accepted(), 1);
}

/// Three calls held open together, then released, over HTTP/1.1, so that
/// three connections exist and all return to the pool idle at once.
///
/// `tag` makes the message texts, and so the gates they wait on, this test's
/// own: the gates are process-wide, and two tests that shared a text would
/// release and wait on each other's calls.
async fn three_connections_then_idle(tap: &Tap, pool: &HttpPool, tag: &str) {
    let keys = ["a", "b", "c"].map(|n| format!("hold:{tag}-{n}"));
    let calls: Vec<_> = keys
        .iter()
        .map(|key| {
            let client = client(&tap.url(), Some(pool));
            let text = key.clone();
            tokio::spawn(async move { client.send_message(job(&text)).await })
        })
        .collect();
    for key in &keys {
        let gate = gate(key);
        until("the held call reached the agent", || {
            gate.started.load(Ordering::SeqCst)
        })
        .await;
    }
    assert_eq!(tap.accepted(), 3, "three calls at once, three connections");
    for key in &keys {
        gate(key).open.notify_one();
    }
    for handle in calls {
        handle.await.expect("ran").expect("the call");
    }
}

#[tokio::test]
async fn a_pool_keeps_no_more_idle_connections_per_host_than_it_is_told_to() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder()
        .with_max_idle_per_host(1)
        .build()
        .expect("pool");

    three_connections_then_idle(&tap, &pool, "limit").await;

    until("two of the three idle connections were closed", || {
        tap.closed() == 2
    })
    .await;
    // And not the third: one idle connection is what was asked for.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(tap.closed(), 2);
}

#[tokio::test]
async fn without_a_limit_every_idle_connection_is_kept() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder().build().expect("pool");

    three_connections_then_idle(&tap, &pool, "nolimit").await;

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(tap.closed(), 0);
}
