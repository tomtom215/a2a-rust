// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The follow-ups to the push reservation (ADR 0037, audit H14): events kept in
//! order while the outbox is down, a stable delivery id across a settled and
//! recorded-again reservation, a task's reservations settled at once by the one
//! that holds its claim, and an executor panic's ending reserved before it is
//! saved. The rig and helpers are `processor_reserve_tests`'s.

use super::*;

/// The outbox is down when an event is reserved: that event is recorded in its
/// place (awaited, in the processor) and not queued in the lane behind the next
/// event's reservation, so the webhook sees them in the order they were stored.
#[tokio::test(start_paused = true)]
async fn events_stay_in_order_across_an_outbox_that_was_down_at_reserve() {
    let mut rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    let mut run = Processor::new(
        &id,
        BackgroundDeps {
            task_store: &*rig.store,
            push: &rig.a,
            limits: &rig.limits,
            metrics: &crate::metrics::NoopMetrics,
        },
        task(TaskState::Working),
        CancellationToken::new(),
        None,
        None,
    );
    rig.flaky
        .fail_reserve
        .store(true, std::sync::atomic::Ordering::SeqCst);
    run.handle(Ok(status(1, TaskState::Working))).await;
    rig.flaky
        .fail_reserve
        .store(false, std::sync::atomic::Ordering::SeqCst);
    run.handle(Ok(status(2, TaskState::Completed))).await;

    for state in [TaskState::Working, TaskState::Completed] {
        assert_eq!(
            next(&mut rig.a_seen).await,
            Seen {
                replica: "a",
                state: Some(state)
            }
        );
    }
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "once each");
    assert_eq!(rig.rows().await, 0);
}

/// A processor stalled past the hold whose event the worker had meanwhile
/// armed (the write had landed) and had delivered finds nothing to arm, and
/// records the event again: under the delivery id it had, so the receiver sees
/// one id twice, which it knows for a retry.
#[tokio::test(start_paused = true)]
async fn an_event_recorded_again_keeps_its_delivery_id() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::AfterThenWait);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;

    // The hold runs out, the stored task shows the event, and the worker
    // arms the rows and delivers them.
    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    let first = tokio::time::timeout(Duration::from_mins(15), async {
        tokio::select! {
            seen = rig.a_seen.recv() => seen.unwrap(),
            seen = rig.b_seen.recv() => seen.unwrap(),
        }
    })
    .await
    .expect("delivered by the worker");
    assert_eq!(first.state, Some(TaskState::Working));
    assert_eq!(rig.rows().await, 0, "delivered and gone");

    // The processor comes back: its rows are gone, so it records again.
    rig.store.resume.notify_one();
    running.await.unwrap();
    rig.wait(Duration::from_secs(30)).await;
    let ids = rig.delivery_ids();
    assert_eq!(ids.len(), 2, "delivered twice: {ids:?}");
    assert_eq!(ids[0], ids[1], "under one delivery id");
}

/// The one that holds a dead replica's task claim settles its reservations at
/// once: the event the dead replica stored is delivered now, not after the hold.
#[tokio::test(start_paused = true)]
async fn a_task_settled_by_its_claim_holder_delivers_a_stored_event_at_once() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;
    rig.kill_a(running).await;
    assert_eq!(rig.rows().await, 1);

    let started = tokio::time::Instant::now();
    assert_eq!(rig.b.settle_task(&TaskId::new("t")).await, 1);
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: Some(TaskState::Working)
        }
    );
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "{:?} against a hold of {:?}",
        started.elapsed(),
        hold(&rig)
    );
    assert_eq!(rig.rows().await, 0);
}

/// And an event the dead replica did not store is removed at once.
#[tokio::test(start_paused = true)]
async fn a_task_settled_by_its_claim_holder_removes_an_event_never_stored() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::Before);
    let running = rig.run_on_a(status(1, TaskState::Working));
    parked(&rig).await;
    rig.kill_a(running).await;
    assert_eq!(rig.rows().await, 1);

    assert_eq!(rig.b.settle_task(&TaskId::new("t")).await, 1);
    assert_eq!(rig.rows().await, 0);
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.b_seen.try_recv().is_err(), "nobody is told of it");
}

