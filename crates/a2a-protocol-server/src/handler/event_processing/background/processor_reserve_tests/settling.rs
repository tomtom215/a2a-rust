// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What the worker does with many stale reservations, with a task store it cannot read, and with a log that disagrees with the task about a reserved position (ADR 0037, audit H14); the rig and helpers are `processor_reserve_tests`'s.

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

/// The task reflects a reserved position, but the log holds a different event
/// there: another writer took the position after the replica that reserved it
/// stopped before logging, so the reserved event was never stored and no
/// webhook hears of it. The event that was stored is delivered, once.
#[tokio::test(start_paused = true)]
async fn a_different_event_at_the_reserved_position_is_released_not_armed() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.park_next_append();
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;
    rig.kill_a(running).await;
    let id = TaskId::new("t");
    assert!(
        rig.store
            .inner
            .read_events(&id, 0, 10)
            .await
            .unwrap()
            .is_empty(),
        "replica a stopped before its event reached the log"
    );

    // Replica b, which now runs the task, stores another event at position 1.
    rig.run_on_b(artifact(1)).await.unwrap();

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: None
        },
        "the artifact the task stored"
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(
        rig.b_seen.try_recv().is_err(),
        "the status update the task never stored was not delivered"
    );
    assert!(rig.a_seen.try_recv().is_err());
    assert_eq!(rig.rows().await, 0);
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 1);
}

/// The task reflects a reserved position and the log has no entry for it
/// (retention trimmed it), though it still has a later one: the event was
/// stored, and a later entry is not mistaken for it. Delivered, then the later
/// event, in order.
#[tokio::test(start_paused = true)]
async fn a_trimmed_log_entry_does_not_hide_a_stored_event() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;
    rig.kill_a(running).await;

    // The task goes on, on replica b, to position 2; retention then trims the
    // log below it, and with it the entry for position 1.
    rig.run_on_b(artifact(2)).await.unwrap();
    let id = TaskId::new("t");
    assert_eq!(rig.store.inner.trim_event_log(&id, 2).await.unwrap(), 1);
    let left = rig.store.inner.read_events(&id, 0, 10).await.unwrap();
    assert_eq!(
        left.iter().map(|recorded| recorded.seq).collect::<Vec<_>>(),
        [2],
        "only a later entry is left"
    );

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: Some(TaskState::Working)
        },
        "the stored status update, delivered"
    );
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: None
        },
        "then the artifact that follows it"
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.b_seen.try_recv().is_err(), "each once");
    assert_eq!(rig.rows().await, 0);
}

/// A batch that comes back short ends the look: the worker does not ask again
/// for what it has just been told is all there is. Each replica looks once in
/// the first ten seconds, so the outbox is asked once by each.
#[tokio::test(start_paused = true)]
async fn a_short_batch_ends_the_look_without_asking_again() {
    let rig = Rig::new(TaskState::Working).await;
    for n in 0..5u64 {
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
    rig.wait(Duration::from_secs(5)).await;
    assert_eq!(rig.rows().await, 0, "all five were settled");
    assert_eq!(
        rig.flaky
            .held_calls
            .load(std::sync::atomic::Ordering::SeqCst),
        2,
        "one look each for two replicas, one question per look"
    );
}
