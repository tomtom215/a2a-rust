// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! An event the task did not take is never delivered, whatever else fails
//! around it, and one it took is not lost to an error that says otherwise
//! (adversarial review R2, 2026-11-12).
//!
//! The recovery predicate reads "the stored position reached the event's
//! position" as "the event was stored". That holds only if no later event
//! moved the position past an event that was not stored, which the processor
//! guarantees by writing no position past a reservation it could not settle.
//! These tests drive the sequences that put the two at odds.

use a2a_protocol_types::artifact::{Artifact, ArtifactId};
use a2a_protocol_types::events::TaskArtifactUpdateEvent;
use a2a_protocol_types::message::Part;
use a2a_protocol_types::task::ContextId;

use super::*;

/// An artifact id the task already holds, with one part.
fn held_artifact() -> Artifact {
    Artifact::new(ArtifactId::new("a"), vec![Part::text("x")])
}

fn task_with_artifact() -> Task {
    let mut t = task(TaskState::Working);
    t.artifacts = Some(vec![held_artifact()]);
    t
}

/// One more part appended to the artifact the task holds.
fn append(seq: u64) -> StreamEvent {
    StreamEvent::at(
        seq,
        StreamResponse::ArtifactUpdate(
            TaskArtifactUpdateEvent::new(
                TaskId::new("t"),
                ContextId::new("c"),
                Artifact::new(ArtifactId::new("a"), vec![Part::text("y")]),
            )
            .with_append(true),
        ),
    )
}

/// A new artifact `id`.
fn artifact_named(seq: u64, id: &str) -> StreamEvent {
    StreamEvent::at(
        seq,
        StreamResponse::ArtifactUpdate(
            TaskArtifactUpdateEvent::new(
                TaskId::new("t"),
                ContextId::new("c"),
                Artifact::new(ArtifactId::new(id), vec![Part::text("z")]),
            )
            .with_last_chunk(true),
        ),
    )
}

/// One processor, which handles `events` in order, running `between` after the
/// event at each index first (to change what fails).
async fn drive(rig: &Rig, stored: Task, events: Vec<StreamEvent>, mut between: impl FnMut(usize)) {
    let (store, lanes, limits) = (Arc::clone(&rig.store), rig.a.clone(), rig.limits.clone());
    let id = TaskId::new("t");
    let mut run = Processor::new(
        &id,
        BackgroundDeps {
            task_store: &*store,
            push: &lanes,
            limits: &limits,
            metrics: &crate::metrics::NoopMetrics,
        },
        stored,
        CancellationToken::new(),
        None,
        None,
    );
    for (index, event) in events.into_iter().enumerate() {
        run.handle(Ok(event)).await;
        between(index);
    }
    run.close_out().await;
}

/// Every delivery either replica made after `d` of paused time.
async fn deliveries(rig: &mut Rig, d: Duration) -> Vec<Seen> {
    rig.wait(d).await;
    let mut seen = Vec::new();
    while let Ok(one) = rig.a_seen.try_recv() {
        seen.push(one);
    }
    while let Ok(one) = rig.b_seen.try_recv() {
        seen.push(one);
    }
    seen
}

fn states(seen: &[Seen]) -> Vec<Option<TaskState>> {
    let mut states: Vec<_> = seen.iter().map(|s| s.state).collect();
    states.sort_by_key(|s| s.map(|s| s as u8));
    states
}

