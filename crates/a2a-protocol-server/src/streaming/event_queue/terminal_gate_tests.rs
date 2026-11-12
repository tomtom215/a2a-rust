// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Tests for [`super`]: the gate, and the writer's side of it.

use super::*;
use a2a_protocol_types::events::TaskStatusUpdateEvent;
use a2a_protocol_types::task::{ContextId, TaskId, TaskState, TaskStatus};

fn status(state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        TaskStatus::new(state),
    ))
}

/// The state a ruled verdict names; `None` for an unconfirmed one.
fn ruled_state(verdict: &Verdict) -> Option<TaskState> {
    match verdict {
        Verdict::Ruled(event) => state_of(event),
        Verdict::Unconfirmed(_) => None,
    }
}

fn state_of(event: &StreamResponse) -> Option<TaskState> {
    match event {
        StreamResponse::StatusUpdate(e) => Some(e.status.state),
        _ => None,
    }
}

#[tokio::test]
async fn a_verdict_reaches_the_ticket_armed_for_its_seq() {
    let gate = TerminalGate::default();
    let mut first = gate.arm(1).expect("open gate arms");
    let mut second = gate.arm(2).expect("open gate arms");
    gate.resolve(2, status(TaskState::Canceled));
    gate.resolve(1, status(TaskState::Completed));
    assert_eq!(
        ruled_state(&first.try_recv().expect("resolved")),
        Some(TaskState::Completed)
    );
    assert_eq!(
        ruled_state(&second.try_recv().expect("resolved")),
        Some(TaskState::Canceled)
    );
}

#[tokio::test]
async fn a_disarmed_or_unknown_ticket_is_ignored() {
    let gate = TerminalGate::default();
    let mut rx = gate.arm(7).expect("arms");
    gate.disarm(7);
    gate.resolve(7, status(TaskState::Completed));
    assert!(
        matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
        "a disarmed ticket is dropped, and receives nothing"
    );
    // Resolving something never armed does not panic or leak.
    gate.resolve(8, status(TaskState::Completed));
    assert!(gate.lock().waiting.is_empty());
}

#[tokio::test]
async fn closing_releases_waiters_and_stops_arming() {
    let gate = std::sync::Arc::new(TerminalGate::default());
    let mut rx = gate.arm(1).expect("arms");
    drop(CloseOnDrop(std::sync::Arc::clone(&gate)));
    assert!(
        matches!(rx.try_recv(), Err(oneshot::error::TryRecvError::Closed)),
        "a waiter is released by the close"
    );
    assert!(gate.arm(2).is_none(), "a closed gate arms nothing");
}

// ── The writer's side ────────────────────────────────────────────────

use crate::streaming::event_queue::{
    EventQueueReader as _, EventQueueWriter as _, InMemoryQueueWriter,
    new_in_memory_queue_with_options, new_in_memory_queue_with_persistence,
};

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn gated() -> (
    std::sync::Arc<InMemoryQueueWriter>,
    crate::streaming::event_queue::InMemoryQueueReader,
    tokio::sync::mpsc::Receiver<
        a2a_protocol_types::error::A2aResult<crate::streaming::StreamEvent>,
    >,
) {
    let (writer, reader, rx) = new_in_memory_queue_with_persistence(8, 1 << 20, TIMEOUT);
    (std::sync::Arc::new(writer.with_terminal_gate()), reader, rx)
}

/// `fut`, or a failure after ten seconds: a broken writer must fail these
/// tests, not hang them.
async fn within<F: std::future::Future>(fut: F) -> F::Output {
    tokio::time::timeout(std::time::Duration::from_secs(10), fut)
        .await
        .expect("did not finish in time")
}

async fn next_state(
    reader: &mut crate::streaming::event_queue::InMemoryQueueReader,
) -> Option<TaskState> {
    let frame = reader.read().await?.ok()?;
    state_of(&frame.event)
}

/// A terminal event is held until the processor rules, and subscribers
/// see the ruling rather than what the executor wrote.
#[tokio::test]
async fn a_terminal_event_is_broadcast_as_the_verdict() {
    let (writer, mut reader, mut rx) = gated();
    let gate = writer.terminal_gate().expect("gated");
    let w = std::sync::Arc::clone(&writer);
    let write = tokio::spawn(async move { w.write(status(TaskState::Completed)).await });

    let handed = within(rx.recv()).await.expect("handed over").expect("ok");
    assert!(!write.is_finished(), "held until the verdict");
    gate.resolve(handed.seq.expect("positioned"), status(TaskState::Canceled));
    within(write).await.expect("joins").expect("write succeeds");

    assert_eq!(
        within(next_state(&mut reader)).await,
        Some(TaskState::Canceled)
    );
}

/// The error a subscriber reads next, if it reads one.
async fn next_error(
    reader: &mut crate::streaming::event_queue::InMemoryQueueReader,
) -> Option<a2a_protocol_types::error::A2aError> {
    reader.read().await?.err()
}

