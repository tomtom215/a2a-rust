// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! What a terminal event tells the stream, the log, the webhooks and `GetTask`
//! when the store's write of it fails (adversarial review R1, 2026-11-12).
//!
//! All four must describe one outcome: the task is stored in the terminal
//! state, and then all four say so, or it is not, and none says it finished.
//! The rig's store fails only the writes the test names, so a read of the task
//! still works, as it does in an outage of one write path.

use super::*;
use crate::streaming::event_queue::terminal_gate::{TerminalGate, Verdict};

/// What a stream subscriber is told at the terminal event's position.
async fn told(
    rig: &Rig,
    event: StreamEvent,
    seq: u64,
    stored: TaskState,
) -> (Verdict, Option<TaskState>) {
    let _ = stored;
    let gate = TerminalGate::default();
    let ticket = gate.arm(seq).expect("an open gate arms");
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
        task(TaskState::Working),
        CancellationToken::new(),
        Some(&gate),
        None,
    );
    run.handle(Ok(event)).await;
    run.close_out().await;
    let verdict = ticket.await.expect("the processor answered the ticket");
    let stored = store.inner.get(&id).await.unwrap().map(|t| t.status.state);
    (verdict, stored)
}

fn ruled(verdict: &Verdict) -> Option<TaskState> {
    match verdict {
        Verdict::Ruled(event) => match &**event {
            StreamResponse::StatusUpdate(u) => Some(u.status.state),
            _ => None,
        },
        Verdict::Unconfirmed(_) => None,
    }
}

/// The review's own sequence: the store rejects only the `Completed` write and
/// still reads. The stream must not be told `Completed`.
#[tokio::test(start_paused = true)]
async fn a_rejected_completion_is_not_told_to_the_stream_as_completed() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store
        .fail_writes(|t| t.status.state == TaskState::Completed, false, None);

    let (verdict, stored) =
        told(&rig, status(1, TaskState::Completed), 1, TaskState::Working).await;

    assert_eq!(stored, Some(TaskState::Working), "the store never took it");
    assert_eq!(
        ruled(&verdict),
        None,
        "the stream was told the task completed, which the store never recorded: {verdict:?}"
    );
    assert!(
        matches!(verdict, Verdict::Unconfirmed(ref e) if e.is_unavailable()),
        "an explicit unavailable outcome, not a success: {verdict:?}"
    );
    let log = rig.store.inner.read_events(&TaskId::new("t"), 0, 10).await;
    assert!(
        log.unwrap().is_empty(),
        "the log must not record a completion the store lacks"
    );
    rig.wait(hold(&rig) + Duration::from_secs(60)).await;
    assert!(
        rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err(),
        "no webhook is told of a completion the store lacks"
    );
    assert_eq!(rig.rows().await, 0, "nothing is left to be armed");
}

/// The write is applied and its acknowledgement is lost: the task *is*
/// completed, so the stream, the log, the webhook and the store agree on that.
#[tokio::test(start_paused = true)]
async fn a_completion_whose_acknowledgement_was_lost_is_confirmed_by_reading_it_back() {
    let mut rig = Rig::new(TaskState::Working).await;
    rig.store
        .fail_writes(|t| t.status.state == TaskState::Completed, true, None);

    let (verdict, stored) =
        told(&rig, status(1, TaskState::Completed), 1, TaskState::Working).await;

    assert_eq!(stored, Some(TaskState::Completed));
    assert_eq!(
        ruled(&verdict),
        Some(TaskState::Completed),
        "the store holds the completion, so the stream may say so: {verdict:?}"
    );
    assert_eq!(
        next(&mut rig.a_seen).await,
        Seen {
            replica: "a",
            state: Some(TaskState::Completed)
        },
        "and the webhook hears it, once"
    );
    rig.wait(hold(&rig) + Duration::from_secs(60)).await;
    assert!(rig.a_seen.try_recv().is_err() && rig.b_seen.try_recv().is_err());
    let log = rig.store.inner.read_events(&TaskId::new("t"), 0, 10).await;
    assert_eq!(log.unwrap().len(), 1, "the log records it");
}

/// An event that repeats a terminal state the task already holds is a harmless
/// repeat: the stream is told that state, and nothing is delivered twice.
#[tokio::test(start_paused = true)]
async fn a_repeated_terminal_event_is_confirmed_by_the_state_already_stored() {
    let rig = Rig::new(TaskState::Completed).await;
    let (verdict, stored) = told(
        &rig,
        status(2, TaskState::Completed),
        2,
        TaskState::Completed,
    )
    .await;
    assert_eq!(stored, Some(TaskState::Completed));
    assert_eq!(ruled(&verdict), Some(TaskState::Completed));
}

/// The store is not readable either: the write failed and the state cannot be
/// confirmed, so it is not announced.
#[tokio::test(start_paused = true)]
async fn a_completion_that_cannot_be_read_back_is_not_announced() {
    let rig = Rig::new(TaskState::Working).await;
    rig.store
        .fail_writes(|t| t.status.state == TaskState::Completed, false, None);
    rig.store
        .unreadable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    // `unreadable` covers `get_with_position`; make plain `get` fail too.
    rig.store
        .fail_reads
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (verdict, _) = told(&rig, status(1, TaskState::Completed), 1, TaskState::Working).await;
    assert!(
        matches!(verdict, Verdict::Unconfirmed(ref e) if e.is_unavailable()),
        "{verdict:?}"
    );
}
