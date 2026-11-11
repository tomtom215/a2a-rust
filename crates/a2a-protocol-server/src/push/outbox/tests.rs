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

/// Jitter is there to spread retries: 256 draws are not one number, and not a
/// short list of them (a fresh random UUID gives a 16-bit draw).
#[test]
fn jitter_spreads_retries_and_is_not_one_number() {
    let draws: std::collections::HashSet<u32> = (0..256).map(|_| jitter()).collect();
    assert!(draws.len() > 100, "{} distinct draws of 256", draws.len());
    assert!(draws.iter().all(|d| *d < JITTER_ONE));
}

#[test]
fn the_in_memory_outbox_shows_what_waits() {
    let outbox = InMemoryPushOutbox::new();
    assert_eq!(format!("{outbox:?}"), "InMemoryPushOutbox { pending: 0 }");
}

/// An outbox that overrides only what is required keeps no reservations and
/// does not renew leases, and says so: a worker given one records an event's
/// rows after the event is stored, and takes the long lease. What it is asked
/// to do beyond that is refused, not silently done.
#[tokio::test]
async fn a_plain_outbox_keeps_no_reservations_and_does_not_renew() {
    struct Plain(InMemoryPushOutbox);
    impl PushOutbox for Plain {
        fn record(&self, d: Vec<NewDelivery>, cap: usize) -> OutboxFuture<'_, usize> {
            self.0.record(d, cap)
        }
        fn claim<'a>(
            &'a self,
            owner: &'a str,
            max: usize,
            lease: Duration,
        ) -> OutboxFuture<'a, Vec<ClaimedDelivery>> {
            self.0.claim(owner, max, lease)
        }
        fn complete(&self, seq: i64) -> OutboxFuture<'_, ()> {
            self.0.complete(seq)
        }
        fn retry_after<'a>(
            &'a self,
            seq: i64,
            owner: &'a str,
            after: Duration,
        ) -> OutboxFuture<'a, ()> {
            self.0.retry_after(seq, owner, after)
        }
        fn pending(&self) -> OutboxFuture<'_, usize> {
            self.0.pending()
        }
    }
    let outbox = Plain(InMemoryPushOutbox::new());
    assert!(!outbox.supports_reservations());
    assert!(!outbox.supports_renewal());
    let unsupported = |what: &str, refused: bool| assert!(refused, "{what} is refused");
    unsupported(
        "reserve",
        outbox
            .reserve(
                "r",
                1,
                vec![delivery("", "a", TaskState::Working)],
                10,
                Duration::ZERO,
            )
            .await
            .is_err(),
    );
    unsupported("arm", outbox.arm("r").await.is_err());
    unsupported("release", outbox.release("r").await.is_err());
    unsupported("held", outbox.held(10).await.is_err());
    unsupported(
        "held_for_task",
        outbox.held_for_task("", None, "t").await.is_err(),
    );
    unsupported(
        "renew",
        outbox.renew("me", Duration::from_secs(1)).await.is_err(),
    );
}