/// No verdict within the write timeout: nothing says the store holds the
/// event, so subscribers are told the completion is unconfirmed (an
/// `UNAVAILABLE` error), not that the task completed (adversarial review R1,
/// 2026-11-12). Until then the event went out unchanged.
#[tokio::test(start_paused = true)]
async fn no_verdict_in_time_tells_subscribers_it_is_unconfirmed() {
    let (writer, mut reader, mut rx) = gated();
    let w = std::sync::Arc::clone(&writer);
    let write = tokio::spawn(async move { w.write(status(TaskState::Completed)).await });
    let _handed = within(rx.recv()).await.expect("handed over");
    within(write).await.expect("joins").expect("write succeeds");
    let error = within(next_error(&mut reader))
        .await
        .expect("an error, not the event");
    assert!(error.is_unavailable(), "{error:?}");
    let gate = writer.terminal_gate().expect("gated");
    assert!(gate.lock().waiting.is_empty(), "the ticket was withdrawn");
}

/// The opt-out: `BestEffort` is what every release before 0.16.0 did, the
/// event goes out unchanged when no verdict arrives.
#[tokio::test(start_paused = true)]
async fn best_effort_broadcasts_the_event_unchanged_when_no_verdict_arrives() {
    let (writer, mut reader, mut rx) = new_in_memory_queue_with_persistence(8, 1 << 20, TIMEOUT);
    let writer = std::sync::Arc::new(
        writer
            .with_terminal_gate()
            .with_terminal_confirmation(crate::handler::TerminalConfirmation::BestEffort),
    );
    let w = std::sync::Arc::clone(&writer);
    let write = tokio::spawn(async move { w.write(status(TaskState::Completed)).await });
    let _handed = within(rx.recv()).await.expect("handed over");
    within(write).await.expect("joins").expect("write succeeds");
    assert_eq!(
        within(next_state(&mut reader)).await,
        Some(TaskState::Completed)
    );
}

/// The store ruled that it does not hold the event: subscribers are told
/// that, in either mode's `Required` default, and the gate carries the
/// processor's own error.
#[tokio::test]
async fn an_unconfirmed_verdict_reaches_subscribers_as_its_error() {
    let (writer, mut reader, mut rx) = gated();
    let gate = writer.terminal_gate().expect("gated");
    let w = std::sync::Arc::clone(&writer);
    let write = tokio::spawn(async move { w.write(status(TaskState::Completed)).await });
    let handed = within(rx.recv()).await.expect("handed over").expect("ok");
    gate.reject(
        handed.seq.expect("positioned"),
        a2a_protocol_types::error::A2aError::unavailable("the store said not now"),
    );
    within(write).await.expect("joins").expect("write succeeds");
    let error = within(next_error(&mut reader)).await.expect("an error");
    assert!(error.is_unavailable());
    assert!(
        error.message.contains("the store said not now"),
        "{error:?}"
    );
}

/// Nothing but a terminal event waits, and a closed persistence channel
/// — no processor left to rule — tells subscribers at once that the
/// terminal event is unconfirmed, without waiting out the timeout.
#[tokio::test]
async fn only_terminal_events_wait_and_only_while_someone_can_answer() {
    let (writer, mut reader, mut rx) = gated();
    writer
        .write(status(TaskState::Working))
        .await
        .expect("not held");
    assert!(within(rx.recv()).await.is_some());
    assert_eq!(
        within(next_state(&mut reader)).await,
        Some(TaskState::Working)
    );

    drop(rx);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        writer.write(status(TaskState::Completed)),
    )
    .await
    .expect("a closed channel does not wait out the timeout")
    .expect("write succeeds");
    let error = within(next_error(&mut reader))
        .await
        .expect("an error, not the event nothing stored");
    assert!(error.is_unavailable(), "{error:?}");
    let gate = writer.terminal_gate().expect("gated");
    assert!(gate.lock().waiting.is_empty(), "the ticket was withdrawn");
}

/// A terminal event held for its verdict is not overtaken by a later
/// write from another clone of the writer (`CancelTask` writes Canceled
/// while the executor may still be emitting): spec §3.5.2, events MUST
/// NOT be reordered, and each stream MUST receive the same events. Until
/// the writes were ordered the later event went out first, and a reader,
/// which skips a position it has already passed, then dropped the
/// terminal: the stream ended with no terminal state.
#[tokio::test]
async fn a_held_terminal_event_is_not_overtaken() {
    let (writer, mut reader, mut rx) = gated();
    let gate = writer.terminal_gate().expect("gated");
    let w = std::sync::Arc::clone(&writer);
    let terminal = tokio::spawn(async move { w.write(status(TaskState::Canceled)).await });
    let held = within(rx.recv()).await.expect("handed over").expect("ok");

    let w = std::sync::Arc::clone(&writer);
    let later = tokio::spawn(async move { w.write(status(TaskState::Working)).await });
    tokio::task::yield_now().await;
    gate.resolve(held.seq.expect("positioned"), status(TaskState::Canceled));
    within(terminal)
        .await
        .expect("joins")
        .expect("write succeeds");
    within(later).await.expect("joins").expect("write succeeds");
    drop(writer);

    let mut delivered = Vec::new();
    while let Some(frame) = within(reader.read()).await {
        let frame = frame.expect("no error");
        delivered.push((frame.seq, state_of(&frame.event)));
    }
    // The terminal event comes first, and the stream ends there (§3.1.6):
    // the later write is position 2, after it, and is never delivered.
    assert_eq!(delivered, [(Some(1), Some(TaskState::Canceled))]);
}

