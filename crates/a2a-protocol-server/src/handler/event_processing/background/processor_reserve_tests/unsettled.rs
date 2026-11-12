// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a processor holds between two events when a write fails (adversarial
//! review R1 and R2, 2026-11-12): the reservations it could not settle, how
//! many it will hold, and whether a terminal event whose write failed ends the
//! executor.
//!
//! The recovery is redundant on purpose: a worker settles a reservation from
//! the store after its hold, and `close_out` settles what is left, so a
//! stream, a log or a webhook shows the same in the end whichever path ran.
//! These tests look at what the processor itself holds and does at each step,
//! which is where the paths differ.

use std::sync::atomic::Ordering::SeqCst;

use super::super::MAX_UNSETTLED;
use super::*;

/// A processor over `rig`'s replica `a`, on a task stored as working.
macro_rules! processor {
    ($rig:expr, $id:expr, $cancel:expr) => {
        Processor::new(
            &$id,
            BackgroundDeps {
                task_store: &*$rig.store,
                push: &$rig.a,
                limits: &$rig.limits,
                metrics: &crate::metrics::NoopMetrics,
            },
            task(TaskState::Working),
            $cancel,
            None,
            None,
        )
    };
}

/// An event whose write the store refused and whose fate it cannot say is held
/// unsettled; the next event settles it first, from what the store then holds.
#[tokio::test(start_paused = true)]
async fn a_later_event_settles_the_reservation_an_earlier_one_left() {
    let rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    let mut run = processor!(rig, id, CancellationToken::new());

    rig.store.fail_writes(|_| true, false, None);
    rig.store.unreadable.store(true, SeqCst);
    run.handle(Ok(status(1, TaskState::Working))).await;
    assert_eq!(
        run.unsettled.len(),
        1,
        "its reservation waits to be settled"
    );

    rig.store.heal();
    rig.store.unreadable.store(false, SeqCst);
    run.handle(Ok(status(2, TaskState::Working))).await;
    assert_eq!(
        run.unsettled.len(),
        0,
        "the earlier reservation was settled before the next event, not at the close"
    );
}

/// A processor holds at most `MAX_UNSETTLED` reservations: past it an event is
/// not reserved for (its push is the lane's once it is stored), so an outbox or
/// a store that is down for long does not grow without bound.
#[tokio::test(start_paused = true)]
async fn a_processor_holds_no_more_than_the_cap_of_unsettled_reservations() {
    let rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    let mut run = processor!(rig, id, CancellationToken::new());
    rig.store.fail_writes(|_| true, false, None);
    rig.store.unreadable.store(true, SeqCst);

    let cap = MAX_UNSETTLED as u64;
    for seq in 1..=cap {
        run.handle(Ok(status(seq, TaskState::Working))).await;
    }
    assert_eq!(run.unsettled.len(), MAX_UNSETTLED);
    assert_eq!(rig.rows().await, MAX_UNSETTLED, "one row held for each");

    run.handle(Ok(status(cap + 1, TaskState::Working))).await;
    assert_eq!(
        run.unsettled.len(),
        MAX_UNSETTLED,
        "nothing more is held past the cap"
    );
    assert_eq!(
        rig.rows().await,
        MAX_UNSETTLED,
        "and no row was reserved for the event past it"
    );
}

/// An event the store refused, when the store can say it does not hold it, is
/// settled at once (its rows released), and one it took though the write
/// reported a failure is settled at once too (its rows armed): neither waits.
#[tokio::test(start_paused = true)]
async fn an_event_the_store_can_account_for_is_settled_at_once() {
    for (ack_lost, rows) in [(false, 0), (true, 1)] {
        let rig = Rig::new(TaskState::Working).await;
        let id = TaskId::new("t");
        let mut run = processor!(rig, id, CancellationToken::new());
        rig.store.fail_writes(|_| true, ack_lost, None);
        run.handle(Ok(status(1, TaskState::Working))).await;
        assert_eq!(
            run.unsettled.len(),
            0,
            "ack lost: {ack_lost}; the store could say, so nothing waits"
        );
        if !ack_lost {
            assert_eq!(rig.rows().await, rows, "released, as the store lacks it");
        }
    }
}

/// A terminal write that reports a failure while the store holds the task in
/// the state the event names is this writer's own ending: the executor is not
/// cancelled, and the reservation is settled as stored.
#[tokio::test(start_paused = true)]
async fn a_completion_that_landed_is_this_writers_and_cancels_nothing() {
    let rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    let cancel = CancellationToken::new();
    let mut run = processor!(rig, id, cancel.clone());
    rig.store
        .fail_writes(|t| t.status.state == TaskState::Completed, true, None);

    run.handle(Ok(status(1, TaskState::Completed))).await;

    assert!(
        !cancel.is_cancelled(),
        "the store holds the completion this writer made: it was not superseded"
    );
    assert_eq!(run.unsettled.len(), 0);
}

/// The store holds the task in a different ending than the one this writer
/// tried to write: another writer finished it, and this writer's executor is
/// cancelled.
#[tokio::test(start_paused = true)]
async fn a_completion_the_store_holds_as_another_ending_supersedes_this_writer() {
    let rig = Rig::new(TaskState::Failed).await;
    let id = TaskId::new("t");
    let cancel = CancellationToken::new();
    let mut run = processor!(rig, id, cancel.clone());
    rig.store
        .fail_writes(|t| t.status.state == TaskState::Completed, false, None);

    run.handle(Ok(status(1, TaskState::Completed))).await;

    assert!(
        cancel.is_cancelled(),
        "another writer ended the task in another state: this executor is cancelled"
    );
}
