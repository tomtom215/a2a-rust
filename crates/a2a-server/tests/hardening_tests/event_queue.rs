// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Tests for `EventQueueManager`: creation, destruction, read/write,
//! and writer-close semantics.

use super::*;

#[tokio::test]
async fn event_queue_get_or_create_returns_writer_and_reader_on_first_call() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-1");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    assert!(
        reader.is_some(),
        "first get_or_create should return a reader"
    );
    // Writer should be usable (not null).
    drop(writer);
}

#[tokio::test]
async fn event_queue_get_or_create_returns_existing_writer_no_reader_on_second_call() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-2");

    let (_writer1, reader1) = manager.get_or_create(&task_id).await;
    assert!(reader1.is_some());

    let (_writer2, reader2) = manager.get_or_create(&task_id).await;
    assert!(
        reader2.is_none(),
        "second get_or_create should return None for reader"
    );
}

#[tokio::test]
async fn event_queue_destroy_allows_fresh_creation() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-3");

    // Create then destroy.
    let (_writer, _reader) = manager.get_or_create(&task_id).await;
    manager.destroy(&task_id).await;

    // Re-create should give a new reader.
    let (_writer2, reader2) = manager.get_or_create(&task_id).await;
    assert!(
        reader2.is_some(),
        "get_or_create after destroy should return a fresh reader"
    );
}

#[tokio::test]
async fn event_queue_write_and_read_events() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-4");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    let mut reader = reader.expect("should get reader");

    // Write an event.
    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: task_id.clone(),
        context_id: "ctx".into(),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });
    writer.write(event).await.expect("write");

    // Drop writer and manager reference to close channel.
    drop(writer);
    manager.destroy(&task_id).await;

    // Read the event back.
    let received = reader.read().await.expect("read should return Some");
    let update = received.expect("event should be Ok");
    assert!(
        matches!(update, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Working),
        "should read back the Working status event"
    );

    // Channel is closed after writer is dropped.
    assert!(reader.read().await.is_none(), "channel should be closed");
}

#[tokio::test]
async fn event_queue_writer_close_causes_reader_none() {
    let manager = EventQueueManager::new();
    let task_id = TaskId::new("eq-5");

    let (writer, reader) = manager.get_or_create(&task_id).await;
    let mut reader = reader.expect("reader");

    // Drop the writer without writing anything.
    drop(writer);
    manager.destroy(&task_id).await;

    // Reader should get None immediately.
    assert!(
        reader.read().await.is_none(),
        "reader should return None when writer is dropped without writing"
    );
}
