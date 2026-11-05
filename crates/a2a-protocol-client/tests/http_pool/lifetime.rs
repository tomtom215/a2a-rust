// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! A pool with a maximum connection lifetime starts using new connections
//! once the current ones are that old, and cuts no call to do it (audit
//! D-12, ADR 0032).
//!
//! A lifetime of 100 s is spread by 10% either way, so a connection is
//! replaced at some point between 90 s and 110 s of its life. The cases pick
//! times outside that band.

use std::sync::atomic::Ordering;
use std::time::Duration;

use a2a_protocol_client::HttpPool;

use crate::harness::{Tap, call, client, elapse, gate, job, serve, until};

const LIFETIME: Duration = Duration::from_secs(100);

/// A pool whose connections are replaced after `LIFETIME`, over HTTP/2 so
/// that reuse does not depend on when a connection is handed back. The idle
/// timeout is long enough that only the lifetime ends a connection here.
fn pool() -> HttpPool {
    HttpPool::builder()
        .with_http2_prior_knowledge()
        .with_idle_timeout(Duration::from_secs(86_400))
        .with_max_connection_lifetime(LIFETIME)
        .build()
        .expect("pool")
}

#[tokio::test]
async fn a_connection_within_its_lifetime_is_reused() {
    let tap = Tap::start(serve().await).await;
    let client = client(&tap.url(), Some(&pool()));

    call(&client).await;
    elapse(Duration::from_secs(89)).await;
    call(&client).await;

    assert_eq!(
        tap.accepted(),
        1,
        "89 s is inside the earliest expiry, 90 s"
    );
}

#[tokio::test]
async fn a_connection_past_its_lifetime_is_replaced() {
    let tap = Tap::start(serve().await).await;
    let client = client(&tap.url(), Some(&pool()));

    call(&client).await;
    elapse(Duration::from_secs(111)).await;
    call(&client).await;

    assert_eq!(tap.accepted(), 2, "111 s is past the latest expiry, 110 s");
}

#[tokio::test]
async fn every_client_on_the_pool_moves_to_the_new_connection_together() {
    let tap = Tap::start(serve().await).await;
    let pool = pool();
    let (first, second) = (
        client(&tap.url(), Some(&pool)),
        client(&tap.url(), Some(&pool)),
    );

    call(&first).await;
    elapse(Duration::from_secs(111)).await;
    call(&first).await;
    call(&second).await;

    assert_eq!(tap.accepted(), 2, "the second client joins the new one");
}

#[tokio::test]
async fn a_call_in_flight_when_its_connection_is_replaced_is_not_cut() {
    let tap = Tap::start(serve().await).await;
    let pool = pool();
    let slow = client(&tap.url(), Some(&pool));
    let quick = client(&tap.url(), Some(&pool));

    let held = tokio::spawn(async move { slow.send_message(job("hold:lifetime")).await });
    let gate = gate("hold:lifetime");
    until("the held call reached the agent", || {
        gate.started.load(Ordering::SeqCst)
    })
    .await;

    elapse(Duration::from_secs(111)).await;
    call(&quick).await;
    assert_eq!(tap.accepted(), 2, "the new call went to a new connection");

    gate.open.notify_one();
    held.await
        .expect("the task ran")
        .expect("the call that straddled the replacement was not cut");
}
