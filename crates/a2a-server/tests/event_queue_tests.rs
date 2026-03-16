// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

//! Direct tests for the event queue primitives:
//! `InMemoryQueueWriter`, `InMemoryQueueReader`, and `EventQueueManager`.

use std::sync::Arc;

use a2a_protocol_types::events::{StreamResponse, TaskStatusUpdateEvent};
use a2a_protocol_types::task::{TaskId, TaskState, TaskStatus};

use a2a_protocol_server::streaming::event_queue::{
    new_in_memory_queue, new_in_memory_queue_with_capacity, new_in_memory_queue_with_options,
};
use a2a_protocol_server::streaming::{
    EventQueueManager, EventQueueReader, EventQueueWriter, DEFAULT_MAX_EVENT_SIZE,
    DEFAULT_QUEUE_CAPACITY,
};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn status_event(task_id: &str, state: TaskState) -> StreamResponse {
    StreamResponse::StatusUpdate(TaskStatusUpdateEvent {
        task_id: TaskId::new(task_id),
        context_id: "ctx".into(),
        status: TaskStatus::new(state),
        metadata: None,
    })
}

// ── 1. Writer/reader lifecycle ───────────────────────────────────────────────

#[tokio::test]
async fn write_then_read_single_event() {
    let (writer, mut reader) = new_in_memory_queue();
    writer
        .write(status_event("t1", TaskState::Working))
        .await
        .unwrap();

    let event = reader.read().await.unwrap().unwrap();
    assert!(
        matches!(event, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Working)
    );
}

#[tokio::test]
async fn write_multiple_events_read_in_order() {
    let (writer, mut reader) = new_in_memory_queue();

    let states = [
        TaskState::Submitted,
        TaskState::Working,
        TaskState::Completed,
    ];
    for state in &states {
        writer.write(status_event("t1", *state)).await.unwrap();
    }

    for expected in &states {
        let event = reader.read().await.unwrap().unwrap();
        match event {
            StreamResponse::StatusUpdate(ref u) => assert_eq!(u.status.state, *expected),
            _ => panic!("expected StatusUpdate"),
        }
    }
}

#[tokio::test]
async fn reader_returns_none_after_writer_dropped() {
    let (writer, mut reader) = new_in_memory_queue();
    writer
        .write(status_event("t1", TaskState::Working))
        .await
        .unwrap();
    drop(writer);

    // Drain the buffered event.
    let _ = reader.read().await;
    // Now should get None (channel closed).
    let result = reader.read().await;
    assert!(
        result.is_none(),
        "reader should return None after writer is dropped"
    );
}

// ── 2. Concurrent writes from multiple writer clones ─────────────────────────

#[tokio::test]
async fn concurrent_writes_from_cloned_writers() {
    let (writer, mut reader) = new_in_memory_queue_with_capacity(100);
    let num_tasks = 10;
    let mut handles = Vec::new();

    for i in 0..num_tasks {
        let w = writer.clone();
        handles.push(tokio::spawn(async move {
            w.write(status_event(&format!("t-{i}"), TaskState::Working))
                .await
                .unwrap();
        }));
    }

    for h in handles {
        h.await.unwrap();
    }
    drop(writer);

    let mut count = 0;
    while let Some(Ok(_)) = reader.read().await {
        count += 1;
    }
    assert_eq!(
        count, num_tasks,
        "should receive all events from concurrent writers"
    );
}

#[tokio::test]
async fn cloned_writer_still_works_after_original_dropped() {
    let (writer, mut reader) = new_in_memory_queue();
    let clone = writer.clone();
    drop(writer);

    clone
        .write(status_event("t1", TaskState::Completed))
        .await
        .unwrap();
    drop(clone);

    let event = reader.read().await.unwrap().unwrap();
    assert!(
        matches!(event, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Completed)
    );
    // Channel now closed.
    assert!(reader.read().await.is_none());
}

// ── 3. Backpressure when queue is full (bounded channel) ─────────────────────

#[tokio::test]
async fn backpressure_bounded_channel() {
    // Capacity of 1: second write should block until first is read.
    let (writer, mut reader) = new_in_memory_queue_with_capacity(1);

    writer
        .write(status_event("t1", TaskState::Working))
        .await
        .unwrap();

    // Spawn a writer that tries to send a second event (will block).
    let w2 = writer.clone();
    let handle = tokio::spawn(async move {
        w2.write(status_event("t1", TaskState::Completed))
            .await
            .unwrap();
    });

    // Give the spawned task a moment to start blocking.
    tokio::task::yield_now().await;
    assert!(
        !handle.is_finished(),
        "second write should be blocked (queue full)"
    );

    // Reading frees a slot, unblocking the writer.
    let _ = reader.read().await;
    handle.await.unwrap();

    let event = reader.read().await.unwrap().unwrap();
    assert!(
        matches!(event, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Completed)
    );
}

