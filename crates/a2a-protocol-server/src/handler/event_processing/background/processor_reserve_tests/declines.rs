// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! When no reservation is made (ADR 0037, audit H14): for an event the state machine never pushes, against a task store with no event log to settle from, and for an ending that is not terminal or has no task store to settle it by; the rig and helpers are `processor_reserve_tests`'s.

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::message::Message;
use a2a_protocol_types::task::{ContextId, TaskStatus};

use super::*;

/// A snapshot or a message is never pushed by the state machine, so a
/// reservation for one would write rows only to remove them. A status update
/// on the same task, with the same configuration, is reserved.
#[tokio::test(start_paused = true)]
async fn nothing_is_reserved_for_an_event_the_state_machine_never_pushes() {
    let rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    for event in [
        StreamResponse::Task(task(TaskState::Working)),
        StreamResponse::Message(Message::user("m", vec![])),
    ] {
        assert!(
            rig.a.reserve(&id, Some(1), &event).await.is_none(),
            "{event:?}"
        );
        assert_eq!(rig.rows().await, 0, "no row was written for {event:?}");
    }

    let reserved = rig
        .a
        .reserve(&id, Some(1), &status(1, TaskState::Working).event)
        .await;
    assert!(reserved.is_some(), "a status update is reserved");
    assert_eq!(rig.rows().await, 1);
}

/// Without an event log a reservation could not be settled from the store, so
/// none is made, whatever the event; with the log, the same call reserves.
#[tokio::test(start_paused = true)]
async fn nothing_is_reserved_against_a_store_with_no_event_log() {
    let rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    let event = status(1, TaskState::Working).event;
    rig.store
        .without_log
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(rig.a.reserve(&id, Some(1), &event).await.is_none());
    assert_eq!(rig.rows().await, 0, "no row was written");

    rig.store
        .without_log
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(
        rig.a.reserve(&id, Some(1), &event).await.is_some(),
        "the same call reserves once there is a log"
    );
    assert_eq!(rig.rows().await, 1);
}

fn failed() -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent::new(
        TaskId::new("t"),
        ContextId::new("c"),
        TaskStatus::new(TaskState::Failed),
    ))
}

/// An ending is a terminal status update, reserved only where a task store can
/// settle it by the state the task is stored in: not for a status that is not
/// terminal, not for another kind of event, and not without a task store.
#[tokio::test(start_paused = true)]
async fn an_ending_is_reserved_only_for_a_terminal_update_and_with_a_task_store() {
    let rig = Rig::new(TaskState::Working).await;
    let id = TaskId::new("t");
    assert!(
        rig.a
            .reserve_ending(&id, &status(1, TaskState::Working).event)
            .await
            .is_none(),
        "a status that is not terminal is not an ending"
    );
    assert!(
        rig.a
            .reserve_ending(&id, &artifact(1).event)
            .await
            .is_none(),
        "nor is an artifact"
    );
    assert_eq!(rig.rows().await, 0);

    // The same terminal update, with no task store to settle it by.
    let configs = Arc::new(InMemoryPushConfigStore::new());
    configs
        .set(TaskPushNotificationConfig::new("t", "https://example.com/hook").with_id("cfg"))
        .await
        .unwrap();
    let (seen, _rx) = mpsc::unbounded_channel();
    let without_store = crate::handler::PushLanes::with_outbox(
        configs,
        Some(Arc::new(Sender::new("plain", seen)) as Arc<dyn PushSender>),
        rig.limits.clone(),
        Arc::new(crate::metrics::NoopMetrics),
        TaskTracker::new(),
        Some((
            Arc::clone(&rig.outbox) as Arc<dyn PushOutbox>,
            CancellationToken::new(),
        )),
    );
    assert!(without_store.reserve_ending(&id, &failed()).await.is_none());
    assert_eq!(rig.rows().await, 0, "no row was written for it");

    // With the store, the terminal update is reserved.
    assert!(rig.a.reserve_ending(&id, &failed()).await.is_some());
    assert_eq!(rig.rows().await, 1);
}
