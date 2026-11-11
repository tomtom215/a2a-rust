// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the worker does with many stale reservations, and with a task store it cannot read (ADR 0037, audit H14); the rig and helpers are `processor_reserve_tests`'s.

use super::*;

/// A replica lost with many tasks in flight leaves as many reservations; one
/// look of the worker settles them all, not a batch of 32 every ten seconds.
#[tokio::test(start_paused = true)]
async fn four_hundred_stale_reservations_are_settled_in_one_look() {
    let rig = Rig::new(TaskState::Working).await;
    for n in 0..400u64 {
        let delivery = crate::push::outbox::NewDelivery {
            delivery_id: format!("d{n}"),
            tenant: String::new(),
            caller: None,
            task_id: "t".to_owned(),
            config_id: format!("c{n}"),
            event: status(n + 1, TaskState::Working).event,
            ends_turn: false,
        };
        // Held already; positions past what the task reflects: not stored.
        rig.outbox
            .reserve(
                &format!("r{n}"),
                n + 1,
                vec![delivery],
                1024,
                Duration::ZERO,
            )
            .await
            .unwrap();
    }
    assert_eq!(rig.rows().await, 400);
    // One look (ten seconds apart), not several: two workers settle 32 a round,
    // at the start and again ten seconds on, which is 128 of the 400.
    rig.wait(Duration::from_secs(12)).await;
    assert_eq!(rig.rows().await, 0, "all released in the first look");
    // Counted by each replica that settled one: two workers can settle the same
    // reservation at once (the removal is idempotent), so at least 400.
    assert!(rig.metrics.count(push_outcome::RESERVATION_SETTLED) >= 400);
}

/// Reservations that cannot be settled because the task store cannot be read
/// stay for the next look, and the look ends: a round that settled nothing is
/// not repeated.
#[tokio::test(start_paused = true)]
async fn an_unreadable_task_store_leaves_the_reservations_and_does_not_spin() {
    let rig = Rig::new(TaskState::Working).await;
    rig.store
        .unreadable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    for n in 0..40u64 {
        let delivery = crate::push::outbox::NewDelivery {
            delivery_id: format!("d{n}"),
            tenant: String::new(),
            caller: None,
            task_id: "t".to_owned(),
            config_id: format!("c{n}"),
            event: status(n + 1, TaskState::Working).event,
            ends_turn: false,
        };
        rig.outbox
            .reserve(
                &format!("r{n}"),
                n + 1,
                vec![delivery],
                1024,
                Duration::ZERO,
            )
            .await
            .unwrap();
    }
    rig.wait(Duration::from_secs(35)).await;
    assert_eq!(rig.rows().await, 40, "left for when the store can be read");
    let reads = rig.store.reads.load(std::sync::atomic::Ordering::SeqCst);
    // Two replicas, a look every ten seconds from the start (four in this
    // time), one round of 32 each: a round that settled nothing is not repeated.
    assert!(
        reads <= 2 * 4 * 32,
        "{reads} reads: a round that settled nothing is repeated"
    );
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);

    // Readable again: settled at the next look.
    rig.store
        .unreadable
        .store(false, std::sync::atomic::Ordering::SeqCst);
    rig.wait(Duration::from_secs(12)).await;
    assert_eq!(rig.rows().await, 0);
}