/// The same when the terminal event first waited for room on a full
/// persistence channel. The later write queues behind it for room, and
/// unless it also waits its turn to be positioned it takes the next slot
/// and is broadcast while the terminal event still awaits its verdict.
#[tokio::test]
async fn a_terminal_event_that_waited_for_room_is_not_overtaken() {
    let (tx, rx) = tokio::sync::broadcast::channel(8);
    let (persistence_tx, mut persisted) = tokio::sync::mpsc::channel(1);
    let writer = std::sync::Arc::new(
        InMemoryQueueWriter::new_with_persistence(
            tx,
            super::super::in_memory::PersistenceTx::Owned(persistence_tx),
            1 << 20,
            TIMEOUT,
        )
        .with_terminal_gate(),
    );
    let mut reader = crate::streaming::event_queue::InMemoryQueueReader::new(rx);
    let gate = writer.terminal_gate().expect("gated");
    writer
        .write(status(TaskState::Working))
        .await
        .expect("fills the channel");

    let w = std::sync::Arc::clone(&writer);
    let terminal = tokio::spawn(async move { w.write(status(TaskState::Canceled)).await });
    tokio::task::yield_now().await;
    let w = std::sync::Arc::clone(&writer);
    let later = tokio::spawn(async move { w.write(status(TaskState::Working)).await });
    tokio::task::yield_now().await;

    let first = within(persisted.recv()).await.expect("handed over");
    assert_eq!(first.expect("ok").seq, Some(1));
    let held = within(persisted.recv()).await.expect("handed over");
    let held = held.expect("ok").seq.expect("positioned");
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    gate.resolve(held, status(TaskState::Canceled));
    within(terminal)
        .await
        .expect("joins")
        .expect("write succeeds");
    within(later).await.expect("joins").expect("write succeeds");
    drop(writer);

    let mut delivered = Vec::new();
    while let Some(frame) = within(reader.read()).await {
        let frame = frame.expect("no error");
        delivered.push((frame.seq, state_of(&frame.event)));
    }
    assert_eq!(
        delivered,
        [
            (Some(1), Some(TaskState::Working)),
            (Some(2), Some(TaskState::Canceled)),
        ]
    );
}

/// A receiver attaching while a terminal event awaits its verdict waits
/// for the broadcast: the event's position is assigned, and a position
/// read then would count it as sent, so the reader would skip it.
#[tokio::test]
async fn an_attach_waits_for_a_held_event() {
    let (writer, _reader, mut rx) = gated();
    let gate = writer.terminal_gate().expect("gated");
    let w = std::sync::Arc::clone(&writer);
    let terminal = tokio::spawn(async move { w.write(status(TaskState::Canceled)).await });
    let held = within(rx.recv()).await.expect("handed over").expect("ok");

    let w = std::sync::Arc::clone(&writer);
    let attach = tokio::spawn(async move { w.attach().await });
    tokio::task::yield_now().await;
    assert!(!attach.is_finished(), "not while the event is held");
    gate.resolve(held.seq.expect("positioned"), status(TaskState::Canceled));
    within(terminal)
        .await
        .expect("joins")
        .expect("write succeeds");
    let (position, mut live) = within(attach).await.expect("joins");
    assert_eq!(position, 1, "attached after the event it waited for");
    assert!(live.try_recv().is_err(), "and does not receive it again");
}

/// A continued task's queue numbers on from its log, whose events earlier
/// turns stored: the mark starts there too, or a subscriber would wait
/// for positions no consumer of this queue will ever store.
#[tokio::test]
async fn seeding_a_writer_seeds_its_mark() {
    let (writer, _reader, _rx) = gated();
    writer.seed_seq(5);
    let read = writer
        .applied_mark()
        .read_through(5, tokio::time::Instant::now(), || async {})
        .await;
    assert_eq!((read.position, read.exact), (5, true));
}

/// A writer with no persistence channel has nobody to answer, so asking
/// for a gate gives it none, and a writer never asked has none either.
#[test]
fn a_gate_needs_a_persistence_channel_and_a_request() {
    let (plain, _reader) = new_in_memory_queue_with_options(8, 1 << 20, TIMEOUT);
    assert!(plain.with_terminal_gate().terminal_gate().is_none());
    let (unasked, _reader, _rx) = new_in_memory_queue_with_persistence(8, 1 << 20, TIMEOUT);
    assert!(unasked.terminal_gate().is_none());
}
