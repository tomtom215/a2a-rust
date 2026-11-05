// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A pool sends HTTP/2 PINGs on the HTTP bindings only when asked to, and
//! drops a connection whose PINGs go unanswered (audit D-12, ADR 0032).
//!
//! The tap reads the client's byte stream as HTTP/2 frames. The clients
//! speak h2c, so that the frames are in the clear.

use std::time::Duration;

use a2a_protocol_client::HttpPool;

use crate::harness::{Tap, call, client, serve, until};

fn quiet_pool() -> HttpPool {
    HttpPool::builder()
        .with_http2_prior_knowledge()
        .build()
        .expect("pool")
}

#[tokio::test]
async fn by_default_a_quiet_connection_carries_no_pings() {
    let tap = Tap::start(serve().await).await;
    let client = client(&tap.url(), Some(&quiet_pool()));

    call(&client).await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    assert_eq!(tap.accepted(), 1);
    assert_eq!(tap.pings(), 0);
}

#[tokio::test]
async fn a_pool_asked_to_keep_alive_pings_an_idle_connection() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder()
        .with_http2_prior_knowledge()
        .with_http2_keep_alive(Duration::from_millis(100), Duration::from_secs(5))
        .build()
        .expect("pool");
    let client = client(&tap.url(), Some(&pool));

    call(&client).await;
    tokio::time::sleep(Duration::from_millis(800)).await;

    let pings = tap.pings();
    assert!(pings >= 4, "{pings} PINGs in 800 ms at one every 100 ms");
    assert_eq!(tap.closed(), 0, "answered PINGs keep the connection open");
}

#[tokio::test]
async fn a_connection_whose_pings_go_unanswered_is_dropped_and_the_next_call_reconnects() {
    let tap = Tap::start(serve().await).await;
    let pool = HttpPool::builder()
        .with_http2_prior_knowledge()
        .with_http2_keep_alive(Duration::from_millis(100), Duration::from_millis(300))
        .build()
        .expect("pool");
    let client = client(&tap.url(), Some(&pool));

    call(&client).await;
    assert_eq!(tap.closed(), 0);

    // The peer vanishes: what the server sends no longer arrives.
    tap.blackhole(true);
    until("the unanswered PING closed the connection", || {
        tap.closed() == 1
    })
    .await;

    // The network comes back; the pool dials again instead of reusing the
    // dead connection.
    tap.blackhole(false);
    call(&client).await;
    assert_eq!(tap.accepted(), 2);
}
