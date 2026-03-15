// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for event queue manager and configurable capacity.

use a2a_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_types::task::{TaskId, TaskState, TaskStatus};

use a2a_server::streaming::{
    EventQueueManager, EventQueueReader, EventQueueWriter, DEFAULT_QUEUE_CAPACITY,
};

fn status_event(task_id: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new(task_id),
        context_id: "ctx".into(),
        status: TaskStatus::new(state),
        metadata: None,
    })
}

#[tokio::test]
async fn default_capacity_is_64() {
    assert_eq!(DEFAULT_QUEUE_CAPACITY, 64);
}

#[tokio::test]
async fn custom_capacity_manager() {
    let mgr = EventQueueManager::with_capacity(2);
    let task_id = TaskId::new("task-1");
    let (writer, reader) = mgr.get_or_create(&task_id).await;
    let reader = reader.expect("first call should return reader");

    // Fill the capacity (2 events).
    writer
        .write(status_event("task-1", TaskState::Working))
        .await
        .unwrap();
    writer
        .write(status_event("task-1", TaskState::Completed))
        .await
        .unwrap();

    // Read both events.
    let mut reader = reader;
    let e1 = reader.read().await.unwrap().unwrap();
    assert!(
        matches!(e1, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Working)
    );
    let e2 = reader.read().await.unwrap().unwrap();
    assert!(
        matches!(e2, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Completed)
    );
}

#[tokio::test]
async fn get_or_create_returns_none_for_existing_queue() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (_writer1, reader1) = mgr.get_or_create(&task_id).await;
    assert!(
        reader1.is_some(),
        "first call should create queue with reader"
    );

    let (_writer2, reader2) = mgr.get_or_create(&task_id).await;
    assert!(
        reader2.is_none(),
        "second call should return None (queue exists)"
    );
}

#[tokio::test]
async fn destroy_allows_recreation() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (_writer, _reader) = mgr.get_or_create(&task_id).await;
    mgr.destroy(&task_id).await;

    let (_writer2, reader2) = mgr.get_or_create(&task_id).await;
    assert!(
        reader2.is_some(),
        "after destroy, get_or_create should create a new queue"
    );
}

#[tokio::test]
async fn writer_close_does_not_panic() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");
    let (writer, _reader) = mgr.get_or_create(&task_id).await;
    // close() should succeed without errors.
    writer.close().await.unwrap();
}

#[tokio::test]
async fn reader_returns_none_when_all_writers_dropped() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");
    let (writer, reader) = mgr.get_or_create(&task_id).await;
    let mut reader = reader.unwrap();

    // Drop the manager's reference and the local writer.
    mgr.destroy(&task_id).await;
    drop(writer);

    // Reader should see None (channel closed).
    let result = reader.read().await;
    assert!(
        result.is_none(),
        "reader should see None when writers are dropped"
    );
}

#[tokio::test]
async fn multiple_tasks_have_independent_queues() {
    let mgr = EventQueueManager::new();
    let id1 = TaskId::new("task-1");
    let id2 = TaskId::new("task-2");

    let (w1, r1) = mgr.get_or_create(&id1).await;
    let (w2, r2) = mgr.get_or_create(&id2).await;
    let mut r1 = r1.unwrap();
    let mut r2 = r2.unwrap();

    // Write to task-1 only.
    w1.write(status_event("task-1", TaskState::Working))
        .await
        .unwrap();

    // Read from task-1 — should get the event.
    let ev1 = r1.read().await.unwrap().unwrap();
    assert!(matches!(ev1, StreamResponse::StatusUpdate(ref u) if u.task_id.0 == "task-1"));

    // Write to task-2.
    w2.write(status_event("task-2", TaskState::Completed))
        .await
        .unwrap();

    let ev2 = r2.read().await.unwrap().unwrap();
    assert!(matches!(ev2, StreamResponse::StatusUpdate(ref u) if u.task_id.0 == "task-2"));
}