/// An executor's panic is saved as `failed`; a stop right after the save leaves
/// the ending reserved, and the task being `failed` is what arms it.
#[tokio::test(start_paused = true)]
async fn a_stop_after_a_panics_save_leaves_an_ending_another_replica_delivers() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::After);
    let running = rig.run_panic_on_a();
    parked(&rig).await;
    rig.kill_a(running).await;
    assert_eq!(
        rig.rows().await,
        1,
        "the ending was reserved before it was saved"
    );

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(
        next(&mut rig.b_seen).await,
        Seen {
            replica: "b",
            state: Some(TaskState::Failed)
        }
    );
    assert_eq!(rig.rows().await, 0);
}

/// A stop before the panic's save leaves the task running: the reserved ending
/// is removed and no webhook is told it failed.
#[tokio::test(start_paused = true)]
async fn a_stop_before_a_panics_save_is_settled_as_not_ended() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.stop(Stop::Before);
    let running = rig.run_panic_on_a();
    parked(&rig).await;
    rig.kill_a(running).await;

    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert_eq!(rig.rows().await, 0);
    assert!(rig.b_seen.try_recv().is_err());
}

/// A panic whose save is refused because another writer already ended the task
/// reserves nothing that stays: the stored state is announced, not `failed`.
#[tokio::test(start_paused = true)]
async fn a_panic_whose_save_is_refused_announces_the_stored_state_only() {
    let mut rig = Rig::new(TaskState::Canceled).await;
    let running = rig.run_panic_on_a();
    running.await.unwrap();

    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Canceled)
        }
    );
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "no `failed`");
    assert_eq!(rig.rows().await, 0);
}

/// The ordinary case: a panic is saved, announced once, at once, from the
/// reservation made before the save.
#[tokio::test(start_paused = true)]
async fn an_executor_panic_announces_failed_once_and_at_once() {
    let mut rig = Rig::new(TaskState::Working).await;
    let started = tokio::time::Instant::now();
    rig.run_panic_on_a().await.unwrap();

    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Failed)
        }
    );
    assert!(started.elapsed() < Duration::from_millis(100));
    rig.wait(Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "once");
    assert_eq!(rig.rows().await, 0);
    assert_eq!(rig.metrics.count(push_outcome::RESERVATION_SETTLED), 0);
}

/// An ending whose reservation outlives the push of the terminal state (it
/// was pushed already, by this processor) is released, never armed: the
/// webhooks are not told twice. An executor's panic reserves only for a task
/// that is not finished, so this is the guard behind that, not a path it takes
/// today; it is set up by hand.
#[tokio::test(start_paused = true)]
async fn an_ending_reserved_after_the_terminal_was_pushed_is_released_not_armed() {
    use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
    use a2a_protocol_types::task::{ContextId, TaskStatus};

    let mut rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    let failed = StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        id.clone(),
        ContextId::new("c"),
        TaskStatus::new(TaskState::Failed),
    ));
    let reservation = rig.a.reserve_ending(&id, &failed).await.expect("reserved");
    assert_eq!(rig.rows().await, 1);

    let mut run = Processor::new(
        &id,
        BackgroundDeps {
            task_store: &*rig.store,
            push: &rig.a,
            limits: &rig.limits,
            metrics: &crate::metrics::NoopMetrics,
        },
        task(TaskState::Working),
        CancellationToken::new(),
        None,
        None,
    );
    run.pushed_terminal = true;
    run.announce(TaskStatus::new(TaskState::Failed), Some(reservation))
        .await;

    assert_eq!(rig.rows().await, 0, "released");
    rig.wait(hold(&rig) + Duration::from_secs(30)).await;
    assert!(rig.a_seen.try_recv().is_err(), "nobody is told twice");
    assert!(rig.b_seen.try_recv().is_err());
}