/// The review's sequence: an append the state machine rejects (the artifact is
/// at its part bound), a release of its reservation that fails and leaves the
/// rows, and then a valid status that moves the stored position past it. When
/// the hold runs out the worker must not read that position as proof the
/// append was stored.
#[tokio::test(start_paused = true)]
async fn a_rejected_append_whose_release_failed_is_not_delivered_when_a_later_event_is_stored() {
    let limits = HandlerLimits::default()
        .with_store_write_retry(Duration::from_secs(1))
        .with_max_parts_per_artifact(1);
    let mut rig = Rig::with(task_with_artifact(), limits).await;
    rig.flaky
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let flaky = Arc::clone(&rig.flaky);

    drive(
        &rig,
        task_with_artifact(),
        vec![append(1), status(2, TaskState::Working)],
        // The outbox is up again by the time the hold runs out.
        |index| {
            if index == 1 {
                flaky
                    .fail_release
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
        },
    )
    .await;

    let wait = hold(&rig) + Duration::from_secs(60);
    let seen = deliveries(&mut rig, wait).await;
    assert_eq!(
        states(&seen),
        vec![Some(TaskState::Working)],
        "the status was stored and is delivered once; the rejected append is not: {seen:?}"
    );
    assert_eq!(rig.rows().await, 0);
}

/// The same with a save that fails (not a rule): the failed event's rows
/// outlive it, and a later save succeeds.
#[tokio::test(start_paused = true)]
async fn a_failed_save_followed_by_a_successful_save_does_not_deliver_the_failed_event() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.fail_writes(
        |t| {
            t.artifacts
                .as_ref()
                .is_some_and(|a| a.iter().any(|a| a.id.0 == "b"))
        },
        false,
        None,
    );
    rig.flaky
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (store, flaky) = (Arc::clone(&rig.store), Arc::clone(&rig.flaky));

    drive(
        &rig,
        task(TaskState::Working),
        vec![artifact_named(1, "b"), status(2, TaskState::Working)],
        |index| {
            if index == 0 {
                // Back to normal for the next event.
                store.heal();
            }
            if index == 1 {
                flaky
                    .fail_release
                    .store(false, std::sync::atomic::Ordering::SeqCst);
            }
        },
    )
    .await;

    let wait = hold(&rig) + Duration::from_secs(60);
    let seen = deliveries(&mut rig, wait).await;
    assert_eq!(
        states(&seen),
        vec![Some(TaskState::Working)],
        "artifact `b` was never stored: {seen:?}"
    );
    let stored = rig
        .store
        .inner
        .get(&TaskId::new("t"))
        .await
        .unwrap()
        .unwrap();
    assert!(stored.artifacts.is_none_or(|a| a.is_empty()));
}

/// While a reservation cannot be settled, the processor stores no log
/// position past it: that is what makes the worker's reading of the position
/// exact. A stored position that reaches the unsettled event's would be the
/// evidence of the bug.
#[tokio::test(start_paused = true)]
async fn no_position_is_stored_past_a_reservation_that_could_not_be_settled() {
    let limits = HandlerLimits::default()
        .with_store_write_retry(Duration::from_secs(1))
        .with_max_parts_per_artifact(1);
    let rig = Rig::with(task_with_artifact(), limits).await;
    rig.flaky
        .fail_release
        .store(true, std::sync::atomic::Ordering::SeqCst);

    drive(
        &rig,
        task_with_artifact(),
        vec![append(1), status(2, TaskState::Working)],
        |_| {},
    )
    .await;

    let (_, position) = rig
        .store
        .inner
        .get_with_position(&TaskId::new("t"))
        .await
        .unwrap()
        .unwrap();
    assert!(
        position.is_none_or(|p| p < 1),
        "the status at position 2 was stored with its position while position 1's rows were unsettled: {position:?}"
    );
}

/// An event the store applied, whose acknowledgement was lost, is delivered:
/// its rows are not removed on the strength of an error.
#[tokio::test(start_paused = true)]
async fn an_event_whose_save_was_applied_but_reported_failed_is_delivered_once() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store.fail_writes(
        |t| {
            t.artifacts
                .as_ref()
                .is_some_and(|a| a.iter().any(|a| a.id.0 == "b"))
        },
        true,
        Some(1),
    );

    drive(
        &rig,
        task(TaskState::Working),
        vec![artifact_named(1, "b"), status(2, TaskState::Working)],
        |_| {},
    )
    .await;

    let wait = hold(&rig) + Duration::from_secs(60);
    let seen = deliveries(&mut rig, wait).await;
    assert_eq!(
        states(&seen),
        vec![None, Some(TaskState::Working)],
        "both were stored, so both are delivered, each once: {seen:?}"
    );
    let stored = rig
        .store
        .inner
        .get(&TaskId::new("t"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.artifacts.map(|a| a.len()),
        Some(1),
        "and the later whole-task save did not undo the artifact"
    );
}
