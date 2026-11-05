// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Semaphore;

use super::SingleFlight;
use crate::error::ClientError;

/// An operation that counts its runs and finishes only when `gate` gives it
/// a permit, so callers can pile up behind the first run.
struct Op {
    runs: AtomicUsize,
    gate: Semaphore,
}

impl Op {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            runs: AtomicUsize::new(0),
            gate: Semaphore::new(0),
        })
    }

    async fn ok(&self) -> Result<u32, ClientError> {
        let run = self.runs.fetch_add(1, Ordering::SeqCst);
        self.gate.acquire().await.expect("open").forget();
        Ok(u32::try_from(run).expect("small") + 7)
    }

    async fn fail(&self) -> Result<u32, ClientError> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        self.gate.acquire().await.expect("open").forget();
        Err(ClientError::HttpClient("refused".into()))
    }
}

/// Yields until `n` callers wait on the run in flight.
async fn until_waiting(flight: &SingleFlight<u32>, n: usize) {
    while flight.waiters() < n {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
async fn concurrent_callers_share_one_run_and_its_value() {
    let flight = Arc::new(SingleFlight::<u32>::default());
    let op = Op::new();
    let mut calls = Vec::new();
    for _ in 0..8 {
        let (flight, op) = (Arc::clone(&flight), Arc::clone(&op));
        calls.push(tokio::spawn(async move { flight.run(|| op.ok()).await }));
    }
    until_waiting(&flight, 7).await;
    op.gate.add_permits(1);
    for call in calls {
        assert_eq!(call.await.expect("join").expect("value"), 7);
    }
    assert_eq!(op.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_failure_reaches_every_waiter_once() {
    let flight = Arc::new(SingleFlight::<u32>::default());
    let op = Op::new();
    let mut calls = Vec::new();
    for _ in 0..5 {
        let (flight, op) = (Arc::clone(&flight), Arc::clone(&op));
        calls.push(tokio::spawn(async move { flight.run(|| op.fail()).await }));
    }
    until_waiting(&flight, 4).await;
    op.gate.add_permits(1);
    for call in calls {
        let err = call.await.expect("join").expect_err("shared failure");
        assert!(
            matches!(&err, ClientError::HttpClient(m) if m == "refused"),
            "{err:?}"
        );
    }
    assert_eq!(op.runs.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_cancelled_leader_hands_the_run_to_a_waiter() {
    let flight = Arc::new(SingleFlight::<u32>::default());
    let op = Op::new();
    let leader = {
        let (flight, op) = (Arc::clone(&flight), Arc::clone(&op));
        tokio::spawn(async move { flight.run(|| op.ok()).await })
    };
    while op.runs.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    let waiter = {
        let (flight, op) = (Arc::clone(&flight), Arc::clone(&op));
        tokio::spawn(async move { flight.run(|| op.ok()).await })
    };
    until_waiting(&flight, 1).await;
    leader.abort();
    assert!(leader.await.expect_err("aborted").is_cancelled());
    // The waiter leads the second run, which the gate now lets finish.
    while op.runs.load(Ordering::SeqCst) < 2 {
        tokio::task::yield_now().await;
    }
    op.gate.add_permits(1);
    assert_eq!(waiter.await.expect("join").expect("value"), 8);
}

#[tokio::test]
async fn a_finished_run_is_not_remembered() {
    let flight = SingleFlight::<u32>::default();
    let op = Op::new();
    op.gate.add_permits(2);
    assert_eq!(flight.run(|| op.ok()).await.expect("first"), 7);
    assert_eq!(flight.run(|| op.ok()).await.expect("second"), 8);
    assert_eq!(op.runs.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn debug_says_whether_a_run_is_in_flight() {
    let flight = Arc::new(SingleFlight::<u32>::default());
    assert_eq!(format!("{flight:?}"), "SingleFlight { running: false }");
    let op = Op::new();
    let call = {
        let (flight, op) = (Arc::clone(&flight), Arc::clone(&op));
        tokio::spawn(async move { flight.run(|| op.ok()).await })
    };
    while op.runs.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    assert_eq!(format!("{flight:?}"), "SingleFlight { running: true }");
    op.gate.add_permits(1);
    call.await.expect("join").expect("value");
}
