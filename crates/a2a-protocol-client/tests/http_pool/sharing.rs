// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Clients given one pool use each other's connections; clients without one
//! do not (ADR 0032: "a counting acceptor sees one TCP connection for N
//! clients, not N").

use std::time::Duration;

use a2a_protocol_client::HttpPool;

use crate::harness::{Tap, call, client, serve, until};

const CLIENTS: usize = 10;

/// `CLIENTS` calls at the same moment, one client each.
async fn concurrently(url: &str, pool: Option<&HttpPool>) {
    let calls: Vec<_> = (0..CLIENTS)
        .map(|_| {
            let client = client(url, pool);
            tokio::spawn(async move { call(&client).await })
        })
        .collect();
    for handle in calls {
        handle.await.expect("the call finished");
    }
}

#[tokio::test]
async fn clients_on_one_pool_share_one_connection() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder()
        .with_http2_prior_knowledge()
        .build()
        .expect("pool");

    concurrently(&tap.url(), Some(&pool)).await;

    assert_eq!(tap.accepted(), 1, "ten clients, one pool, one connection");
}

#[tokio::test]
async fn clients_without_a_pool_each_open_their_own_connection() {
    let tap = Tap::start(serve().await).await;

    concurrently(&tap.url(), None).await;

    assert_eq!(tap.accepted(), CLIENTS, "ten clients, ten pools");
}

#[tokio::test]
async fn a_second_client_reuses_the_idle_connection_the_first_left() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder().build().expect("pool");
    let (first, second) = (
        client(&tap.url(), Some(&pool)),
        client(&tap.url(), Some(&pool)),
    );

    call(&first).await;
    // The connection goes back to the pool when the response is read; give
    // the task that returns it the moment it needs.
    tokio::time::sleep(Duration::from_millis(100)).await;
    call(&second).await;

    assert_eq!(tap.accepted(), 1, "HTTP/1.1 keep-alive, across two clients");
}

#[tokio::test]
async fn two_pools_do_not_share() {
    let tap = Tap::start(serve().await).await;
    let (one, other) = (
        HttpPool::builder()
            .with_http2_prior_knowledge()
            .build()
            .expect("pool"),
        HttpPool::builder()
            .with_http2_prior_knowledge()
            .build()
            .expect("pool"),
    );
    let (first, second) = (
        client(&tap.url(), Some(&one)),
        client(&tap.url(), Some(&other)),
    );

    call(&first).await;
    call(&second).await;
    until("both connections reached the tap", || tap.accepted() == 2).await;

    assert_eq!(tap.accepted(), 2);
}

#[tokio::test]
async fn two_clients_that_ask_for_the_shared_pool_share_its_connection() {
    let tap = Tap::start(serve().await).await;
    // Every test in this process may use the process-wide pool, but each has
    // a server of its own, and a pool keeps connections per host and port,
    // so what this server's tap counts is this test's alone.
    let (first, second) = (
        client(&tap.url(), Some(&HttpPool::shared())),
        client(&tap.url(), Some(&HttpPool::shared())),
    );

    call(&first).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    call(&second).await;

    assert_eq!(tap.accepted(), 1);
}
