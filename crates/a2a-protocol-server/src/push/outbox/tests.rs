// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The in-memory outbox against the contract every store keeps, its clock on
//! a paused runtime, and the backoff.

use std::time::Duration;

use a2a_protocol_types::task::TaskState;

use super::contract::{self, delivery};
use super::*;

#[tokio::test]
async fn the_in_memory_outbox_keeps_the_contract() {
    contract::run_all(|| async { InMemoryPushOutbox::new() }).await;
}

/// A lease runs for exactly its length: claimed again at 10 s, not before.
#[tokio::test(start_paused = true)]
async fn a_lease_runs_for_exactly_its_length() {
    let outbox = InMemoryPushOutbox::new();
    outbox
        .record(vec![delivery("", "a", TaskState::Working)], 10)
        .await
        .unwrap();
    outbox
        .claim("me", 10, Duration::from_secs(10))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_millis(9_999)).await;
    assert!(
        outbox
            .claim("next", 10, Duration::from_secs(10))
            .await
            .unwrap()
            .is_empty()
    );
    tokio::time::advance(Duration::from_millis(1)).await;
    assert_eq!(
        outbox
            .claim("next", 10, Duration::from_secs(10))
            .await
            .unwrap()
            .len(),
        1
    );
}

/// A released row is due exactly its backoff later, and its age counts from
/// when it was recorded.
#[tokio::test(start_paused = true)]
async fn a_released_row_is_due_exactly_its_backoff_later() {
    let outbox = InMemoryPushOutbox::new();
    outbox
        .record(vec![delivery("", "a", TaskState::Working)], 10)
        .await
        .unwrap();
    let seq = outbox
        .claim("me", 10, Duration::from_secs(60))
        .await
        .unwrap()[0]
        .seq;
    outbox
        .retry_after(seq, "me", Duration::from_secs(4))
        .await
        .unwrap();
    tokio::time::advance(Duration::from_millis(3_999)).await;
    assert!(
        outbox
            .claim("me", 10, Duration::from_secs(60))
            .await
            .unwrap()
            .is_empty()
    );
    tokio::time::advance(Duration::from_millis(1)).await;
    let again = outbox
        .claim("me", 10, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(again[0].attempts, 1);
    assert_eq!(again[0].age, Duration::from_secs(4));
}

/// 1 s, doubled per failed attempt, to the bound; jitter takes off up to a
/// fifth.
#[test]
fn the_backoff_doubles_from_a_second_to_its_bound() {
    let hour = Duration::from_secs(3600);
    assert_eq!(backoff(0, hour, 0), Duration::from_secs(1));
    assert_eq!(backoff(1, hour, 0), Duration::from_secs(1));
    assert_eq!(backoff(2, hour, 0), Duration::from_secs(2));
    assert_eq!(backoff(3, hour, 0), Duration::from_secs(4));
    assert_eq!(backoff(12, hour, 0), Duration::from_secs(2048));
    assert_eq!(backoff(13, hour, 0), hour, "4096 s is past the bound");
    assert_eq!(backoff(40, hour, 0), hour, "no overflow");
    assert_eq!(
        backoff(3, hour, JITTER_ONE / 2),
        Duration::from_millis(3_600)
    );
    assert_eq!(backoff(3, hour, JITTER_ONE), Duration::from_millis(3_200));
    assert_eq!(
        backoff(3, hour, u32::MAX),
        Duration::from_millis(3_200),
        "at most a fifth"
    );
}

#[test]
fn jitter_is_a_fraction() {
    for _ in 0..1000 {
        assert!(jitter() < JITTER_ONE);
    }
}

#[test]
fn the_in_memory_outbox_shows_what_waits() {
    let outbox = InMemoryPushOutbox::new();
    assert_eq!(format!("{outbox:?}"), "InMemoryPushOutbox { pending: 0 }");
}
