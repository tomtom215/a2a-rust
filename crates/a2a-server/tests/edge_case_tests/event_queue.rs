// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Event queue manager lifecycle, destruction, and read/write tests.

use super::*;

#[tokio::test]
async fn event_queue_manager_lifecycle() {
    let mgr = a2a_protocol_server::EventQueueManager::new();

    let task_id = TaskId::new("task-1");
    assert_eq!(mgr.active_count().await, 0, "fresh manager must have 0 queues");

    // Create a queue
    let (_writer, reader) = mgr.get_or_create(&task_id).await;
    assert!(reader.is_some(), "first get_or_create must return a reader");
    assert_eq!(mgr.active_count().await, 1, "one queue must be active after creation");

    // get_or_create again returns existing writer, no new reader
    let (_writer2, reader2) = mgr.get_or_create(&task_id).await;
    assert!(reader2.is_none(), "second get_or_create must NOT return a new reader");
    assert_eq!(mgr.active_count().await, 1, "count must remain 1 for same task_id");

    // Destroy the queue
    mgr.destroy(&task_id).await;
    assert_eq!(mgr.active_count().await, 0, "count must be 0 after destroy");
}

#[tokio::test]
async fn event_queue_manager_destroy_all() {
    let mgr = a2a_protocol_server::EventQueueManager::new();

    mgr.get_or_create(&TaskId::new("t1")).await;
    mgr.get_or_create(&TaskId::new("t2")).await;
    assert_eq!(mgr.active_count().await, 2, "two queues must be active");

    mgr.destroy_all().await;
    assert_eq!(mgr.active_count().await, 0, "all queues must be destroyed");
}

#[tokio::test]
async fn event_queue_write_and_read() {
    let (writer, reader) = a2a_protocol_server::streaming::event_queue::new_in_memory_queue();
    let mut reader = reader;

    let event = StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new("t1"),
        context_id: "ctx".into(),
        status: TaskStatus::new(TaskState::Working),
        metadata: None,
    });

    writer.write(event).await.unwrap();
    drop(writer); // Close the channel

    let received: Option<a2a_protocol_types::error::A2aResult<StreamResponse>> =
        reader.read().await;
    let received = received.expect("reader must yield an event before EOF");
    let received = received.unwrap();
    assert!(
        matches!(received, StreamResponse::StatusUpdate(_)),
        "expected StatusUpdate, got {received:?}"
    );

    // After writer is dropped, reader should get None
    let eof: Option<a2a_protocol_types::error::A2aResult<StreamResponse>> = reader.read().await;
    assert!(eof.is_none(), "reader must return None after writer is dropped");
}