// ── 4. max_concurrent_queues enforcement in EventQueueManager ────────────────

#[tokio::test]
async fn max_concurrent_queues_blocks_new_creation() {
    let mgr = EventQueueManager::with_capacity(8).with_max_concurrent_queues(2);

    let id1 = TaskId::new("task-1");
    let id2 = TaskId::new("task-2");
    let id3 = TaskId::new("task-3");

    let (_w1, r1) = mgr.get_or_create(&id1).await;
    assert!(r1.is_some(), "first queue should be created with reader");

    let (_w2, r2) = mgr.get_or_create(&id2).await;
    assert!(r2.is_some(), "second queue should be created with reader");

    // Third queue should be rejected (limit is 2).
    let (_w3, r3) = mgr.get_or_create(&id3).await;
    assert!(
        r3.is_none(),
        "third queue should return None reader (limit reached)"
    );

    // Verify that the third task was NOT inserted into the map.
    assert_eq!(
        mgr.active_count().await,
        2,
        "only 2 queues should be active"
    );
}

#[tokio::test]
async fn destroying_queue_frees_slot_for_new_creation() {
    let mgr = EventQueueManager::with_capacity(8).with_max_concurrent_queues(1);

    let id1 = TaskId::new("task-1");
    let id2 = TaskId::new("task-2");

    let (_w1, r1) = mgr.get_or_create(&id1).await;
    assert!(r1.is_some());

    // Limit reached.
    let (_w2, r2) = mgr.get_or_create(&id2).await;
    assert!(r2.is_none());

    // Destroy task-1, freeing a slot.
    mgr.destroy(&id1).await;

    let (_w3, r3) = mgr.get_or_create(&id2).await;
    assert!(r3.is_some(), "should create new queue after slot freed");
}

// ── 5. destroy() behavior — reader sees None ─────────────────────────────────

#[tokio::test]
async fn destroy_causes_reader_to_see_none() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (writer, reader) = mgr.get_or_create(&task_id).await;
    let mut reader = reader.unwrap();

    // Write one event for good measure.
    writer
        .write(status_event("task-1", TaskState::Working))
        .await
        .unwrap();

    // Destroy removes the manager's Arc reference to the writer.
    mgr.destroy(&task_id).await;
    // Drop the local writer Arc — now all senders are gone.
    drop(writer);

    // Drain the buffered event.
    let _ = reader.read().await;
    // Now reader should see None.
    let result = reader.read().await;
    assert!(
        result.is_none(),
        "reader should see None after destroy + writer drop"
    );
}

#[tokio::test]
async fn destroy_nonexistent_task_is_noop() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("nonexistent");
    // Should not panic.
    mgr.destroy(&task_id).await;
    assert_eq!(mgr.active_count().await, 0);
}

// ── 6. max_event_size enforcement ────────────────────────────────────────────

#[tokio::test]
async fn oversized_event_rejected() {
    // Create a queue with a tiny max event size (32 bytes).
    let (writer, _reader) =
        new_in_memory_queue_with_options(8, 32, std::time::Duration::from_secs(5));

    // A normal status event serializes to well over 32 bytes.
    let result = writer
        .write(status_event("task-1", TaskState::Working))
        .await;

    assert!(result.is_err(), "oversized event should be rejected");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("exceeds maximum"),
        "error should mention size exceeded, got: {err_msg}"
    );
}

#[tokio::test]
async fn event_within_size_limit_accepted() {
    // Use a generous limit.
    let (writer, mut reader) = new_in_memory_queue_with_options(
        8,
        DEFAULT_MAX_EVENT_SIZE,
        std::time::Duration::from_secs(5),
    );

    writer
        .write(status_event("t1", TaskState::Working))
        .await
        .unwrap();

    let event = reader.read().await.unwrap().unwrap();
    assert!(matches!(event, StreamResponse::StatusUpdate(_)));
}

#[tokio::test]
async fn manager_max_event_size_propagates_to_queues() {
    // Create a manager with a very small max event size.
    let mgr = EventQueueManager::with_capacity(8).with_max_event_size(16);
    let task_id = TaskId::new("task-1");
    let (writer, _reader) = mgr.get_or_create(&task_id).await;

    let result = writer
        .write(status_event("task-1", TaskState::Working))
        .await;
    assert!(
        result.is_err(),
        "manager's max_event_size should be enforced"
    );
}

// ── 7. destroy_all() clears all queues ───────────────────────────────────────

