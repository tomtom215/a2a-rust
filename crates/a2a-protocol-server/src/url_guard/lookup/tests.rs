// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for the bounded lookup: at most the limit's worth of blocking
//! threads, and a permit that outlives a caller that stopped waiting.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use super::*;

fn one_address() -> Vec<SocketAddr> {
    vec!["203.0.113.1:80".parse().expect("address")]
}

/// Six lookups that each hold a thread for 40 ms, against a limit of two:
/// never more than two run at once, and all six finish.
#[tokio::test]
async fn no_more_lookups_run_than_the_limit_allows() {
    let limit = Arc::new(Semaphore::new(2));
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));

    let mut tasks = Vec::new();
    for _ in 0..6 {
        let (limit, running, most) = (Arc::clone(&limit), Arc::clone(&running), Arc::clone(&most));
        tasks.push(tokio::spawn(async move {
            run_bounded(&limit, move || {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                most.fetch_max(now, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(one_address())
            })
            .await
        }));
    }
    for task in tasks {
        task.await.expect("task").expect("lookup");
    }
    assert_eq!(
        most.load(Ordering::SeqCst),
        2,
        "two ran together, never three"
    );
    assert_eq!(limit.available_permits(), 2, "every permit came back");
}

/// The audit's scenario: the caller's timeout fires and it stops waiting, but
/// the thread is still inside the resolver. The permit must still be taken
/// until it returns, or a stream of abandoned lookups would hold unbounded
/// threads.
#[tokio::test]
async fn an_abandoned_lookup_keeps_its_permit_until_the_thread_returns() {
    let limit = Arc::new(Semaphore::new(1));
    let (release, blocked) = mpsc::channel::<()>();
    let (started_tx, started_rx) = mpsc::channel::<()>();

    let slow = run_bounded(&limit, move || {
        started_tx.send(()).expect("started");
        let _ = blocked.recv();
        Ok(one_address())
    });
    let abandoned = tokio::time::timeout(Duration::from_millis(100), slow).await;
    assert!(abandoned.is_err(), "the caller gave up");
    started_rx.recv().expect("the thread did start");
    assert_eq!(
        limit.available_permits(),
        0,
        "the thread is still in the resolver, so the permit is still out"
    );

    // A second lookup waits for the permit inside its own timeout and gives up.
    let second = tokio::time::timeout(
        Duration::from_millis(100),
        run_bounded(&limit, || Ok(one_address())),
    )
    .await;
    assert!(second.is_err(), "no permit, so it waited");

    release.send(()).expect("release the thread");
    let mut waited = 0;
    while limit.available_permits() == 0 {
        assert!(waited < 100, "the permit never came back");
        tokio::time::sleep(Duration::from_millis(20)).await;
        waited += 1;
    }
    run_bounded(&limit, || Ok(one_address()))
        .await
        .expect("a lookup runs once the thread has returned");
}

/// A lookup's own error comes back as it is, not as a bound being hit.
#[tokio::test]
async fn a_lookup_error_is_passed_through() {
    let limit = Arc::new(Semaphore::new(1));
    let err = run_bounded(&limit, || {
        Err(io::Error::new(io::ErrorKind::NotFound, "no such host"))
    })
    .await
    .expect_err("fails");
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(limit.available_permits(), 1);
}

/// The real lookup, which needs no network for `localhost`.
#[tokio::test]
async fn lookup_host_resolves_localhost() {
    let addrs = lookup_host("localhost:8080".to_owned())
        .await
        .expect("resolves");
    assert_ne!(addrs, Vec::<SocketAddr>::new());
    assert!(
        addrs
            .iter()
            .all(|a| a.ip().is_loopback() && a.port() == 8080)
    );
}