#[tokio::test]
async fn destroy_all_clears_all_queues() {
    let mgr = EventQueueManager::new();
    let ids: Vec<TaskId> = (0..5).map(|i| TaskId::new(format!("task-{i}"))).collect();

    let mut writers = Vec::new();
    let mut readers = Vec::new();
    for id in &ids {
        let (w, r) = mgr.get_or_create(id).await;
        writers.push(w);
        readers.push(r.unwrap());
    }
    assert_eq!(mgr.active_count().await, 5);

    mgr.destroy_all().await;
    assert_eq!(mgr.active_count().await, 0, "all queues should be removed");

    // Drop all writers (manager already dropped its Arc refs via destroy_all).
    drop(writers);

    // All readers should see None.
    for reader in &mut readers {
        let result = reader.read().await;
        assert!(result.is_none(), "reader should see None after destroy_all");
    }
}

#[tokio::test]
async fn destroy_all_allows_recreation() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (_w, _r) = mgr.get_or_create(&task_id).await;
    mgr.destroy_all().await;

    let (_w2, r2) = mgr.get_or_create(&task_id).await;
    assert!(
        r2.is_some(),
        "should be able to recreate queue after destroy_all"
    );
    assert_eq!(mgr.active_count().await, 1);
}

// ── 8. get_or_create returns existing writer for same task_id ────────────────

#[tokio::test]
async fn get_or_create_returns_existing_writer_for_same_task() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (writer1, reader1) = mgr.get_or_create(&task_id).await;
    assert!(reader1.is_some(), "first call creates a new queue");

    let (writer2, reader2) = mgr.get_or_create(&task_id).await;
    assert!(
        reader2.is_none(),
        "second call returns None reader (existing queue)"
    );

    // Both writers should be the same Arc (same underlying channel).
    assert!(
        Arc::ptr_eq(&writer1, &writer2),
        "writers should be the same Arc for the same task_id"
    );
}

#[tokio::test]
async fn get_or_create_different_tasks_get_different_writers() {
    let mgr = EventQueueManager::new();
    let id1 = TaskId::new("task-1");
    let id2 = TaskId::new("task-2");

    let (w1, r1) = mgr.get_or_create(&id1).await;
    let (w2, r2) = mgr.get_or_create(&id2).await;

    assert!(r1.is_some());
    assert!(r2.is_some());
    assert!(
        !Arc::ptr_eq(&w1, &w2),
        "different tasks should have different writers"
    );
}

#[tokio::test]
async fn existing_writer_can_still_send_to_original_reader() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let (_writer1, reader1) = mgr.get_or_create(&task_id).await;
    let mut reader1 = reader1.unwrap();

    // Second call returns the same writer but no reader.
    let (writer2, reader2) = mgr.get_or_create(&task_id).await;
    assert!(reader2.is_none());

    // Writing via the second handle should be readable from the original reader.
    writer2
        .write(status_event("task-1", TaskState::Completed))
        .await
        .unwrap();

    let event = reader1.read().await.unwrap().unwrap();
    assert!(
        matches!(event, StreamResponse::StatusUpdate(ref u) if u.status.state == TaskState::Completed)
    );
}

// ── 9. active_count() returns correct count ──────────────────────────────────

#[tokio::test]
async fn active_count_starts_at_zero() {
    let mgr = EventQueueManager::new();
    assert_eq!(mgr.active_count().await, 0);
}

#[tokio::test]
async fn active_count_increments_on_create() {
    let mgr = EventQueueManager::new();
    for i in 0..5 {
        let id = TaskId::new(format!("task-{i}"));
        let _ = mgr.get_or_create(&id).await;
        assert_eq!(mgr.active_count().await, i + 1);
    }
}

#[tokio::test]
async fn active_count_decrements_on_destroy() {
    let mgr = EventQueueManager::new();
    let ids: Vec<TaskId> = (0..3).map(|i| TaskId::new(format!("task-{i}"))).collect();

    for id in &ids {
        let _ = mgr.get_or_create(id).await;
    }
    assert_eq!(mgr.active_count().await, 3);

    mgr.destroy(&ids[1]).await;
    assert_eq!(mgr.active_count().await, 2);

    mgr.destroy(&ids[0]).await;
    assert_eq!(mgr.active_count().await, 1);

    mgr.destroy(&ids[2]).await;
    assert_eq!(mgr.active_count().await, 0);
}

#[tokio::test]
async fn active_count_not_affected_by_duplicate_get_or_create() {
    let mgr = EventQueueManager::new();
    let task_id = TaskId::new("task-1");

    let _ = mgr.get_or_create(&task_id).await;
    assert_eq!(mgr.active_count().await, 1);

    // Calling again for the same task should NOT increment count.
    let _ = mgr.get_or_create(&task_id).await;
    assert_eq!(mgr.active_count().await, 1);
}

// ── Constants ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn default_constants_are_sane() {
    assert_eq!(DEFAULT_QUEUE_CAPACITY, 64);
    assert_eq!(DEFAULT_MAX_EVENT_SIZE, 16 * 1024 * 1024);
}
